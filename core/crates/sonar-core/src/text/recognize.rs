//! Recognition: groups lexer pieces into typed *units* (word, number, BPM,
//! key, noise) and applies the context rules shared by the tokenizer and
//! the filename priors.
//!
//! Rules (per path segment, i.e. per file/folder name):
//! - **Explicit BPM**: a number (`120`, `92.5`) next to `bpm` on either
//!   side, joined or separated by space/`_`/`-` (`120bpm`, `120 BPM`,
//!   `120_bpm`, `BPM120`).
//! - **Bare BPM**: a standalone integer 60–200 without leading zero, only
//!   when the segment has no explicit BPM, has *tempo context* (a loop word
//!   such as `loop`/`groove`/`beat`, or a strong key) and exactly one such
//!   candidate. `Kick 75` stays a number; `Bass Loop 128` is 128 BPM.
//! - **Keys**: see [`super::music`]; a note followed by a mode word
//!   (`A minor`, `F# Min`, `AMinor`) is a key in any case.
//! - **Bare note letter** (`C`): a key only if uppercase, alone in its run,
//!   directly after a BPM unit, and the segment has no strong key
//!   (`Loop_120_C` → C; `A Kick`, `Kick A`, `Loop A 120` → words).
//! - **Noise**: zero-padded numbers (`01`, take/index numbers), versions
//!   (`v2`, `V1.2`), a number with a technical unit (`24bit`, `44.1kHz`,
//!   `48k`), common sample rates (`44100`).

use super::lex::{Piece, PieceKind};
use super::music::{
    is_bpm_marker, is_loop_word, is_sample_rate, is_tech_unit, parse_key_piece, parse_key_query,
    parse_mode_word, Key, KeyPiece,
};

/// Bare numbers in this range may be read as a BPM (with tempo context).
const BARE_BPM_RANGE: std::ops::RangeInclusive<u32> = 60..=200;
/// Explicit `N bpm` values outside this range are kept as plain numbers.
const EXPLICIT_BPM_RANGE: std::ops::RangeInclusive<f32> = 1.0..=999.0;

/// What a unit means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum UnitKind {
    Word,
    Number,
    Bpm {
        value: f32,
        explicit: bool,
    },
    /// `bare`: a lone note letter accepted by the context rule.
    Key {
        key: Key,
        bare: bool,
    },
    /// Lone uppercase note letter, pending the context rule.
    BareNote(Key),
    Noise,
}

/// One or more consecutive pieces with a meaning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Unit {
    pub kind: UnitKind,
    /// First and last piece index (inclusive).
    pub first: usize,
    pub last: usize,
    /// Byte span in the input.
    pub start: usize,
    pub end: usize,
    pub segment: u32,
}

/// Switches for the noise rules and the query mode.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NoiseRules {
    pub drop_padded_numbers: bool,
    pub drop_versions: bool,
    /// Lenient lowercase keys (`am`, `eb`), see `parse_key_query`.
    pub query: bool,
}

struct Ctx<'a> {
    text: &'a str,
    pieces: &'a [Piece],
    rules: NoiseRules,
}

impl Ctx<'_> {
    fn raw(&self, k: usize) -> &str {
        &self.text[self.pieces[k].start..self.pieces[k].end]
    }

    fn is_digit(&self, k: usize) -> bool {
        self.pieces
            .get(k)
            .is_some_and(|p| p.kind == PieceKind::Digit)
    }

    /// `b` exists, is in `a`'s segment and only space/`_`/`-` (≤ 3 chars)
    /// lie between them.
    fn adjacent(&self, a: usize, b: usize) -> bool {
        let (Some(pa), Some(pb)) = (self.pieces.get(a), self.pieces.get(b)) else {
            return false;
        };
        let gap = &self.text[pa.end..pb.start];
        pa.segment == pb.segment
            && gap.len() <= 3
            && gap.chars().all(|c| matches!(c, ' ' | '_' | '-'))
    }

    /// Last piece of a decimal number starting at digit piece `k`
    /// (`92.5` → the `5` piece), else `k`.
    fn decimal_end(&self, k: usize) -> usize {
        let dot = self.pieces.get(k + 1).is_some_and(|n| {
            self.is_digit(k + 1) && &self.text[self.pieces[k].end..n.start] == "."
        });
        if dot {
            k + 1
        } else {
            k
        }
    }

    fn number_value(&self, first: usize, last: usize) -> Option<f32> {
        let v: f32 = self.text[self.pieces[first].start..self.pieces[last].end]
            .parse()
            .ok()?;
        EXPLICIT_BPM_RANGE.contains(&v).then_some(v)
    }

    /// The piece is the only piece of its alphanumeric run.
    fn standalone(&self, k: usize) -> bool {
        let run = self.pieces[k].run;
        let prev_same = k > 0 && self.pieces[k - 1].run == run;
        let next_same = self.pieces.get(k + 1).is_some_and(|n| n.run == run);
        !prev_same && !next_same
    }

    fn digit_unit(&self, i: usize) -> (UnitKind, usize) {
        let num_end = self.decimal_end(i);
        let after = num_end + 1;
        if self.adjacent(num_end, after) && !self.is_digit(after) {
            let marker = self.raw(after);
            if is_bpm_marker(marker) {
                if let Some(value) = self.number_value(i, num_end) {
                    let explicit = true;
                    return (UnitKind::Bpm { value, explicit }, after);
                }
            } else if is_tech_unit(marker) {
                return (UnitKind::Noise, after);
            }
        }
        let digits = self.raw(i);
        if self.rules.drop_padded_numbers && digits.len() >= 2 && digits.starts_with('0') {
            return (UnitKind::Noise, i);
        }
        if digits.parse::<u64>().is_ok_and(is_sample_rate) {
            return (UnitKind::Noise, i);
        }
        (UnitKind::Number, i)
    }

    fn alpha_unit(&self, i: usize) -> (UnitKind, usize) {
        let raw = self.raw(i);
        let next = i + 1;
        if is_bpm_marker(raw) && self.is_digit(next) && self.adjacent(i, next) {
            let end = self.decimal_end(next);
            if let Some(value) = self.number_value(next, end) {
                let explicit = true;
                return (UnitKind::Bpm { value, explicit }, end);
            }
        }
        let same_run = |k: usize| {
            self.pieces
                .get(k)
                .is_some_and(|p| p.run == self.pieces[i].run)
        };
        if self.rules.drop_versions
            && raw.eq_ignore_ascii_case("v")
            && self.is_digit(next)
            && same_run(next)
        {
            let mut end = next;
            while self.decimal_end(end) != end {
                end += 1;
            }
            return (UnitKind::Noise, end);
        }
        let mode_next = (self.adjacent(i, next) && !self.is_digit(next))
            .then(|| parse_mode_word(self.raw(next)))
            .flatten();
        match parse_key_piece(raw) {
            Some(KeyPiece::Strong(key)) | Some(KeyPiece::Note { key, .. })
                if key.mode.is_none() && mode_next.is_some() =>
            {
                let key = Key {
                    mode: mode_next,
                    ..key
                };
                (UnitKind::Key { key, bare: false }, next)
            }
            Some(KeyPiece::Strong(key)) => (UnitKind::Key { key, bare: false }, i),
            Some(KeyPiece::Note {
                key,
                upper_letter: true,
                accidental: false,
            }) if self.standalone(i) => (UnitKind::BareNote(key), i),
            _ => match self.rules.query.then(|| parse_key_query(raw)).flatten() {
                Some(key) => (UnitKind::Key { key, bare: false }, i),
                None => (UnitKind::Word, i),
            },
        }
    }
}

/// Recognizes `pieces` of `text` into `out` (cleared first).
pub(crate) fn recognize(text: &str, pieces: &[Piece], rules: NoiseRules, out: &mut Vec<Unit>) {
    out.clear();
    let cx = Ctx {
        text,
        pieces,
        rules,
    };
    let mut i = 0;
    while i < pieces.len() {
        let (kind, last) = match pieces[i].kind {
            PieceKind::Digit => cx.digit_unit(i),
            PieceKind::Alpha => cx.alpha_unit(i),
        };
        out.push(Unit {
            kind,
            first: i,
            last,
            start: pieces[i].start,
            end: pieces[last].end,
            segment: pieces[i].segment,
        });
        i = last + 1;
    }
    let mut seg_start = 0;
    while seg_start < out.len() {
        let seg = out[seg_start].segment;
        let len = out[seg_start..]
            .iter()
            .take_while(|u| u.segment == seg)
            .count();
        resolve_segment(&cx, &mut out[seg_start..seg_start + len]);
        seg_start += len;
    }
}

/// Applies the bare-BPM and bare-note context rules within one segment.
fn resolve_segment(cx: &Ctx<'_>, units: &mut [Unit]) {
    let explicit_bpm = units.iter().any(|u| matches!(u.kind, UnitKind::Bpm { .. }));
    let strong_key = units.iter().any(|u| matches!(u.kind, UnitKind::Key { .. }));
    let loop_word = units
        .iter()
        .any(|u| u.kind == UnitKind::Word && is_loop_word(&cx.text[u.start..u.end]));

    if !explicit_bpm && (loop_word || strong_key) {
        let bare_value = |u: &Unit| -> Option<u32> {
            let digits = &cx.text[u.start..u.end];
            (u.kind == UnitKind::Number && !digits.starts_with('0') && cx.standalone(u.first))
                .then(|| digits.parse().ok())
                .flatten()
                .filter(|v| BARE_BPM_RANGE.contains(v))
        };
        let mut values = units.iter().filter_map(bare_value);
        if let Some(first) = values.next() {
            if values.all(|v| v == first) {
                for u in units.iter_mut().filter(|u| bare_value(u).is_some()) {
                    // Integers ≤ 200 are exact in f32.
                    let value = first as f32;
                    u.kind = UnitKind::Bpm {
                        value,
                        explicit: false,
                    };
                }
            }
        }
    }

    for k in 0..units.len() {
        if let UnitKind::BareNote(key) = units[k].kind {
            let after_bpm = k > 0 && matches!(units[k - 1].kind, UnitKind::Bpm { .. });
            units[k].kind = if after_bpm && !strong_key {
                UnitKind::Key { key, bare: true }
            } else {
                UnitKind::Word
            };
        }
    }
}

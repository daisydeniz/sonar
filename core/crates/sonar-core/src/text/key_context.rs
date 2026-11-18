//! Context rules that turn a lone note letter (`C`) or a lowercase key
//! spelling (`am`, `eb`) in a file name into a key (T2.1, T10.1).
//!
//! Applied per path segment after the BPM rules (see
//! [`super::recognize`]). A unit is a key only with enough evidence in the
//! same name; otherwise it stays a word:
//! - **Lowercase key** (`am`, `gm`, `eb`, `dbm`; one whole run): the name
//!   has a BPM (explicit or bare) or a loop word. Spellings that are often
//!   something else ([`AMBIGUOUS_LOWER`]: `fm` synthesis, `db` decibels,
//!   `gb`, `ebm` genre, `dbm`) only directly after the BPM (`loop_120_fm`).
//!   Accepted as a *bare* (non-explicit) key.
//! - **Bare uppercase letter** (`C`, alone in its run), only when the name
//!   has no strong key and the letter does not follow a variant marker
//!   (`Take B`, `Vol A`, [`VARIANT_MARKERS`]):
//!   - directly after a BPM (`Loop_120_C`) → bare key;
//!   - in brackets, `(C)` / `[C]`, when the name has a tonal word
//!     ([`TONAL_WORDS`], `808`) or — without a drum word
//!     ([`DRUM_WORDS`]) — a BPM or loop word → explicit key
//!     (`Tom Loop 08 (C)(120 BPM)`);
//!   - last in the name (a trailing media extension is ignored), when the
//!     name has a tonal word (`Bass_808_C`, `Piano_C`) → bare key.
//!
//! A bare letter has no mode (`Key::mode == None`, conventionally major).

use super::music::is_loop_word;
use super::recognize::{Unit, UnitKind};
use super::tokenize::is_media_extension;

/// Words naming pitched/tonal sources: a trailing note letter in such a
/// name is the sample's note (`Sub Bass C`, `Tom_Low_G`).
const TONAL_WORDS: &[&str] = &[
    "bass", "basses", "sub", "subs", "reese", "pluck", "plucks", "lead", "leads", "keys", "piano",
    "epiano", "rhodes", "organ", "synth", "synths", "pad", "pads", "stab", "stabs", "chord",
    "chords", "arp", "bell", "bells", "marimba", "kalimba", "mallet", "mallets", "vibes", "guitar",
    "string", "strings", "brass", "horn", "horns", "flute", "tom", "toms", "dunun", "surdo",
    "timpani", "tabla",
];

/// Unpitched drum words: a bracketed letter in a drum name is more likely
/// a variation (`Kick Loop (A) 120bpm`) than a key.
const DRUM_WORDS: &[&str] = &[
    "kick",
    "kicks",
    "snare",
    "snares",
    "hat",
    "hats",
    "hihat",
    "hihats",
    "clap",
    "claps",
    "drum",
    "drums",
    "perc",
    "percussion",
    "cymbal",
    "crash",
    "ride",
    "shaker",
    "rim",
    "top",
    "tops",
    "fx",
    "sfx",
];

/// Words after which a letter is a variant label, not a note.
const VARIANT_MARKERS: &[&str] = &[
    "take",
    "vol",
    "volume",
    "part",
    "pt",
    "var",
    "variation",
    "version",
    "ver",
    "mix",
    "side",
    "kit",
    "layer",
    "group",
    "set",
    "alt",
    "type",
    "rr",
    "no",
    "round",
    "velocity",
    "vel",
];

/// Lowercase spellings that are usually not keys; accepted only directly
/// after a BPM.
const AMBIGUOUS_LOWER: &[&str] = &["fm", "db", "gb", "ebm", "dbm"];

fn in_list(list: &[&str], raw: &str) -> bool {
    list.iter().any(|w| raw.eq_ignore_ascii_case(w))
}

/// Resolves pending `LowerKey` / `BareNote` units of one segment into keys
/// or words (see module docs). `strong_key`: the segment had a key unit
/// before this pass.
pub(crate) fn resolve_keys(text: &str, units: &mut [Unit], strong_key: bool) {
    let raw = |u: &Unit| &text[u.start..u.end];
    let word_in = |u: &Unit, list: &[&str]| u.kind == UnitKind::Word && in_list(list, raw(u));
    let has_bpm = units.iter().any(|u| matches!(u.kind, UnitKind::Bpm { .. }));
    let loop_word = units
        .iter()
        .any(|u| u.kind == UnitKind::Word && is_loop_word(raw(u)));
    let tonal = units
        .iter()
        .any(|u| word_in(u, TONAL_WORDS) || (u.kind == UnitKind::Number && raw(u) == "808"));
    let drum = units.iter().any(|u| word_in(u, DRUM_WORDS));
    let after_bpm =
        |units: &[Unit], k: usize| k > 0 && matches!(units[k - 1].kind, UnitKind::Bpm { .. });

    let mut lower_key = false;
    for k in 0..units.len() {
        if let UnitKind::LowerKey(key) = units[k].kind {
            let ambiguous = in_list(AMBIGUOUS_LOWER, raw(&units[k]));
            let ok = (has_bpm || loop_word) && (!ambiguous || after_bpm(units, k));
            lower_key |= ok;
            units[k].kind = if ok {
                UnitKind::Key { key, bare: true }
            } else {
                UnitKind::Word
            };
        }
    }

    let blocked = strong_key || lower_key;
    for k in 0..units.len() {
        let UnitKind::BareNote(key) = units[k].kind else {
            continue;
        };
        let variant = k > 0 && word_in(&units[k - 1], VARIANT_MARKERS);
        units[k].kind = if blocked || variant {
            UnitKind::Word
        } else if after_bpm(units, k) {
            UnitKind::Key { key, bare: true }
        } else if bracketed(text, &units[k]) && (tonal || (!drum && (has_bpm || loop_word))) {
            UnitKind::Key { key, bare: false }
        } else if tonal && is_last(text, units, k) {
            UnitKind::Key { key, bare: true }
        } else {
            UnitKind::Word
        };
    }
}

/// `u` is directly enclosed in `()` or `[]`.
fn bracketed(text: &str, u: &Unit) -> bool {
    let before = text[..u.start].chars().next_back();
    let after = text[u.end..].chars().next();
    matches!(
        (before, after),
        (Some('('), Some(')')) | (Some('['), Some(']'))
    )
}

/// Unit `k` is the last of the segment, or only a `.wav`-style media
/// extension follows it.
fn is_last(text: &str, units: &[Unit], k: usize) -> bool {
    match &units[k + 1..] {
        [] => true,
        [ext] => {
            ext.kind == UnitKind::Word
                && &text[units[k].end..ext.start] == "."
                && is_media_extension(&text[ext.start..ext.end])
        }
        _ => false,
    }
}

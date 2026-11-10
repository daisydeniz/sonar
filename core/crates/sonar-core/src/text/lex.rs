//! Lexer: splits text into alphanumeric *pieces* with byte offsets, and
//! folds piece text (lowercase, diacritics removed).
//!
//! Boundaries:
//! - separators: every char that is not a letter, digit or combining mark
//!   (`_ - . space ( ) [ ] , + &` …); `/` and `\` also start a new path
//!   *segment*;
//! - letter↔digit (`TR808` → `TR` `808`, `120bpm` → `120` `bpm`);
//! - camelCase: lower→Upper (`DeepHouse` → `Deep` `House`) and acronym end
//!   Upper→Upper+lower (`TRKick` → `TR` `Kick`).
//!
//! `#`, `♯` and `♭` directly after a lone note letter stay in the piece
//! (`F#m`, `C♯`); elsewhere they are separators. Apostrophes between letters
//! are dropped without a boundary (`Don't` → `dont`).

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Character class of a piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PieceKind {
    /// Letters (any script), plus accidentals after a note letter.
    Alpha,
    /// ASCII digits.
    Digit,
}

/// One alphanumeric piece of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Piece {
    /// Byte offset of the first char in the input.
    pub start: usize,
    /// Byte offset one past the last char.
    pub end: usize,
    pub kind: PieceKind,
    /// Index of the maximal alphanumeric run (text between separators).
    pub run: u32,
    /// Path segment index (incremented at `/` and `\`).
    pub segment: u32,
    /// Split from the previous piece of the same run by a case change
    /// (not by a letter↔digit change).
    pub case_split: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Upper,
    /// Lowercase or caseless letters, combining marks, accidentals.
    Lower,
    Digit,
    /// Apostrophe: no boundary between letters, else a separator.
    Join,
    Sep,
    PathSep,
}

fn is_note_letter(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a'..='g')
}

/// Splits `text` into pieces (see module docs).
pub(crate) fn lex(text: &str, out: &mut Vec<Piece>) {
    out.clear();
    let mut cur: Option<Piece> = None;
    let mut prev_class = Class::Sep;
    let mut run: u32 = 0;
    let mut segment: u32 = 0;
    let mut chars = text.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        let next = chars.peek().map(|&(_, n)| n);
        let class = classify(c, i, cur.as_ref(), text, next);
        match class {
            Class::Sep | Class::PathSep => {
                close(&mut cur, out, &mut run);
                if class == Class::PathSep {
                    segment += 1;
                }
            }
            Class::Join => {} // inside a word (checked by `classify`)
            Class::Digit | Class::Upper | Class::Lower => {
                let kind = if class == Class::Digit {
                    PieceKind::Digit
                } else {
                    PieceKind::Alpha
                };
                let end = i + c.len_utf8();
                let fresh = |case_split| Piece {
                    start: i,
                    end,
                    kind,
                    run,
                    segment,
                    case_split,
                };
                match cur.as_mut() {
                    None => cur = Some(fresh(false)),
                    Some(p) if p.kind != kind => {
                        out.push(*p);
                        *p = fresh(false);
                    }
                    Some(p) => {
                        let acronym_end =
                            prev_class == Class::Upper && next.is_some_and(char::is_lowercase);
                        if class == Class::Upper && (prev_class == Class::Lower || acronym_end) {
                            out.push(*p);
                            *p = fresh(true);
                        } else {
                            p.end = end;
                        }
                    }
                }
            }
        }
        prev_class = class;
    }
    close(&mut cur, out, &mut run);
}

fn close(cur: &mut Option<Piece>, out: &mut Vec<Piece>, run: &mut u32) {
    if let Some(p) = cur.take() {
        out.push(p);
        *run += 1;
    }
}

fn classify(c: char, i: usize, cur: Option<&Piece>, text: &str, next: Option<char>) -> Class {
    if c.is_ascii_digit() {
        return Class::Digit;
    }
    if c == '/' || c == '\\' {
        return Class::PathSep;
    }
    if matches!(c, '#' | '♯' | '♭') {
        // Accidental only right after a lone note letter (`F#`, `B♭`).
        let lone_note = cur.is_some_and(|p| {
            p.kind == PieceKind::Alpha
                && p.end == i
                && p.end - p.start == 1
                && text[p.start..p.end].chars().all(is_note_letter)
        });
        return if lone_note { Class::Lower } else { Class::Sep };
    }
    if matches!(c, '\'' | '’') {
        let in_word = cur.is_some_and(|p| p.kind == PieceKind::Alpha && p.end == i);
        return if in_word && next.is_some_and(char::is_alphabetic) {
            Class::Join
        } else {
            Class::Sep
        };
    }
    if c.is_uppercase() {
        Class::Upper
    } else if c.is_alphabetic() || is_combining_mark(c) {
        Class::Lower
    } else {
        Class::Sep
    }
}

/// Appends the folded form of `raw` to `out`: lowercase, diacritics removed
/// (NFD, combining marks dropped, a few ligatures expanded), apostrophes
/// dropped, `♯`/`♭` → `#`/`b`.
pub(crate) fn fold_into(raw: &str, out: &mut String) {
    if raw.is_ascii() {
        out.extend(
            raw.bytes()
                .filter(|&b| b != b'\'')
                .map(|b| char::from(b.to_ascii_lowercase())),
        );
        return;
    }
    for c in raw.nfd() {
        match c {
            '\'' | '’' => {}
            '♯' => out.push('#'),
            '♭' => out.push('b'),
            'ß' => out.push_str("ss"),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            'ø' | 'Ø' => out.push('o'),
            'đ' | 'Đ' => out.push('d'),
            'ł' | 'Ł' => out.push('l'),
            'ı' => out.push('i'),
            'þ' | 'Þ' => out.push_str("th"),
            c if is_combining_mark(c) => {}
            c => out.extend(c.to_lowercase()),
        }
    }
}

/// Folded copy of `raw` (see [`fold_into`]).
pub(crate) fn fold(raw: &str) -> String {
    let mut s = String::with_capacity(raw.len());
    fold_into(raw, &mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(text: &str) -> Vec<String> {
        let mut v = Vec::new();
        lex(text, &mut v);
        v.iter().map(|p| text[p.start..p.end].to_owned()).collect()
    }

    #[test]
    fn splits_table() {
        let cases: &[(&str, &[&str])] = &[
            (
                "Deep_House-Kick 01.wav",
                &["Deep", "House", "Kick", "01", "wav"],
            ),
            ("DeepHouseKick", &["Deep", "House", "Kick"]),
            ("TR808Kick", &["TR", "808", "Kick"]),
            ("HTMLParser", &["HTML", "Parser"]),
            ("120BPM", &["120", "BPM"]),
            ("F#m", &["F#m"]),
            ("F#Minor", &["F#", "Minor"]),
            ("Kick#2", &["Kick", "2"]),
            ("C♯ B♭", &["C♯", "B♭"]),
            ("Don't Stop", &["Don't", "Stop"]),
            ("'quoted'", &["quoted"]),
            ("(Wet) [Dry]", &["Wet", "Dry"]),
            ("", &[]),
        ];
        for (input, want) in cases {
            assert_eq!(pieces(input), *want, "input {input:?}");
        }
    }

    #[test]
    fn runs_segments_and_case_splits() {
        let mut v = Vec::new();
        lex("DnB/x1", &mut v);
        let summary: Vec<_> = v.iter().map(|p| (p.run, p.segment, p.case_split)).collect();
        assert_eq!(
            summary,
            [(0, 0, false), (0, 0, true), (1, 1, false), (1, 1, false)]
        );
    }

    #[test]
    fn folds_case_and_diacritics() {
        let cases = [
            ("Kick", "kick"),
            ("Café", "cafe"),
            ("Cafe\u{301}", "cafe"), // NFD input (macOS file names)
            ("Straße", "strasse"),
            ("Ørsted", "orsted"),
            ("C♯", "c#"),
            ("B♭", "bb"),
            ("Don't", "dont"),
            ("ÜBER", "uber"),
        ];
        for (raw, want) in cases {
            assert_eq!(fold(raw), want, "raw {raw:?}");
        }
    }
}

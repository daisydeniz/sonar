//! Musical keys for filters, facets and sort: a compact code per key,
//! lenient parsing of stored/typed key values and harmonic compatibility
//! on the Camelot wheel.
//!
//! **Code**: `root * 2 + minor` (0..24): `C`=0, `Cm`=1, `C#`=2, … `Bm`=23.
//! A key without a mode (`C`) counts as major, like the `key:` tokens
//! (ADR-016). Sorting by code orders keys by root, major before minor.
//!
//! **Compatible keys** (harmonic mixing): the Camelot wheel (Mark Davis,
//! Mixed In Key) lays the 24 keys on the circle of fifths, numbered 1–12,
//! minor = `A`, major = `B`. A key mixes well with itself, its two
//! neighbours on the wheel (±1: a fifth up/down, same mode) and its
//! relative major/minor (same number, other letter): `Am` (8A) → `Am`,
//! `Dm` (7A), `Em` (9A), `C` (8B).

use crate::text::music::{parse_key_piece, KeyPiece};
use crate::text::{Key, Mode};

/// Number of distinct key codes.
pub const KEY_CODES: u8 = 24;

/// Display names of the pitch classes (sharps).
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Code of `key` (see the module docs); mode `None` counts as major.
pub fn key_code(key: Key) -> u8 {
    (key.root % 12) * 2 + u8::from(key.mode == Some(Mode::Minor))
}

/// The key of `code` (mode always set), `None` for codes ≥ 24.
pub fn key_from_code(code: u8) -> Option<Key> {
    let mode = if code % 2 == 1 {
        Mode::Minor
    } else {
        Mode::Major
    };
    (code < KEY_CODES).then_some(Key {
        root: code / 2,
        mode: Some(mode),
    })
}

/// Short label of a code: `C`, `C#m`, `A#` (sharps; major has no suffix).
/// Unknown codes give `?`.
pub fn key_label(code: u8) -> String {
    match key_from_code(code) {
        Some(k) => {
            let m = if k.mode == Some(Mode::Minor) { "m" } else { "" };
            format!("{}{m}", NAMES[usize::from(k.root)])
        }
        None => "?".into(),
    }
}

/// Camelot position of a code: number 1–12 and `true` for minor (`A`).
pub fn camelot(code: u8) -> (u8, bool) {
    let root = code / 2 % 12;
    let minor = code % 2 == 1;
    // Relative major of a minor key: three semitones up.
    let major_root = if minor { (root + 3) % 12 } else { root };
    // Major keys: C = 8B and every fifth (+7 semitones) is +1 on the wheel.
    ((major_root * 7 + 7) % 12 + 1, minor)
}

/// Code of a Camelot position (`number` 1–12), `None` if out of range.
pub fn from_camelot(number: u8, minor: bool) -> Option<u8> {
    if !(1..=12).contains(&number) {
        return None;
    }
    // Inverse of `camelot` (n − 1 ≡ 7·(root + 1)): 7 is its own inverse
    // mod 12 (7 · 7 = 49 ≡ 1), so root ≡ 7·(n − 1) − 1 ≡ 7·n + 4.
    let major_root = (number * 7 + 4) % 12;
    let root = if minor {
        (major_root + 9) % 12
    } else {
        major_root
    };
    Some(root * 2 + u8::from(minor))
}

/// The key itself and its harmonically compatible keys (module docs):
/// same key, ±1 on the Camelot wheel, relative major/minor. Four codes,
/// the key's own first.
pub fn compatible_codes(code: u8) -> [u8; 4] {
    let (n, minor) = camelot(code);
    let wrap = |n: u8| (n + 11) % 12 + 1; // 0 → 12, 13 → 1
    let at = |n: u8, minor: bool| from_camelot(wrap(n), minor).unwrap_or(code);
    [code, at(n + 11, minor), at(n + 1, minor), at(n, !minor)]
}

/// Parses a key as stored (analysis, overrides) or typed in a filter:
/// `Am`, `A minor`, `a min`, `C#`, `Db`, `Bbm`, `F#MAJ`, `key:c#m` (index
/// token) or a Camelot code (`8A`, `12b`). Case-insensitive; `None` if it
/// is not a key.
pub fn parse_key(raw: &str) -> Option<Key> {
    let s = raw.trim();
    let s = s.strip_prefix("key:").unwrap_or(s);
    if let Some(code) = parse_camelot(s) {
        return key_from_code(code);
    }
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut chars = compact.chars();
    let first = chars.next()?.to_ascii_uppercase();
    // Uppercase letter + lowercase rest is the strict spelling the name
    // recogniser accepts (`Am`, `Bbm`, `C#min`).
    let normal = format!("{first}{}", chars.as_str().to_lowercase());
    match parse_key_piece(&normal)? {
        KeyPiece::Strong(k) => Some(k),
        KeyPiece::Note { key, .. } => Some(key),
    }
}

/// `8A` / `12b` → code.
fn parse_camelot(s: &str) -> Option<u8> {
    let letter = s.chars().last()?;
    let num = &s[..s.len() - letter.len_utf8()];
    let minor = match letter {
        'A' | 'a' => true,
        'B' | 'b' => false,
        _ => return None,
    };
    from_camelot(num.parse().ok()?, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(s: &str) -> u8 {
        key_code(parse_key(s).unwrap_or_else(|| panic!("not a key: {s}")))
    }

    #[test]
    fn codes_labels_and_parsing() {
        assert_eq!(
            (code("C"), code("Cm"), code("C#"), code("Bm")),
            (0, 1, 2, 23)
        );
        for c in 0..KEY_CODES {
            assert_eq!(code(&key_label(c)), c, "round trip {c}");
        }
        for (raw, want) in [
            ("Am", "Am"),
            ("a minor", "Am"),
            ("A MIN", "Am"),
            ("am", "Am"),
            ("Db", "C#"),
            ("Bbm", "A#m"),
            ("bbm", "A#m"),
            ("F#maj", "F#"),
            ("key:c#m", "C#m"),
            ("key:a#", "A#"),
            ("8A", "Am"),
            ("8b", "C"),
            ("12A", "C#m"),
            ("1B", "B"),
        ] {
            assert_eq!(key_label(code(raw)), want, "{raw}");
        }
        for bad in ["", "H", "kick", "13A", "0B", "8C", "key:"] {
            assert!(parse_key(bad).is_none(), "{bad}");
        }
        assert_eq!(key_label(24), "?");
    }

    #[test]
    fn camelot_numbers() {
        for (key, n, minor) in [
            ("C", 8, false),
            ("Am", 8, true),
            ("G", 9, false),
            ("E", 12, false),
            ("B", 1, false),
            ("G#m", 1, true),
            ("Cm", 5, true),
            ("C#m", 12, true),
        ] {
            assert_eq!(camelot(code(key)), (n, minor), "{key}");
        }
        for c in 0..KEY_CODES {
            let (n, m) = camelot(c);
            assert_eq!(from_camelot(n, m), Some(c));
        }
    }

    /// All 24 keys: [self, wheel −1, wheel +1, relative], from the
    /// published Camelot wheel.
    #[test]
    fn compatible_keys_table() {
        let table: [(&str, [&str; 3]); 24] = [
            ("C", ["F", "G", "Am"]),
            ("C#", ["F#", "G#", "A#m"]),
            ("D", ["G", "A", "Bm"]),
            ("D#", ["G#", "A#", "Cm"]),
            ("E", ["A", "B", "C#m"]),
            ("F", ["A#", "C", "Dm"]),
            ("F#", ["B", "C#", "D#m"]),
            ("G", ["C", "D", "Em"]),
            ("G#", ["C#", "D#", "Fm"]),
            ("A", ["D", "E", "F#m"]),
            ("A#", ["D#", "F", "Gm"]),
            ("B", ["E", "F#", "G#m"]),
            ("Cm", ["Fm", "Gm", "D#"]),
            ("C#m", ["F#m", "G#m", "E"]),
            ("Dm", ["Gm", "Am", "F"]),
            ("D#m", ["G#m", "A#m", "F#"]),
            ("Em", ["Am", "Bm", "G"]),
            ("Fm", ["A#m", "Cm", "G#"]),
            ("F#m", ["Bm", "C#m", "A"]),
            ("Gm", ["Cm", "Dm", "A#"]),
            ("G#m", ["C#m", "D#m", "B"]),
            ("Am", ["Dm", "Em", "C"]),
            ("A#m", ["D#m", "Fm", "C#"]),
            ("Bm", ["Em", "F#m", "D"]),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for (key, others) in table {
            let got = compatible_codes(code(key)).map(key_label);
            assert_eq!(got[0], key);
            assert_eq!(got[1..], others.map(String::from), "{key}");
            seen.insert(code(key));
        }
        assert_eq!(seen.len(), 24);
    }
}

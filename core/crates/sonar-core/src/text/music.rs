//! Musical vocabulary: keys, notes and BPM markers in file names.
//!
//! Key spelling accepted in one piece: note letter `A`–`G`, optional
//! accidental (`#`, `♯`, `b`, `♭`, `sharp`, `flat`), optional mode (`m`,
//! `min`, `minor`, `maj`, `major`). Enharmonics are normalised to sharps
//! (`Gb` = `F#`, `Cb` = `B`). Case rules keep common words out:
//! - uppercase note letter: any accidental/mode, but the short minor `m`
//!   must be lowercase and a `b` accidental must be lowercase (`FM`, `AM`,
//!   `dB`, `BB` are words, not keys);
//! - lowercase note letter: only with an unambiguous accidental (`#`, `♯`,
//!   `♭`, `sharp`, `flat`) or a long mode (`min`, `maj`, …): `c#m`, `cmin`
//!   are keys; `am`, `fm`, `eb`, `bb` are words;
//! - a bare letter (`A`, `C`) is never a key on its own (see `tokenize` for
//!   the context rule).

use std::fmt;

use serde::Serialize;

/// Pitch-class names (sharps), index = semitones above C. Used in tokens.
const PC_NAMES: [&str; 12] = [
    "c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b",
];
/// Display names (sharps, uppercase).
const PC_DISPLAY: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Major or minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Major (`maj`, `major`).
    Major,
    /// Minor (`m`, `min`, `minor`).
    Minor,
}

/// A musical key (or a bare note when `mode` is `None`), enharmonically
/// normalised to sharps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Key {
    /// Pitch class of the tonic: semitones above C (0 = C … 11 = B).
    pub root: u8,
    /// Mode, if the name says it (`C` alone → `None`, conventionally major).
    pub mode: Option<Mode>,
}

impl Key {
    /// Lowercase sharp name of the root (`"c#"`).
    pub fn root_name(&self) -> &'static str {
        PC_NAMES[usize::from(self.root % 12)]
    }

    /// Canonical key token: `key:<root>` plus `m` for minor (`key:c#m`,
    /// `key:a#`). Major and unspecified mode share `key:<root>`.
    pub fn token(&self) -> String {
        let m = if self.mode == Some(Mode::Minor) {
            "m"
        } else {
            ""
        };
        format!("key:{}{m}", self.root_name())
    }

    /// Canonical note token, ignoring mode: `note:<root>` (`note:c#`).
    pub fn note_token(&self) -> String {
        format!("note:{}", self.root_name())
    }
}

impl fmt::Display for Key {
    /// `C#m`, `A#`, `Cmaj` (explicit major).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suffix = match self.mode {
            Some(Mode::Minor) => "m",
            Some(Mode::Major) => "maj",
            None => "",
        };
        write!(f, "{}{suffix}", PC_DISPLAY[usize::from(self.root % 12)])
    }
}

/// A key-like piece before context rules are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyPiece {
    /// Unambiguous key/note (`Am`, `F#`, `cmin`, `Gbmaj`).
    Strong(Key),
    /// Note letter with optional accidental and no mode, not strong enough
    /// alone (`a`, `C`, `eb`): becomes a key only next to a mode word or
    /// by the bare-letter rule.
    Note {
        key: Key,
        upper_letter: bool,
        accidental: bool,
    },
}

fn letter_pc(c: char) -> Option<i8> {
    Some(match c.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    })
}

/// Strips an ASCII-case-insensitive prefix.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Parses a long mode word (`min`, `minor`, `maj`, `major`; any case).
pub(crate) fn parse_mode_word(raw: &str) -> Option<Mode> {
    match raw.to_ascii_lowercase().as_str() {
        "min" | "minor" => Some(Mode::Minor),
        "maj" | "major" => Some(Mode::Major),
        _ => None,
    }
}

/// Parses one raw (case-preserved) piece as a key/note (see module docs).
pub(crate) fn parse_key_piece(raw: &str) -> Option<KeyPiece> {
    let mut chars = raw.chars();
    let letter = chars.next()?;
    let base = letter_pc(letter)?;
    let upper = letter.is_ascii_uppercase();
    let rest = chars.as_str();

    // (semitone shift, unambiguous?) and the remainder after it.
    let (shift, clear_acc, rest) =
        if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
            (1, true, r)
        } else if let Some(r) = rest.strip_prefix('♭') {
            (-1, true, r)
        } else if let Some(r) = strip_prefix_ci(rest, "sharp") {
            (1, true, r)
        } else if let Some(r) = strip_prefix_ci(rest, "flat") {
            (-1, true, r)
        } else if let Some(r) = rest.strip_prefix('b') {
            (-1, false, r)
        } else {
            (0, false, rest)
        };
    let (mode, long_mode) = match rest {
        "" => (None, false),
        "m" => (Some(Mode::Minor), false),
        r => (Some(parse_mode_word(r)?), true),
    };
    // `rem_euclid(12)` of a value in -1..=12 always fits in u8.
    let root = u8::try_from((base + shift).rem_euclid(12)).ok()?;
    let key = Key { root, mode };
    let accidental = shift != 0;

    let strong = if upper {
        accidental || mode.is_some()
    } else {
        clear_acc || long_mode
    };
    if strong {
        Some(KeyPiece::Strong(key))
    } else if mode.is_none() {
        Some(KeyPiece::Note {
            key,
            upper_letter: upper,
            accidental,
        })
    } else {
        None // lowercase letter + short `m` (`am`, `fm`): a word
    }
}

/// Query mode only: an all-lowercase piece read as if its note letter were
/// uppercase (`am` → Am, `eb` → D#, `dbm` → C#m, `fm` → Fm). Users type
/// lowercase queries; names on disk keep the strict rules. A bare letter
/// (`a`) is still not a key.
pub(crate) fn parse_key_query(raw: &str) -> Option<Key> {
    if raw.chars().any(char::is_uppercase) {
        return None;
    }
    let mut chars = raw.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let capitalised = format!("{letter}{}", chars.as_str());
    match parse_key_piece(&capitalised)? {
        KeyPiece::Strong(key) => Some(key),
        KeyPiece::Note { .. } => None,
    }
}

/// True for the BPM marker word (`bpm`, any case).
pub(crate) fn is_bpm_marker(raw: &str) -> bool {
    raw.eq_ignore_ascii_case("bpm")
}

/// Unit words after a number that mark technical noise (`24bit`,
/// `44.1kHz`, `48k`, `320kbps`): the number is never a BPM.
pub(crate) fn is_tech_unit(raw: &str) -> bool {
    ["bit", "bits", "khz", "hz", "k", "kbps"]
        .iter()
        .any(|u| raw.eq_ignore_ascii_case(u))
}

/// Common sample rates: dropped as numbers (never a BPM, never useful).
pub(crate) fn is_sample_rate(n: u64) -> bool {
    matches!(
        n,
        8_000
            | 11_025
            | 16_000
            | 22_050
            | 32_000
            | 44_100
            | 48_000
            | 88_200
            | 96_000
            | 176_400
            | 192_000
    )
}

/// Words that make a bare number in the same name a likely tempo.
pub(crate) fn is_loop_word(raw: &str) -> bool {
    [
        "loop",
        "loops",
        "groove",
        "grooves",
        "beat",
        "beats",
        "break",
        "breaks",
        "breakbeat",
        "riff",
        "riffs",
        "phrase",
        "progression",
    ]
    .iter()
    .any(|w| raw.eq_ignore_ascii_case(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strong(raw: &str) -> Option<String> {
        match parse_key_piece(raw) {
            Some(KeyPiece::Strong(k)) => Some(k.to_string()),
            _ => None,
        }
    }

    #[test]
    fn strong_keys_table() {
        let cases: &[(&str, Option<&str>)] = &[
            ("Am", Some("Am")),
            ("Cmin", Some("Cm")),
            ("C#m", Some("C#m")),
            ("c#m", Some("C#m")),
            ("Gbmaj", Some("F#maj")),
            ("Gb", Some("F#")),
            ("Bb", Some("A#")),
            ("Ebm", Some("D#m")),
            ("F#", Some("F#")),
            ("f#", Some("F#")),
            ("C♯", Some("C#")),
            ("B♭m", Some("A#m")),
            ("Fsharp", Some("F#")),
            ("Bflat", Some("A#")),
            ("Cb", Some("B")),
            ("CMIN", Some("Cm")),
            ("Emajor", Some("Emaj")),
            ("cmin", Some("Cm")),
            ("ebminor", Some("D#m")),
            // Words, not keys.
            ("FM", None),
            ("AM", None),
            ("dB", None),
            ("BB", None),
            ("am", None),
            ("fm", None),
            ("eb", None),
            ("Bass", None),
            ("Ambient", None),
            ("Dub", None),
            ("Abs", None),
            ("A", None),
            ("Hm", None),
        ];
        for (raw, want) in cases {
            assert_eq!(strong(raw).as_deref(), *want, "raw {raw:?}");
        }
    }

    #[test]
    fn note_pieces_need_context() {
        assert!(matches!(
            parse_key_piece("A"),
            Some(KeyPiece::Note {
                upper_letter: true,
                accidental: false,
                ..
            })
        ));
        assert!(matches!(
            parse_key_piece("eb"),
            Some(KeyPiece::Note {
                upper_letter: false,
                accidental: true,
                ..
            })
        ));
        assert_eq!(parse_key_piece("am"), None);
    }

    #[test]
    fn tokens_and_display() {
        let k = Key {
            root: 1,
            mode: Some(Mode::Minor),
        };
        assert_eq!(
            (k.token(), k.note_token()),
            ("key:c#m".into(), "note:c#".into())
        );
        let k = Key {
            root: 10,
            mode: Some(Mode::Major),
        };
        assert_eq!(
            (k.token(), k.to_string()),
            ("key:a#".into(), "A#maj".into())
        );
    }
}

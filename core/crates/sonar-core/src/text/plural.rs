//! Minimal English plural folding (`kicks` → `kick`, `hihats` → `hihat`).
//!
//! Sample libraries name folders in the plural (`Kicks/`, `Loops/`,
//! `Vocals/`) and files in the singular, so without this a search for
//! `kick` misses `Kicks/BD_01.wav`. The tokenizer keeps the original word
//! and adds the singular at the same position (like Elasticsearch
//! `keyword_repeat` + stemmer), on both the index and the query side.
//!
//! Rules: Lucene's `EnglishMinimalStemmer` (Harman's "S-stemmer", Harman
//! 1991, *How effective is suffixing?*, JASIS 42(1)) plus `-es` after a
//! sibilant (`crashes` → `crash`, `glitches` → `glitch`, `boxes` → `box`),
//! and words ending in `is` are kept (`analysis`). Only ASCII-letter words
//! of 4+ bytes are touched, so `bass`, `fx`, `vox`, `808` and key tokens are
//! never changed.

/// Singular of the folded word `w`, or `None` if the rules keep it as is.
pub(crate) fn singular(w: &str) -> Option<String> {
    let b = w.as_bytes();
    let n = b.len();
    if n < 4 || !b.iter().all(u8::is_ascii_lowercase) || b[n - 1] != b's' {
        return None;
    }
    if w.ends_with("ies") {
        // `bodies` → `body`, but not `ties` / `aies` / `eies`.
        return (n > 4 && !matches!(b[n - 4], b'a' | b'e')).then(|| format!("{}y", &w[..n - 3]));
    }
    for sibilant in ["sses", "shes", "ches", "xes", "zzes"] {
        if w.ends_with(sibilant) {
            return Some(w[..n - 2].to_owned());
        }
    }
    match b[n - 2] {
        b'u' | b's' | b'i' => None,
        b'e' if matches!(b[n - 3], b'a' | b'e' | b'o') => None,
        _ => Some(w[..n - 1].to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singular_table() {
        let cases: &[(&str, Option<&str>)] = &[
            ("kicks", Some("kick")),
            ("loops", Some("loop")),
            ("hihats", Some("hihat")),
            ("vocals", Some("vocal")),
            ("drums", Some("drum")),
            ("shots", Some("shot")),
            ("crashes", Some("crash")),
            ("glitches", Some("glitch")),
            ("boxes", Some("box")),
            ("classes", Some("class")),
            ("bodies", Some("body")),
            ("atmos", Some("atmo")),
            ("keys", Some("key")),
            // Kept.
            ("bass", None),
            ("chorus", None),
            ("analysis", None),
            ("shoes", None),
            ("ties", None),
            ("hats2", None),
            ("fxs", None),
            ("kick", None),
            ("cafés", None),
        ];
        for &(w, want) in cases {
            assert_eq!(singular(w).as_deref(), want, "{w}");
        }
    }
}

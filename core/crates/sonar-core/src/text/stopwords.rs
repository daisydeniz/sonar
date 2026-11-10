//! Pack-noise stop list: words that appear in many sample names/paths but
//! never help a search.
//!
//! Defaults are deliberately short. Musically meaningful words stay
//! searchable even when frequent: `loop`, `one`, `shot`, `oneshot`, `kit`,
//! `drum`, `wet`, `dry`, `mono`, `stereo`, vendor and pack names, single
//! letters. BM25's IDF already discounts frequent words; the list only
//! removes words a user would never type to find a sound.

/// Default stop words (folded, sorted for binary search).
pub const DEFAULT_STOP_WORDS: &[&str] = &[
    "aif", "aifc", "aiff", "and", "bit", "bits", "bpm", "by", "com", "copy", "flac", "free",
    "http", "https", "hz", "kbps", "khz", "m4a", "mp3", "net", "of", "ogg", "org", "prod", "rex",
    "rx2", "sample", "samples", "the", "wav", "wave", "www",
];

/// A configurable stop-word set. Words are compared in folded form
/// (lowercase, no diacritics).
#[derive(Debug, Clone)]
pub struct StopWords {
    defaults: bool,
    /// Extra words, folded and sorted.
    extra: Vec<String>,
}

impl Default for StopWords {
    /// The [`DEFAULT_STOP_WORDS`] list.
    fn default() -> Self {
        Self {
            defaults: true,
            extra: Vec::new(),
        }
    }
}

impl StopWords {
    /// No stop words at all.
    pub fn none() -> Self {
        Self {
            defaults: false,
            extra: Vec::new(),
        }
    }

    /// Exactly `words` (folded), without the defaults.
    pub fn from_words<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::none().with_extra(words)
    }

    /// Adds `words` (folded) to this set.
    pub fn with_extra<I, S>(mut self, words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.extra
            .extend(words.into_iter().map(|w| super::lex::fold(w.as_ref())));
        self.extra.sort_unstable();
        self.extra.dedup();
        self
    }

    /// True if some stop word starts with the folded `prefix` (as-you-type:
    /// `bp` may become the dropped `bpm`).
    pub fn any_starts_with(&self, prefix: &str) -> bool {
        let defaults: &[&str] = if self.defaults {
            DEFAULT_STOP_WORDS
        } else {
            &[]
        };
        let i = defaults.partition_point(|w| *w < prefix);
        let j = self.extra.partition_point(|w| w.as_str() < prefix);
        defaults.get(i).is_some_and(|w| w.starts_with(prefix))
            || self.extra.get(j).is_some_and(|w| w.starts_with(prefix))
    }

    /// True if the folded word `w` is a stop word.
    pub fn contains(&self, w: &str) -> bool {
        (self.defaults && DEFAULT_STOP_WORDS.binary_search(&w).is_ok())
            || self.extra.binary_search_by(|e| e.as_str().cmp(w)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sorted_and_folded() {
        assert!(DEFAULT_STOP_WORDS.windows(2).all(|w| w[0] < w[1]));
        assert!(DEFAULT_STOP_WORDS
            .iter()
            .all(|w| super::super::lex::fold(w) == *w));
    }

    #[test]
    fn membership_table() {
        let custom = StopWords::default().with_extra(["Ñoise", "PACK"]);
        let only = StopWords::from_words(["vinyl"]);
        let cases: &[(&StopWords, &str, bool)] = &[
            (&StopWords::default(), "wav", true),
            (&StopWords::default(), "loop", false),
            (&StopWords::default(), "shot", false),
            (&StopWords::default(), "a", false),
            (&custom, "noise", true),
            (&custom, "pack", true),
            (&custom, "www", true),
            (&only, "vinyl", true),
            (&only, "wav", false),
            (&StopWords::none(), "wav", false),
        ];
        for (set, w, want) in cases {
            assert_eq!(set.contains(w), *want, "word {w:?}");
        }
    }
}

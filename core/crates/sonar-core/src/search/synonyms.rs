//! Synonym groups (`docs/synonyms.tsv`), expanded at query time.
//!
//! File format: one group of equivalent terms per line, members separated
//! by TABs or commas, `#` starts a comment line. Each member is analyzed
//! with the index-mode text chain and kept as its sequence of primary
//! tokens (one per position), so `Hi-Hat` and `hi hat` are the same
//! two-word member and match as a phrase.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::text::{Tokenizer, TokenizerConfig};
use crate::{paths, Error, Result};

/// The built-in groups (`docs/synonyms.tsv` at build time).
pub const BUILTIN_SYNONYMS: &str = include_str!("../../../../../docs/synonyms.tsv");

/// File name of the per-machine override in the data directory.
pub const SYNONYMS_FILE: &str = "synonyms.tsv";

/// Groups of equivalent terms with a lookup by first token.
#[derive(Debug, Clone, Default)]
pub struct Synonyms {
    /// group → members → tokens.
    groups: Vec<Vec<Vec<String>>>,
    /// First token of a member → `(group, member)` indexes.
    by_first: HashMap<String, Vec<(usize, usize)>>,
}

impl Synonyms {
    /// No synonyms.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The built-in groups ([`BUILTIN_SYNONYMS`]).
    pub fn builtin() -> Self {
        Self::parse(BUILTIN_SYNONYMS)
    }

    /// Reads a synonyms file.
    ///
    /// # Errors
    /// [`Error::Io`] if it cannot be read (or is not UTF-8).
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        Ok(Self::parse(&text))
    }

    /// `<data dir>/synonyms.tsv` if it exists, else [`Synonyms::builtin`].
    ///
    /// # Errors
    /// [`Error::NoDataDir`], or [`Error::Io`] if the file exists but cannot
    /// be read.
    pub fn load_default() -> Result<Self> {
        Self::load_or_builtin(paths::data_dir()?.join(SYNONYMS_FILE))
    }

    /// `path` if it exists, else [`Synonyms::builtin`].
    ///
    /// # Errors
    /// [`Error::Io`] if the file exists but cannot be read.
    pub fn load_or_builtin(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        match fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::builtin()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Parses the file format (see the module docs). Members that analyze
    /// to nothing (stop words) are skipped; groups left with fewer than two
    /// distinct members are dropped.
    pub fn parse(text: &str) -> Self {
        let mut tokenizer = Tokenizer::new(TokenizerConfig {
            singularize: false,
            ..TokenizerConfig::default()
        });
        let mut out = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut group: Vec<Vec<String>> = Vec::new();
            for raw in line.split(['\t', ',']) {
                let member = primary_tokens(&mut tokenizer, raw);
                if !member.is_empty() && !group.contains(&member) {
                    group.push(member);
                }
            }
            if group.len() >= 2 {
                let g = out.groups.len();
                for (m, member) in group.iter().enumerate() {
                    out.by_first
                        .entry(member[0].clone())
                        .or_default()
                        .push((g, m));
                }
                out.groups.push(group);
            }
        }
        out
    }

    /// Number of groups.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// True if there are no groups.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Members (token sequences) of group `g`.
    pub(crate) fn members(&self, g: usize) -> &[Vec<String>] {
        self.groups.get(g).map_or(&[], Vec::as_slice)
    }

    /// Longest member of at least `min_len` tokens matching the query
    /// positions `slots[at..]` (each member token must be one of the
    /// alternatives at its position): `(length, groups)`, all groups that
    /// have a member of that length matching there.
    pub(crate) fn longest_match(
        &self,
        slots: &[Vec<String>],
        at: usize,
        min_len: usize,
    ) -> Option<(usize, Vec<usize>)> {
        let mut best: Option<(usize, Vec<usize>)> = None;
        for alt in slots.get(at)? {
            for &(g, m) in self.by_first.get(alt).into_iter().flatten() {
                let member = &self.groups[g][m];
                let k = member.len();
                let fits = k >= min_len
                    && at + k <= slots.len()
                    && member
                        .iter()
                        .zip(&slots[at..])
                        .all(|(tok, alts)| alts.contains(tok));
                if !fits {
                    continue;
                }
                match &mut best {
                    Some((len, groups)) if *len == k => {
                        if !groups.contains(&g) {
                            groups.push(g);
                        }
                    }
                    Some((len, _)) if *len > k => {}
                    _ => best = Some((k, vec![g])),
                }
            }
        }
        best
    }
}

/// First token of each position of `raw` (index-mode analysis).
fn primary_tokens(tokenizer: &mut Tokenizer, raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut last = None;
    for t in tokenizer.tokenize(raw.trim()) {
        if last != Some(t.position) {
            last = Some(t.position);
            out.push(t.text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots(words: &[&[&str]]) -> Vec<Vec<String>> {
        words
            .iter()
            .map(|alts| alts.iter().map(|s| (*s).to_owned()).collect())
            .collect()
    }

    #[test]
    fn parses_groups_members_and_comments() {
        let s = Synonyms::parse(
            "# comment\n\nkick\tBD\tbass drum\nHat, hi-hat, Hi Hat, hh\nlonely\nthe\tof\n",
        );
        assert_eq!(s.len(), 2, "single-member and stop-word groups dropped");
        assert_eq!(
            s.members(0),
            &[
                vec!["kick".to_owned()],
                vec!["bd".into()],
                vec!["bass".into(), "drum".into()]
            ]
        );
        // `hi-hat` and `Hi Hat` are one member.
        assert_eq!(s.members(1).len(), 3);
        assert!(s.members(9).is_empty());
    }

    #[test]
    fn longest_match_prefers_multi_word_members() {
        let s = Synonyms::parse("kick\tbd\tbass drum\nbass\tlow end\n");
        let q = slots(&[&["bass"], &["drum"], &["loop"]]);
        assert_eq!(s.longest_match(&q, 0, 1), Some((2, vec![0])));
        assert_eq!(s.longest_match(&q, 0, 3), None);
        let q = slots(&[&["bass"]]);
        assert_eq!(s.longest_match(&q, 0, 1), Some((1, vec![1])));
        assert_eq!(s.longest_match(&q, 0, 2), None);
        let q = slots(&[&["kicks", "kick"]]);
        assert_eq!(s.longest_match(&q, 0, 1), Some((1, vec![0])));
        assert_eq!(s.longest_match(&q, 1, 1), None);
    }

    #[test]
    fn builtin_file_is_sane() {
        let s = Synonyms::builtin();
        assert!(s.len() >= 20, "{}", s.len());
        let q = slots(&[&["bd"]]);
        let (_, groups) = s.longest_match(&q, 0, 1).expect("bd is a kick");
        assert!(s.members(groups[0]).contains(&vec!["kick".to_owned()]));
        let q = slots(&[&["one"], &["shot"]]);
        let (k, groups) = s.longest_match(&q, 0, 1).expect("one shot");
        assert_eq!(k, 2);
        assert!(s.members(groups[0]).contains(&vec!["oneshot".to_owned()]));
    }

    #[test]
    fn load_reads_a_file_and_reports_missing_ones() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("syn.tsv");
        fs::write(&path, "boom\tbap\n").expect("write");
        assert_eq!(Synonyms::load(&path).expect("load").len(), 1);
        let missing = Synonyms::load(tmp.path().join("nope.tsv"));
        assert!(matches!(missing, Err(Error::Io { .. })));
    }

    #[test]
    fn override_file_replaces_builtin_and_absent_file_falls_back() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join(SYNONYMS_FILE);
        let builtin = Synonyms::builtin().len();
        assert_eq!(
            Synonyms::load_or_builtin(&path).expect("absent").len(),
            builtin
        );
        fs::write(&path, "boom\tbap\n").expect("write");
        assert_eq!(Synonyms::load_or_builtin(&path).expect("override").len(), 1);
        // A directory in its place is an error, not a silent fallback.
        let dir = tmp.path().join("dir");
        fs::create_dir(&dir).expect("mkdir");
        assert!(matches!(
            Synonyms::load_or_builtin(&dir),
            Err(Error::Io { .. })
        ));
    }
}

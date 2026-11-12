//! Category hint from a file's name and parent folders, read with the
//! ADR-016 tokenizer (`sonar_core::text`) so names are split and folded
//! exactly as the search index does (`HiHat_01` → `hi hat hihat`,
//! `Kicks` → `kicks|kick`).
//!
//! Per name segment (file stem first, then up to [`HINT_DIR_LEVELS`]
//! parent folders, nearest first; the first segment with a hint wins):
//! 1. genre phrases are skipped (`Future Bass`, `Drum & Bass`, `Bass
//!    House`: not a bass sample);
//! 2. multi-word taxonomy words match first (`bass drum` → kick);
//! 3. the remaining tokens are looked up in the taxonomy's words, then (only
//!    if no word matched) its weak words (`Snare Fills` → snare, `Fills` →
//!    drums);
//! 4. exactly one category → hint; several (`Synth & Plucks`) → no hint
//!    from this segment.
//!
//! Only the file's own path is read; the eval hides it from the analyzer.

use std::path::{Component, Path};

use crate::text::{strip_audio_extension, Token, TokenKind, Tokenizer};

use super::taxonomy::{Category, TAXONOMY};

/// Parent folders consulted when the file name has no hint (same depth as
/// the BPM/key priors, [`crate::text::PRIOR_DIR_LEVELS`]).
pub const HINT_DIR_LEVELS: usize = crate::text::PRIOR_DIR_LEVELS;

/// Two-word genre names whose words must not count as categories.
pub const GENRE_PHRASES: &[[&str; 2]] = &[
    ["future", "bass"],
    ["drum", "bass"],
    ["n", "bass"],
    ["bass", "house"],
    ["bass", "music"],
    ["uk", "bass"],
];

/// Category hinted by `path`'s file name or nearest parent folders, if any.
pub fn category_hint(path: &Path) -> Option<Category> {
    let mut tokenizer = Tokenizer::default();
    let mut tokens = Vec::new();
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy();
        let stem = strip_audio_extension(&name);
        if let Some(c) = segment_category(stem, &mut tokenizer, &mut tokens) {
            return Some(c);
        }
    }
    let dirs: Vec<Component<'_>> = path
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .collect();
    dirs.iter()
        .rev()
        .take(HINT_DIR_LEVELS)
        .filter_map(|c| match c {
            Component::Normal(d) => Some(d.to_string_lossy()),
            _ => None,
        })
        .find_map(|d| segment_category(&d, &mut tokenizer, &mut tokens))
}

/// Category of one name segment (no path separators), if exactly one is
/// named.
pub fn segment_category(
    text: &str,
    tokenizer: &mut Tokenizer,
    tokens: &mut Vec<Token>,
) -> Option<Category> {
    tokens.clear();
    tokenizer.tokenize_into(text, tokens);
    // Words per position (a plural and its singular share one).
    let mut slots: Vec<Vec<&str>> = Vec::new();
    for t in tokens.iter() {
        if !matches!(t.kind, TokenKind::Word | TokenKind::Number) {
            continue;
        }
        let pos = t.position as usize;
        if slots.len() <= pos {
            slots.resize_with(pos + 1, Vec::new);
        }
        slots[pos].push(t.text.as_str());
    }
    let has = |pos: usize, w: &str| slots.get(pos).is_some_and(|s| s.contains(&w));
    let mut used = vec![false; slots.len()];
    let mut strong = [false; super::taxonomy::N_CATEGORIES];
    let mut weak = [false; super::taxonomy::N_CATEGORIES];
    // Genre phrases, then multi-word taxonomy words.
    for pos in 0..slots.len().saturating_sub(1) {
        if GENRE_PHRASES
            .iter()
            .any(|[a, b]| has(pos, a) && has(pos + 1, b))
        {
            used[pos] = true;
            used[pos + 1] = true;
        }
    }
    for info in &TAXONOMY {
        for w in info.words.iter().filter(|w| w.contains(' ')) {
            let parts: Vec<&str> = w.split(' ').collect();
            for pos in 0..slots.len() {
                let fits = (0..parts.len()).all(|k| !used.get(pos + k).copied().unwrap_or(true));
                if fits && parts.iter().enumerate().all(|(k, p)| has(pos + k, p)) {
                    strong[info.category.index()] = true;
                    used[pos..pos + parts.len()].fill(true);
                }
            }
        }
    }
    for (pos, words) in slots.iter().enumerate() {
        if used[pos] {
            continue;
        }
        for info in &TAXONOMY {
            let i = info.category.index();
            if info.words.iter().any(|w| words.contains(w)) {
                strong[i] = true;
            } else if info.weak_words.iter().any(|w| words.contains(w)) {
                weak[i] = true;
            }
        }
    }
    let pick = |flags: &[bool]| -> Option<Option<Category>> {
        let mut found = flags
            .iter()
            .enumerate()
            .filter(|(_, &f)| f)
            .map(|(i, _)| Category::ALL[i]);
        let first = found.next()?;
        Some(found.next().is_none().then_some(first))
    };
    match pick(&strong) {
        Some(decided) => decided,
        None => pick(&weak).flatten(),
    }
}

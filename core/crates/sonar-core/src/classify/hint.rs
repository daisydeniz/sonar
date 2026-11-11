//! Loop/one-shot hint from a file's name and parent folders, read with the
//! ADR-016 tokenizer (`sonar_core::text`) so names are split and folded
//! exactly like the search index does (`Kick_OneShot` → `one|oneshot shot`,
//! `Loops` → `loops|loop`).
//!
//! - **Loop**: a loop word ([`is_loop_word`]: `loop`, `groove`, `beat`,
//!   `break`, `riff`, `phrase`, `progression` and plurals) or a tempo token
//!   (`120bpm`, or a bare number the tokenizer accepted as a tempo).
//! - **OneShot**: `oneshot`, `hit`, or `shot` right after `one`/`1`
//!   (`One Shot`, `one-shot`, `1shot`).
//! - Both in the same name → no hint. The file stem is read first, then up
//!   to [`HINT_DIR_LEVELS`] parent folders (nearest first); the first
//!   segment with a hint wins.

use std::path::{Component, Path};

use crate::text::music::is_loop_word;
use crate::text::{strip_audio_extension, Token, TokenKind, Tokenizer};

use super::kind::Kind;

/// Parent folders consulted when the file name has no hint (same depth as
/// the BPM/key priors, [`crate::text::PRIOR_DIR_LEVELS`]).
pub const HINT_DIR_LEVELS: usize = crate::text::PRIOR_DIR_LEVELS;

/// Kind hinted by `path`'s file name or nearest parent folders, if any.
pub fn kind_hint(path: &Path) -> Option<Kind> {
    let mut tokenizer = Tokenizer::default();
    let mut tokens = Vec::new();
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy();
        if let Some(k) = segment_hint(strip_audio_extension(&name), &mut tokenizer, &mut tokens) {
            return Some(k);
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
        .find_map(|d| segment_hint(&d, &mut tokenizer, &mut tokens))
}

/// Hint of one name segment (no path separators).
pub fn segment_hint(
    text: &str,
    tokenizer: &mut Tokenizer,
    tokens: &mut Vec<Token>,
) -> Option<Kind> {
    tokens.clear();
    tokenizer.tokenize_into(text, tokens);
    let mut is_loop = false;
    let mut is_one_shot = false;
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            TokenKind::Bpm => is_loop = true,
            TokenKind::Word | TokenKind::Number => {
                let w = t.text.as_str();
                if is_loop_word(w) {
                    is_loop = true;
                } else if w == "oneshot" || w == "hit" || (w == "shot" && follows_one(tokens, i)) {
                    is_one_shot = true;
                }
            }
            TokenKind::Key | TokenKind::Note => {}
        }
    }
    match (is_loop, is_one_shot) {
        (true, false) => Some(Kind::Loop),
        (false, true) => Some(Kind::OneShot),
        _ => None,
    }
}

/// `tokens[i]` is directly preceded (previous position) by `one` or `1`.
fn follows_one(tokens: &[Token], i: usize) -> bool {
    let pos = tokens[i].position;
    tokens[..i]
        .iter()
        .any(|t| t.position + 1 == pos && (t.text == "one" || t.text == "1"))
}

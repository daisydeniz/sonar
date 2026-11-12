//! The CLAP text tower (feature `embed`, ADR-026): RoBERTa byte-level BPE
//! from `tokenizer.json`, fp32 text graph. The graph has no attention mask,
//! so one query runs at a time and is never padded. Content is truncated to
//! [`MAX_TEXT_TOKENS`] tokens plus `<s>` / `</s>` (the 77-token LAION
//! training length). 4.8 ms per query on an M4 Pro.

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use super::session::{build, ort_err};
use super::{normalize, EmbedEp, EmbedOptions, Embedding, TEXT_MODEL, TOKENIZER};
use crate::{Error, Result};

/// Most content tokens of a query (77 minus `<s>` and `</s>`).
pub const MAX_TEXT_TOKENS: usize = 75;
/// Intra-op threads of the text session (one query at a time).
const TEXT_THREADS: usize = 4;
const BOS_FALLBACK: u32 = 0;
const EOS_FALLBACK: u32 = 2;

/// Tokenizer plus text session. Not shared between threads (`embed` takes
/// `&mut self`); create one per user of it.
pub struct TextEncoder {
    tokenizer: Tokenizer,
    session: Session,
    bos: u32,
    eos: u32,
}

impl std::fmt::Debug for TextEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextEncoder").finish_non_exhaustive()
    }
}

/// Loads `tokenizer.json`.
fn load_tokenizer(dir: &Path) -> Result<Tokenizer> {
    let path = dir.join(TOKENIZER);
    if !path.is_file() {
        return Err(Error::ModelMissing { path });
    }
    Tokenizer::from_file(&path).map_err(|e| Error::Embed {
        reason: format!("cannot read {}: {e}", path.display()),
    })
}

impl TextEncoder {
    /// Loads the tokenizer and the text model of `opts`.
    ///
    /// # Errors
    /// [`Error::ModelMissing`] / [`Error::Embed`] if a file is missing,
    /// corrupt or does not load.
    pub fn new(opts: &EmbedOptions) -> Result<Self> {
        let dir = opts.model_dir();
        let tokenizer = load_tokenizer(&dir)?;
        // The 500 MB text graph on CPU is plenty fast (4.8 ms); CoreML would
        // only add a compile step.
        let session = build(&dir.join(TEXT_MODEL), EmbedEp::Cpu, TEXT_THREADS)?;
        let id = |t: &str, fallback| tokenizer.token_to_id(t).unwrap_or(fallback);
        Ok(Self {
            bos: id("<s>", BOS_FALLBACK),
            eos: id("</s>", EOS_FALLBACK),
            tokenizer,
            session,
        })
    }

    /// Token ids of `text`: `<s>`, at most [`MAX_TEXT_TOKENS`] content
    /// tokens, `</s>`.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] for empty text; [`Error::Embed`] if the
    /// tokenizer fails.
    pub fn token_ids(&self, text: &str) -> Result<Vec<i64>> {
        token_ids(&self.tokenizer, text, self.bos, self.eos)
    }

    /// L2-normalised embedding of `text`.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] for empty text; [`Error::Embed`] if the
    /// model fails.
    pub fn embed(&mut self, text: &str) -> Result<Embedding> {
        let ids = self.token_ids(text)?;
        let len = ids.len();
        let input = Tensor::from_array(([1usize, len], ids.into_boxed_slice()))
            .map_err(|e| ort_err("building the text input", e))?;
        let outputs = self
            .session
            .run(ort::inputs!["input_ids" => input])
            .map_err(|e| ort_err("running the text model", e))?;
        let (_, raw) = outputs
            .get("text_embeds")
            .ok_or_else(|| Error::Embed {
                reason: "text model has no `text_embeds` output".into(),
            })?
            .try_extract_tensor::<f32>()
            .map_err(|e| ort_err("reading the text embedding", e))?;
        normalize(raw)
    }
}

/// See [`TextEncoder::token_ids`].
pub(super) fn token_ids(tok: &Tokenizer, text: &str, bos: u32, eos: u32) -> Result<Vec<i64>> {
    if text.trim().is_empty() {
        return Err(Error::InvalidQuery {
            reason: "empty text query".into(),
        });
    }
    let enc = tok.encode(text, false).map_err(|e| Error::Embed {
        reason: format!("tokenizer failed: {e}"),
    })?;
    let mut ids = Vec::with_capacity(MAX_TEXT_TOKENS + 2);
    ids.push(i64::from(bos));
    ids.extend(
        enc.get_ids()
            .iter()
            .take(MAX_TEXT_TOKENS)
            .map(|&i| i64::from(i)),
    );
    ids.push(i64::from(eos));
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Option<std::path::PathBuf> {
        let d = EmbedOptions::default().model_dir();
        d.join(TOKENIZER).is_file().then_some(d)
    }

    #[test]
    fn kick_drum_tokenizes_like_the_reference() {
        let Some(dir) = dir() else {
            eprintln!("skipped: CLAP model files not found (scripts/fetch-models.sh)");
            return;
        };
        let tok = load_tokenizer(&dir).expect("tokenizer");
        let ids = token_ids(&tok, "a kick drum", 0, 2).expect("ids");
        assert_eq!(ids, [0, 102, 3151, 12638, 2]);
        // Same ids as the tokenizer's own special-token handling.
        let with = tok.encode("a kick drum", true).expect("encode");
        assert_eq!(
            with.get_ids()
                .iter()
                .map(|&i| i64::from(i))
                .collect::<Vec<_>>(),
            ids
        );
    }

    #[test]
    fn long_queries_are_truncated_and_empty_ones_rejected() {
        let Some(dir) = dir() else {
            eprintln!("skipped: CLAP model files not found (scripts/fetch-models.sh)");
            return;
        };
        let tok = load_tokenizer(&dir).expect("tokenizer");
        let long = "kick ".repeat(300);
        let ids = token_ids(&tok, &long, 0, 2).expect("ids");
        assert_eq!(ids.len(), MAX_TEXT_TOKENS + 2);
        assert_eq!((ids[0], ids[ids.len() - 1]), (0, 2));
        assert!(matches!(
            token_ids(&tok, "  ", 0, 2),
            Err(Error::InvalidQuery { .. })
        ));
    }

    #[test]
    fn missing_or_corrupt_tokenizer_is_a_typed_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert!(matches!(
            load_tokenizer(tmp.path()),
            Err(Error::ModelMissing { .. })
        ));
        std::fs::write(tmp.path().join(TOKENIZER), b"{ not json").expect("write");
        assert!(matches!(
            load_tokenizer(tmp.path()),
            Err(Error::Embed { .. })
        ));
    }
}

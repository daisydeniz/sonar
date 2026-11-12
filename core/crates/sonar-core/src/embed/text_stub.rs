//! Stand-in for the text tower in builds without the `embed` feature.

use super::{EmbedOptions, Embedding};
use crate::{Error, Result};

/// Most content tokens of a query (77 minus `<s>` and `</s>`).
pub const MAX_TEXT_TOKENS: usize = 75;

fn unavailable() -> Error {
    Error::Embed {
        reason: "this build has no CLAP support (feature `embed`)".into(),
    }
}

/// Unavailable in this build.
#[derive(Debug)]
pub struct TextEncoder {
    _private: (),
}

impl TextEncoder {
    /// Always [`Error::Embed`] in this build.
    ///
    /// # Errors
    /// Always.
    pub fn new(_opts: &EmbedOptions) -> Result<Self> {
        Err(unavailable())
    }

    /// Always [`Error::Embed`] in this build.
    ///
    /// # Errors
    /// Always.
    pub fn embed(&mut self, _text: &str) -> Result<Embedding> {
        Err(unavailable())
    }
}

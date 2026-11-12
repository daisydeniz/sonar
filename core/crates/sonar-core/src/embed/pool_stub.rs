//! Stand-in for the audio session pool in builds without the `embed`
//! feature: it cannot be constructed, so the job stage is skipped.

use super::{EmbedOptions, Embedding};
use crate::{Error, Result};

fn unavailable() -> Error {
    Error::Embed {
        reason: "this build has no CLAP support (feature `embed`)".into(),
    }
}

/// Unavailable in this build.
#[derive(Debug)]
pub struct EmbedPool {
    _private: (),
}

impl EmbedPool {
    /// Always [`Error::Embed`] in this build.
    ///
    /// # Errors
    /// Always.
    pub fn new(_opts: &EmbedOptions) -> Result<Self> {
        Err(unavailable())
    }

    /// Number of sessions (none).
    pub fn sessions(&self) -> usize {
        0
    }

    /// Always [`Error::Embed`] in this build.
    ///
    /// # Errors
    /// Always.
    pub fn embed(&self, _features: Vec<f32>) -> Result<Embedding> {
        Err(unavailable())
    }
}

//! ONNX Runtime session construction (feature `embed`).

use std::path::Path;

use ort::execution_providers::CoreMLExecutionProvider;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;

use super::EmbedEp;
use crate::{Error, Result};

/// Maps an `ort` error to [`Error::Embed`] with some context.
pub(super) fn ort_err(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Embed {
        reason: format!("{what}: {e}"),
    }
}

/// Loads `path` with graph optimisation level 3 and `intra` intra-op
/// threads. With [`EmbedEp::CoreMl`] the CoreML provider is tried first
/// (dynamic shapes; registration failures are not fatal) and, if the model
/// does not load with it, the CPU provider is used.
///
/// # Errors
/// [`Error::ModelMissing`] if the file does not exist; [`Error::Embed`] if
/// it cannot be loaded (corrupt file, runtime error).
pub(super) fn build(path: &Path, ep: EmbedEp, intra: usize) -> Result<Session> {
    if !path.is_file() {
        return Err(Error::ModelMissing {
            path: path.to_path_buf(),
        });
    }
    if ep == EmbedEp::CoreMl {
        match load(path, intra, true) {
            Ok(s) => return Ok(s),
            Err(e) => tracing::warn!(error = %e, "CoreML provider unavailable, using the CPU"),
        }
    }
    load(path, intra, false)
}

fn load(path: &Path, intra: usize, coreml: bool) -> Result<Session> {
    let what = || format!("loading {}", path.display());
    let mut b = Session::builder()
        .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
        .and_then(|b| b.with_intra_threads(intra))
        .map_err(|e| ort_err(&what(), e))?;
    if coreml {
        b = b
            .with_execution_providers([CoreMLExecutionProvider::default()
                .with_subgraphs(true)
                .build()
                .error_on_failure()])
            .map_err(|e| ort_err(&what(), e))?;
    }
    b.commit_from_file(path).map_err(|e| ort_err(&what(), e))
}

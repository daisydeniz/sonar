//! CLAP embeddings (T5.2, ADR-026, ADR-027): audio clips and text queries
//! in one 512-d space (Wu et al. 2023, arXiv:2211.06687; LAION
//! `clap-htsat-unfused`), for semantic search and zero-shot categories.
//!
//! - [`front`] / [`mel`]: the 10 s, 48 kHz log-mel front end (always built).
//! - [`prep`]: file -> features (decode from the native rate to 48 kHz, 10 s).
//! - [`store`]: the append-only embedding store next to the aspect vectors
//!   (`index/clap`), with the `(file id, &[f32; 512])` read API (always built).
//! - `pool` (feature `embed`): N single-thread int8 audio sessions of ONNX
//!   Runtime on their own threads, batched input `[B, 1, 1001, 64]`, output
//!   L2-normalised. CPU execution provider by default (ADR-026: faster than
//!   CoreML here); CoreML is selectable with [`EmbedEp::CoreMl`] or
//!   `SONAR_EMBED_EP=coreml` and falls back to CPU if it cannot load.
//! - `text` (feature `embed`): RoBERTa BPE tokenizer + fp32 text tower.
//!
//! Without the `embed` feature (the app's FFI build until T5.6) the model
//! side is a stub whose constructors return [`crate::Error::Embed`], so the
//! `Embed` job stage is skipped, never failed.
//!
//! Model files live in `<models dir>/clap-htsat-unfused/` (see
//! [`models_dir`]); `scripts/fetch-models.sh` downloads them.

pub mod front;
mod mel;
pub mod prep;
pub mod store;

#[cfg(all(test, feature = "embed"))]
mod model_tests;
#[cfg(feature = "embed")]
mod pool;
#[cfg(not(feature = "embed"))]
#[path = "pool_stub.rs"]
mod pool;
#[cfg(feature = "embed")]
mod session;
#[cfg(feature = "embed")]
mod text;
#[cfg(not(feature = "embed"))]
#[path = "text_stub.rs"]
mod text;

use std::path::{Path, PathBuf};
use std::str::FromStr;

pub use front::{FrontEnd, FEATURE_LEN, FRAMES, N_MELS, SAMPLE_RATE};
pub use pool::EmbedPool;
pub use prep::AudioPrep;
pub use store::{EmbedStats, EmbedStore};
pub use text::{TextEncoder, MAX_TEXT_TOKENS};

use crate::{paths, Error, Result};

/// Embedding dimension of both towers.
pub const EMBED_DIM: usize = 512;
/// One L2-normalised embedding.
pub type Embedding = [f32; EMBED_DIM];

/// Environment variable overriding the models directory.
pub const MODELS_DIR_ENV: &str = "SONAR_MODELS_DIR";
/// Environment variable selecting the execution provider (`cpu`, `coreml`).
pub const EP_ENV: &str = "SONAR_EMBED_EP";
/// Model directory name inside the models directory.
pub const MODEL_DIR_NAME: &str = "clap-htsat-unfused";
/// int8 audio tower (ADR-026).
pub const AUDIO_MODEL: &str = "audio_model_quantized.onnx";
/// fp32 text tower (ADR-026).
pub const TEXT_MODEL: &str = "text_model.onnx";
/// RoBERTa BPE tokenizer.
pub const TOKENIZER: &str = "tokenizer.json";

/// ONNX Runtime execution provider of the audio sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbedEp {
    /// CPU (default): N single-thread sessions, 122-158 clips/s on an M4 Pro.
    #[default]
    Cpu,
    /// CoreML (GPU/ANE) with dynamic shapes; slower than CPU in ADR-026's
    /// measurements. Falls back to CPU if it cannot be registered or loaded.
    CoreMl,
}

impl EmbedEp {
    /// `SONAR_EMBED_EP`, defaulting to [`EmbedEp::Cpu`] when unset or
    /// unrecognised.
    pub fn from_env() -> Self {
        std::env::var(EP_ENV)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_default()
    }

    /// Name for logs and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::CoreMl => "coreml",
        }
    }
}

impl FromStr for EmbedEp {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cpu" => Ok(Self::Cpu),
            "coreml" => Ok(Self::CoreMl),
            other => Err(format!(
                "unknown execution provider '{other}' (cpu or coreml)"
            )),
        }
    }
}

/// Options of the embedding pipeline.
#[derive(Debug, Clone)]
pub struct EmbedOptions {
    /// Models directory (containing [`MODEL_DIR_NAME`]); `None` =
    /// [`models_dir`].
    pub models_dir: Option<PathBuf>,
    /// Audio sessions; 0 = [`default_sessions`] (1 for CoreML).
    pub sessions: usize,
    /// Most clips one session runs together (a session takes what is
    /// queued, up to this). 0 = [`DEFAULT_MAX_BATCH`].
    pub max_batch: usize,
    /// Execution provider.
    pub ep: EmbedEp,
}

impl Default for EmbedOptions {
    fn default() -> Self {
        Self {
            models_dir: None,
            sessions: 0,
            max_batch: 0,
            ep: EmbedEp::from_env(),
        }
    }
}

/// Default clips per session run (ADR-027: batching did not beat batch 1 on
/// the CPU EP).
pub const DEFAULT_MAX_BATCH: usize = 1;

impl EmbedOptions {
    /// The directory holding the model files.
    pub fn model_dir(&self) -> PathBuf {
        self.models_dir
            .clone()
            .unwrap_or_else(models_dir)
            .join(MODEL_DIR_NAME)
    }

    /// Resolved session count.
    pub fn resolved_sessions(&self) -> usize {
        match (self.sessions, self.ep) {
            (0, EmbedEp::CoreMl) => 1,
            (0, EmbedEp::Cpu) => default_sessions(),
            (n, _) => n.min(64),
        }
    }

    /// Resolved clips per session run.
    pub fn resolved_batch(&self) -> usize {
        if self.max_batch == 0 {
            DEFAULT_MAX_BATCH
        } else {
            self.max_batch.min(64)
        }
    }

    /// Why the audio embedding cannot run (`None` = it can): a build without
    /// the `embed` feature, or a missing audio model file. Cheap; does not
    /// load the model.
    pub fn unavailable_reason(&self) -> Option<String> {
        if !cfg!(feature = "embed") {
            return Some("this build has no CLAP support (feature `embed`)".into());
        }
        let path = self.model_dir().join(AUDIO_MODEL);
        (!path.is_file()).then(|| {
            format!(
                "CLAP model not found at {} (run scripts/fetch-models.sh or set {MODELS_DIR_ENV})",
                path.display()
            )
        })
    }
}

/// Where the models are: `$SONAR_MODELS_DIR`; else `<repo>/models` when this
/// is a development build whose checkout has the model files; else
/// `<data dir>/models`.
pub fn models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(MODELS_DIR_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../models");
    if dev.join(MODEL_DIR_NAME).join(AUDIO_MODEL).is_file() {
        return dev;
    }
    paths::data_dir().map_or_else(|_| PathBuf::from("models"), |d| d.join("models"))
}

/// Default audio sessions: the performance cores (N single-thread sessions
/// beat one multi-thread session, ADR-026; efficiency cores add nothing,
/// ADR-027), at least 1. Probed once per process (cached).
pub fn default_sessions() -> usize {
    static CACHED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CACHED.get_or_init(probe_sessions)
}

fn probe_sessions() -> usize {
    let p_cores = std::process::Command::new("sysctl")
        .args(["-n", "hw.perflevel0.logicalcpu"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<usize>().ok());
    p_cores
        .or_else(|| std::thread::available_parallelism().ok().map(usize::from))
        .unwrap_or(1)
        .clamp(1, 16)
}

/// L2-normalises raw tower output in place into an [`Embedding`].
///
/// # Errors
/// [`Error::Embed`] if the vector has the wrong length, a non-finite value
/// or (almost) zero length.
#[cfg_attr(not(feature = "embed"), allow(dead_code))]
pub(crate) fn normalize(raw: &[f32]) -> Result<Embedding> {
    let mut out = [0.0f32; EMBED_DIM];
    if raw.len() != EMBED_DIM {
        return Err(Error::Embed {
            reason: format!("model returned {} values, expected {EMBED_DIM}", raw.len()),
        });
    }
    let norm = raw.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || norm < 1e-12 {
        return Err(Error::Embed {
            reason: "model returned a degenerate embedding".into(),
        });
    }
    for (o, v) in out.iter_mut().zip(raw) {
        *o = v / norm;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_gives_unit_length_and_rejects_bad_output() {
        let raw: Vec<f32> = (0..EMBED_DIM).map(|i| (i % 7) as f32 - 3.0).collect();
        let e = normalize(&raw).expect("normalize");
        let n: f32 = e.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-5);
        assert!(normalize(&raw[..10]).is_err());
        assert!(normalize(&[0.0; EMBED_DIM]).is_err());
        let mut nan = [1.0f32; EMBED_DIM];
        nan[3] = f32::NAN;
        assert!(normalize(&nan).is_err());
    }

    #[test]
    fn execution_provider_names_parse() {
        assert_eq!("cpu".parse(), Ok(EmbedEp::Cpu));
        assert_eq!(" CoreML ".parse(), Ok(EmbedEp::CoreMl));
        assert!("gpu".parse::<EmbedEp>().is_err());
        assert_eq!(EmbedEp::CoreMl.as_str(), "coreml");
    }

    #[test]
    fn missing_models_are_reported_not_panicked() {
        let tmp = tempfile::tempdir().expect("tmp");
        let opts = EmbedOptions {
            models_dir: Some(tmp.path().to_path_buf()),
            ..EmbedOptions::default()
        };
        let reason = opts.unavailable_reason().expect("unavailable");
        assert!(!reason.is_empty());
        assert_eq!(opts.resolved_batch(), DEFAULT_MAX_BATCH);
        assert!(opts.resolved_sessions() >= 1);
    }
}

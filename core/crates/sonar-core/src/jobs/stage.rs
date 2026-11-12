//! Pipeline stages, their versions and per-file results.

use std::fmt;
use std::str::FromStr;

use serde::Serialize;

use crate::dsp::aspects::AspectVectors;
use crate::embed::Embedding;
use crate::meta::BasicMeta;
use crate::store::analysis::KindTempo;
use crate::store::key_pitch::KeyPitch;

/// Current version of the `meta` stage (format, duration). Bump it to
/// re-run the stage on every file.
///
/// History: 2 (T1.7d) exact length of truncated PCM WAV/AIFF (counted by
/// packet bytes), AIFF `COMM` length, truncated AIFF decodable.
pub const META_VERSION: u32 = 2;
/// Current version of the `dsp` stage (decode window, peak/loudness,
/// loop/one-shot kind, tempo, key and note). Bump it to re-run the stage,
/// also when [`crate::dsp::DSP_VERSION`], [`crate::classify::KIND_VERSION`],
/// [`crate::tempo::TEMPO_VERSION`] or [`crate::key::KEY_VERSION`] change.
///
/// History: 2 (T1.7d) same length fixes as `META_VERSION` 2 (the stage
/// stores `num_frames`; truncated AIFF no longer an error; an AIFF `SSND`
/// past `COMM` is no longer metered). 3 (T3.4) stores `analysis.kind` /
/// `kind_conf` and, for loops, `bpm` / `bpm_conf` / `bpm_source`. 4 (T3.5)
/// stores `key` / `key_conf` / `key_source` and, for pitched one-shots,
/// `pitch_midi` / `pitch_conf`. 5 (T4.1) appends the raw aspect vectors to
/// the vector store (ADR-023); bump it also when
/// [`crate::dsp::aspects::ASPECTS_VERSION`] changes.
pub const DSP_VERSION: u32 = 5;
/// Version of the `embed` stage (T5.2, ADR-027): CLAP audio embedding of
/// the first 10 s at the native rate resampled to 48 kHz, int8 audio tower,
/// stored L2-normalised in the embedding store (`index/clap`). Bump it to
/// re-embed every file, e.g. when the model or the front end
/// ([`crate::embed::front`]) changes.
pub const EMBED_VERSION: u32 = 1;

/// A pipeline stage, in priority order (`Meta < Dsp < Embed`).
///
/// Stages run strictly one after another across the whole store: all
/// pending `Meta` work, then `Dsp`, then `Embed` (ADR-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Fast probe: container/codec, sample rate, channels, bit depth and
    /// exact duration (from the header for intact PCM WAV/AIFF, else
    /// counted; ADR-015). No peak/loudness (those are measured in `Dsp`).
    Meta,
    /// Analysis decode (30 s window) plus peak and loudness, loop/one-shot
    /// kind and (loops only) tempo.
    Dsp,
    /// CLAP audio embedding (T5.2, ADR-027), always last. Skipped (not
    /// failed) when the model files are missing or the build has no CLAP
    /// support. A failing file gets an `error` stage row but keeps its
    /// `files.status`.
    Embed,
}

impl Stage {
    /// Every stage in priority order.
    pub const ALL: [Stage; 3] = [Stage::Meta, Stage::Dsp, Stage::Embed];

    /// Name as stored in `file_stages.stage`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meta => "meta",
            Self::Dsp => "dsp",
            Self::Embed => "embed",
        }
    }

    /// Current algorithm version; a file is pending for the stage when its
    /// `file_stages` row is missing or older than this.
    pub fn version(self) -> u32 {
        match self {
            Self::Meta => META_VERSION,
            Self::Dsp => DSP_VERSION,
            Self::Embed => EMBED_VERSION,
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Stage {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|st| st.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown stage '{s}' (expected meta, dsp or embed)"))
    }
}

/// How a stage ended for one file (`file_stages.outcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Completed.
    Ok,
    /// Completed on the readable part of a damaged or truncated file.
    Partial,
    /// Failed; not retried until the content changes or the version bumps.
    Error,
}

impl Outcome {
    /// Name as stored in `file_stages.outcome`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Partial => "partial",
            Self::Error => "error",
        }
    }
}

/// What the `dsp` stage writes: `analysis` columns (ADR-021, ADR-022) and
/// a vector store row (ADR-023).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DspAnalysis {
    /// `kind`, `kind_conf`, `bpm`, `bpm_conf`, `bpm_source`.
    pub kind_tempo: KindTempo,
    /// `key`, `key_conf`, `key_source`, `pitch_midi`, `pitch_conf`.
    pub key_pitch: KeyPitch,
    /// Raw aspect vectors, appended to the vector store.
    pub aspects: AspectVectors,
}

/// What a worker produced for one claimed file.
#[derive(Debug)]
pub(crate) enum FileResult {
    /// Stage completed (`Ok` or `Partial`).
    Done {
        /// Measured metadata (`meta.partial` / `meta.problem` say whether
        /// and why the result is partial).
        meta: BasicMeta,
        /// Kind, tempo, key, note and aspect vectors (`Dsp` stage only;
        /// boxed, the vectors make it large).
        analysis: Option<Box<DspAnalysis>>,
    },
    /// `Embed` stage completed: the file's CLAP embedding.
    Embedded {
        /// The file is damaged or truncated; the embedding covers what was
        /// readable (recorded as `partial`).
        partial: bool,
        /// First problem seen when `partial`.
        problem: Option<String>,
        /// L2-normalised embedding, appended to the embedding store.
        embedding: Box<Embedding>,
    },
    /// Decode or format failure: recorded as an error row.
    Failed(String),
    /// Not recorded, file stays pending: an I/O error (offline drive,
    /// permissions) that may go away.
    Skipped,
    /// Never started because the job was cancelled; not counted.
    Unstarted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_order_is_priority() {
        for s in Stage::ALL {
            assert_eq!(s.as_str().parse::<Stage>(), Ok(s));
        }
        assert!(Stage::Meta < Stage::Dsp && Stage::Dsp < Stage::Embed);
        assert!("bogus".parse::<Stage>().is_err());
    }
}

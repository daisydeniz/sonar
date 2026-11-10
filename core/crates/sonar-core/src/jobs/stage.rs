//! Pipeline stages, their versions and per-file results.

use std::fmt;
use std::str::FromStr;

use serde::Serialize;

use crate::meta::BasicMeta;

/// Current version of the `meta` stage (format, duration). Bump it to
/// re-run the stage on every file.
pub const META_VERSION: u32 = 1;
/// Current version of the `dsp` stage (decode window, peak/loudness, and
/// later the DSP features of T3.x). Bump it to re-run the stage.
pub const DSP_VERSION: u32 = 1;
/// Version of the `embed` stage (T5.2). Declared so the stage name and
/// version slot exist in the schema; there is no implementation yet.
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
    /// Analysis decode (30 s window) plus peak and loudness.
    Dsp,
    /// CLAP embedding. **Not implemented yet** (T5.2): it has a name and a
    /// version but [`Stage::is_runnable`] is false, so the runner skips it.
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

    /// Whether the runner can execute this stage in this build. `Embed` is
    /// declared but not implemented, so it is always skipped.
    pub fn is_runnable(self) -> bool {
        !matches!(self, Self::Embed)
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

/// What a worker produced for one claimed file.
#[derive(Debug)]
pub(crate) enum FileResult {
    /// Stage completed (`Ok` or `Partial`).
    Done {
        /// Measured metadata (`meta.partial` / `meta.problem` say whether
        /// and why the result is partial).
        meta: BasicMeta,
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
        assert!(!Stage::Embed.is_runnable());
        assert!(Stage::Meta.is_runnable() && Stage::Dsp.is_runnable());
    }
}

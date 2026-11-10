//! Basic metadata of an audio file (`docs/ARCHITECTURE.md` §3 `audio_meta`,
//! §4 "basic meta"): duration, sample rate, channels, bit depth, sample
//! peak and integrated loudness.
//!
//! Format fields come from the decoder ([`crate::decode::SourceInfo`]: the
//! true, counted length). Peak and loudness are measured in the same pass
//! as the analysis-window decode ([`Decoder::decode_with_meta`]), on the
//! decoded samples at the file's own rate, all channels, before downmix or
//! resampling, over the first [`MetaOptions::max_meta_secs`] of the file.
//!
//! - Peak: sample peak (not true peak, no oversampling), `max |x|` over all
//!   channels, `20·log10`, dBFS; integer samples are scaled by
//!   `1/2^(bits−1)`, so full scale is 0 dBFS and float files may exceed it.
//! - Loudness: ITU-R BS.1770-4 gated integrated loudness, LUFS
//!   ([`meter`], [`kweight`], [`channels`]); short files are approximated
//!   (see [`meter`]).
//! - Absent values are `None` (digital silence, below the −70 LUFS gate,
//!   nothing measured), never −∞ or NaN.

pub mod channels;
pub mod kweight;
pub mod meter;

#[cfg(test)]
mod tests;

use std::path::Path;

use serde::Serialize;

use crate::decode::{DecodeOptions, Decoder, SourceInfo};
use crate::Result;
pub use meter::Meter;

/// Default meta window (s): peak/loudness are measured over the first
/// 10 minutes; longer files still get their exact duration.
pub const DEFAULT_MAX_META_SECS: f64 = 600.0;

/// Options for the meta measurement.
#[derive(Debug, Clone)]
pub struct MetaOptions {
    /// Peak/loudness are measured over at most this many seconds from the
    /// start of the file. Negative or NaN = 0 (nothing measured), infinity
    /// = whole file.
    pub max_meta_secs: f64,
}

impl Default for MetaOptions {
    fn default() -> Self {
        Self {
            max_meta_secs: DEFAULT_MAX_META_SECS,
        }
    }
}

impl MetaOptions {
    /// Frames of the meta window at `rate` Hz.
    pub(crate) fn max_frames(&self, rate: u32) -> u64 {
        let s = self.max_meta_secs;
        if s.is_nan() || s <= 0.0 {
            return 0;
        }
        // Saturating float → int cast: infinity becomes u64::MAX.
        (s * f64::from(rate)).ceil() as u64
    }
}

/// Basic metadata of one file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BasicMeta {
    /// Playable length, rounded to the nearest millisecond.
    pub duration_ms: u64,
    /// Playable length in frames (exact).
    pub num_frames: u64,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Bits per sample for PCM/lossless codecs; `None` for lossy.
    pub bit_depth: Option<u32>,
    /// Sample peak (dBFS) over the measured span; `None` for silence.
    pub peak_dbfs: Option<f64>,
    /// BS.1770-4 integrated loudness (LUFS) over the measured span; `None`
    /// below the absolute gate (silence).
    pub loudness_lufs: Option<f64>,
    /// Span actually measured for peak/loudness (s); less than the
    /// duration when the file is longer than the meta window.
    pub measured_s: f64,
    /// Decoding was incomplete (damaged/truncated file): values cover the
    /// readable part only.
    pub partial: bool,
    /// Why `partial` is set (first problem seen while decoding or metering);
    /// the pipeline stores it in `file_stages.message` (T1.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl BasicMeta {
    /// Combines the decoder's format info with a finished measurement.
    pub(crate) fn from_measurement(
        src: &SourceInfo,
        meter: &Meter,
        partial: bool,
        problem: Option<String>,
    ) -> Self {
        Self {
            duration_ms: duration_ms(src.num_frames, src.sample_rate),
            num_frames: src.num_frames,
            sample_rate: src.sample_rate,
            channels: src.channels,
            bit_depth: src.bit_depth,
            peak_dbfs: meter.peak_dbfs(),
            loudness_lufs: meter.integrated_lufs(),
            measured_s: if meter.measured_frames() == 0 {
                0.0
            } else {
                meter.measured_secs()
            },
            partial,
            problem,
        }
    }
}

/// `frames / rate` in milliseconds, rounded half up (exact integer math).
pub fn duration_ms(frames: u64, rate: u32) -> u64 {
    let rate = u128::from(rate.max(1));
    let ms = (u128::from(frames) * 1000 + rate / 2) / rate;
    u64::try_from(ms).unwrap_or(u64::MAX)
}

/// Measures `path` alone (no analysis window is decoded). Prefer
/// [`Decoder::decode_with_meta`] in the pipeline: one decode for both.
pub fn analyze_file(path: &Path, opts: &MetaOptions) -> Result<BasicMeta> {
    let decode = DecodeOptions {
        max_analysis_secs: Some(0.0),
        ..DecodeOptions::default()
    };
    Decoder::new()
        .decode_with_meta(path, &decode, opts)
        .map(|(_, meta)| meta)
}

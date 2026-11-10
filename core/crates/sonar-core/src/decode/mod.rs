//! Audio decoding for analysis: any supported file → mono `f32` at the
//! analysis rate (22.05 kHz), capped to an analysis window, plus the true
//! full-length duration (`docs/ARCHITECTURE.md` §4).
//!
//! Pipeline: symphonia demux/decode ([`source`]) → equal-weight mono
//! downmix ([`mix`]) of the first `max_analysis_secs` → band-limited
//! resampling with rubato ([`resample`]); frames past the window are only
//! counted from packet durations, never decoded ([`stream`]). A length-only
//! probe (zero analysis and meter windows: the meta stage) of an intact
//! uncompressed PCM WAV/AIFF takes the length from the header instead,
//! without reading the payload, under rules that make it equal to the
//! counted length ([`header`], ADR-015).
//! [`Decoder::decode_with_meta`] additionally measures peak and loudness
//! ([`crate::meta`]) in the same pass, at the native rate, before downmix.
//!
//! Supported (symphonia 0.6 features in `core/Cargo.toml`): WAV (PCM int
//! 8–32, float 32/64, extensible, A-law/µ-law, IMA/MS ADPCM), AIFF/AIFC,
//! FLAC, MP3, Ogg Vorbis, MP4/M4A (AAC-LC, ALAC).
//!
//! Robustness: no input may panic, hang or allocate without bound.
//! Headers are validated against hard limits before decoder buffers are
//! sized ([`source::MAX_CHANNELS`] etc.), the buffered window is capped
//! ([`HARD_MAX_SOURCE_FRAMES`]), undecodable packets are bounded
//! ([`stream::MAX_CONSECUTIVE_ERRORS`]) and every call runs under
//! `catch_unwind`, so a panic inside a third-party codec becomes an
//! [`crate::Error::Decode`].
//!
//! Empty files: a valid header with zero frames decodes to `Ok` with no
//! samples and `num_frames == 0`; a zero-byte file is an error.

mod gapless;
mod header;
mod inner;
mod mix;
mod resample;
pub mod source;
pub mod stream;

#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod robust_tests;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::Serialize;

use crate::meta::{BasicMeta, MetaOptions, Meter};
use crate::quiet_panic;
use crate::Result;
use resample::MonoResampler;
use source::decode_err;

/// Analysis sample rate (Hz) used across the DSP pipeline.
pub const ANALYSIS_RATE: u32 = 22_050;
/// Default analysis window (s).
pub const DEFAULT_MAX_ANALYSIS_SECS: f64 = 30.0;
/// Absolute cap on source frames buffered for one file (64 Mi frames =
/// 256 MiB of `f32`, ~25 min at 44.1 kHz), applied even when no analysis
/// window is requested.
pub const HARD_MAX_SOURCE_FRAMES: usize = 1 << 26;
/// Minimum extra source time decoded past the window so the resampler's
/// band-limited output is exact up to the window edge (then trimmed). The
/// actual margin is at least two resampler FFT blocks (one block of later
/// input influences the output; at 8/16 kHz a block is 140/80 ms).
const RESAMPLE_MARGIN_SECS: f64 = 0.05;
/// Mono scratch larger than this (samples, 32 MiB) is released after a file.
const RETAIN_SCRATCH_SAMPLES: usize = 1 << 23;
/// Planar packet scratch larger than this (samples, 4 MiB in total) is
/// released after a file (typical: ≤ 4,096 frames × 2 channels).
const RETAIN_PLANAR_SAMPLES: usize = 1 << 20;
/// Upper bound for the up-front reservation driven by the header length.
const MAX_RESERVE_FRAMES: usize = 1 << 21;

/// Options for [`decode_file`] / [`Decoder::decode`].
#[derive(Debug, Clone)]
pub struct DecodeOptions {
    /// Output sample rate (Hz); [`ANALYSIS_RATE`] by default. Must be within
    /// [`source::MIN_SAMPLE_RATE`]..=[`source::MAX_SAMPLE_RATE`].
    pub target_rate: u32,
    /// Analysis window from the start of the file (s). `None` = whole file
    /// (still bounded by [`HARD_MAX_SOURCE_FRAMES`]). `Some(0.0)` returns no
    /// samples but still the true duration (cheap metadata probe; for
    /// intact PCM WAV/AIFF read from the header, ADR-015).
    /// Negative or NaN values are treated as 0.
    pub max_analysis_secs: Option<f64>,
    /// Set to `true` from another thread to stop with [`crate::Error::Cancelled`].
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            target_rate: ANALYSIS_RATE,
            max_analysis_secs: Some(DEFAULT_MAX_ANALYSIS_SECS),
            cancel: None,
        }
    }
}

/// Format of the file as stored (before downmix/resampling).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceInfo {
    /// Container short name (`wave`, `aiff`, `flac`, `isomp4`, ...).
    pub container: String,
    /// Codec short name (`pcm_s16le`, `pcm_f32le`, `flac`, `mp3`, ...).
    pub codec: String,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Bits per sample for lossless/PCM codecs; `None` for lossy codecs.
    pub bit_depth: Option<u32>,
    /// True playable length in frames (counted, not just declared).
    pub num_frames: u64,
    /// Length declared by the container header, if any.
    pub declared_frames: Option<u64>,
    /// `num_frames / sample_rate` (s).
    pub duration_s: f64,
}

/// Result of decoding one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// Mono samples at [`Decoded::sample_rate`], first
    /// `max_analysis_secs` of the file only.
    pub samples: Vec<f32>,
    /// Rate of `samples` (= the requested target rate).
    pub sample_rate: u32,
    /// Stored format and true duration.
    pub source: SourceInfo,
    /// The file is longer than the analysis window (`samples` covers only
    /// its start).
    pub truncated_for_analysis: bool,
    /// Decoding stopped early or skipped damaged packets (truncated or
    /// corrupt file); `samples` and `num_frames` cover what was readable.
    pub partial: bool,
    /// First problem seen when `partial` is set.
    pub problem: Option<String>,
}

impl Decoded {
    /// Duration of `samples` (s).
    pub fn analysis_secs(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }
}

/// Decodes `path` with fresh scratch state. Prefer one [`Decoder`] per
/// worker thread when decoding many files (reuses FFT plans and buffers).
pub fn decode_file(path: &Path, opts: &DecodeOptions) -> Result<Decoded> {
    Decoder::new().decode(path, opts)
}

/// Reusable decoder: keeps resampler FFT plans (per rate pair) and scratch
/// buffers between files. Not shared between threads; create one each.
#[derive(Default)]
pub struct Decoder {
    resampler: MonoResampler,
    planar: stream::Planar,
    mono: Vec<f32>,
    meter: Meter,
}

impl Decoder {
    /// New decoder with empty caches.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `path`. Never panics: internal panics (e.g. in a codec)
    /// are caught and reported as [`crate::Error::Decode`], and the scratch state
    /// is discarded.
    pub fn decode(&mut self, path: &Path, opts: &DecodeOptions) -> Result<Decoded> {
        self.decode_guarded(path, opts, None, true).map(|(d, _)| d)
    }

    /// [`Decoder::decode`] plus basic metadata ([`crate::meta`]) in the same
    /// pass: peak and loudness are measured on the native-rate, all-channel
    /// samples of the first `meta.max_meta_secs`. Packets are decoded up to
    /// the longer of the analysis and meta windows; only the analysis
    /// window is buffered, and the returned [`Decoded`] is identical to
    /// [`Decoder::decode`]'s, except `num_frames` on a truncated file whose
    /// short final packet lies past the analysis window (ADR-012). A packet past the analysis window that the
    /// codec rejects stops the measurement there (the rest is counted) and
    /// sets [`BasicMeta::partial`].
    pub fn decode_with_meta(
        &mut self,
        path: &Path,
        opts: &DecodeOptions,
        meta: &MetaOptions,
    ) -> Result<(Decoded, BasicMeta)> {
        let (d, m) = self.decode_guarded(path, opts, Some(meta), true)?;
        let m = m.ok_or_else(|| decode_err(path, "meta not measured"))?;
        Ok((d, m))
    }

    /// Test hook: like the public methods, but a length-only probe always
    /// counts packets (never takes the [`header`] length).
    #[cfg(test)]
    pub(crate) fn decode_counted(
        &mut self,
        path: &Path,
        opts: &DecodeOptions,
        meta: Option<&MetaOptions>,
    ) -> Result<(Decoded, Option<BasicMeta>)> {
        self.decode_guarded(path, opts, meta, false)
    }

    /// `allow_header`: a length-only probe may take the length from the
    /// container header ([`header`]); only tests pass `false`. The body is
    /// in [`inner`].
    fn decode_guarded(
        &mut self,
        path: &Path,
        opts: &DecodeOptions,
        meta: Option<&MetaOptions>,
        allow_header: bool,
    ) -> Result<(Decoded, Option<BasicMeta>)> {
        let result = quiet_panic::guarded(|| self.decode_inner(path, opts, meta, allow_header));
        let out = result.unwrap_or_else(|_| {
            *self = Self::default();
            Err(decode_err(path, "internal decoder panic"))
        });
        self.mono.clear();
        if self.mono.capacity() > RETAIN_SCRATCH_SAMPLES {
            self.mono = Vec::new();
        }
        let planar: usize = self.planar.iter().map(Vec::capacity).sum();
        if planar > RETAIN_PLANAR_SAMPLES {
            self.planar = Vec::new();
        }
        out
    }
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder").finish_non_exhaustive()
    }
}

/// Frame counts derived from the requested window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    /// Source frames inside the analysis window.
    window_frames: u64,
    /// Source frames to decode (window + resampler margin, hard-capped).
    keep_frames: usize,
    /// Maximum output frames at the target rate.
    max_out: usize,
}

/// `block` is the resampler's FFT input block (0 when not resampling).
fn analysis_window(max_secs: Option<f64>, rate: u32, target: u32, block: usize) -> Window {
    let hard = HARD_MAX_SOURCE_FRAMES as f64 / f64::from(rate);
    let secs = match max_secs {
        Some(s) if s.is_nan() || s <= 0.0 => 0.0,
        Some(s) => s.min(hard),
        None => hard,
    };
    let window_frames = (secs * f64::from(rate)).ceil() as u64;
    let margin = if rate == target || secs == 0.0 {
        0
    } else {
        let min_margin = (RESAMPLE_MARGIN_SECS * f64::from(rate)).ceil() as u64;
        min_margin.max(2 * block as u64)
    };
    let keep = (window_frames + margin).min(HARD_MAX_SOURCE_FRAMES as u64);
    Window {
        window_frames,
        keep_frames: keep as usize,
        max_out: (secs * f64::from(target)).ceil() as usize,
    }
}

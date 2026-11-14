//! Audio for the UI (T5.6): waveform peaks and decoded preview PCM.
//!
//! Both decode the file on the caller's thread (blocking, a few ms to a
//! second for long files); the host calls them off the main thread. Nothing
//! is cached or written to the store: peaks are cheap to recompute and a
//! host caches what it draws.

use super::{lock, Engine};
use crate::decode::source::{MAX_SAMPLE_RATE, MIN_SAMPLE_RATE};
use crate::decode::{decode_file, probe_sample_rate, DecodeOptions, ANALYSIS_RATE};
use crate::{Error, Result};

/// Most buckets [`Engine::waveform_peaks`] accepts.
pub const MAX_PEAK_BUCKETS: usize = 16_384;
/// Longest stretch [`Engine::preview_pcm`] decodes (s); longer files are
/// cut and flagged [`PreviewPcm::truncated`].
pub const MAX_PREVIEW_SECS: f64 = 600.0;

/// Decoded audio for a player that cannot read the file's format itself
/// (e.g. Ogg Vorbis with AVFoundation).
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewPcm {
    /// Sample rate (Hz): the file's own rate, no resampling.
    pub sample_rate: u32,
    /// Channel count of `samples` (v1: always 1, equal-weight downmix).
    pub channels: u16,
    /// Interleaved `f32` samples in -1..1.
    pub samples: Vec<f32>,
    /// The file is longer than [`MAX_PREVIEW_SECS`] and was cut.
    pub truncated: bool,
}

/// Min and max of `samples` in `buckets` equal slices, for drawing a
/// waveform: returns exactly `buckets` `(min, max)` pairs. With fewer
/// samples than buckets, buckets repeat the nearest sample; no samples give
/// all zeros. `buckets == 0` gives an empty vector.
pub fn waveform_peaks(samples: &[f32], buckets: usize) -> Vec<(f32, f32)> {
    let len = samples.len();
    (0..buckets)
        .map(|i| {
            if len == 0 {
                return (0.0, 0.0);
            }
            let start = (i as u128 * len as u128 / buckets as u128) as usize;
            let end = ((i as u128 + 1) * len as u128 / buckets as u128) as usize;
            let end = end.max(start + 1).min(len);
            let start = start.min(end - 1);
            let (lo, hi) = samples[start..end]
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
            // An all-NaN bucket leaves the sentinels: draw it as silence.
            if lo > hi {
                (0.0, 0.0)
            } else {
                (lo, hi)
            }
        })
        .collect()
}

impl Engine {
    /// Path of file `id` or [`Error::FileNotFound`].
    fn existing_path(&self, id: i64) -> Result<std::path::PathBuf> {
        lock(&self.inner.store)
            .file_path(id)?
            .ok_or(Error::FileNotFound { id })
    }

    /// `buckets` min/max pairs over the whole file (mono, analysis rate),
    /// for a waveform view. Blocking (decodes the file).
    ///
    /// # Errors
    /// [`Error::InvalidInput`] for 0 or more than [`MAX_PEAK_BUCKETS`]
    /// buckets, [`Error::FileNotFound`], [`Error::Decode`] /
    /// [`Error::UnsupportedFormat`] / [`Error::Io`] for the audio file.
    pub fn waveform_peaks(&self, id: i64, buckets: usize) -> Result<Vec<(f32, f32)>> {
        if buckets == 0 || buckets > MAX_PEAK_BUCKETS {
            return Err(Error::InvalidInput {
                reason: format!("peak buckets must be 1..={MAX_PEAK_BUCKETS}, got {buckets}"),
            });
        }
        let path = self.existing_path(id)?;
        let opts = DecodeOptions {
            target_rate: ANALYSIS_RATE,
            max_analysis_secs: None,
            cancel: None,
        };
        let decoded = decode_file(&path, &opts)?;
        Ok(waveform_peaks(&decoded.samples, buckets))
    }

    /// Decoded PCM of file `id` at its own sample rate (see [`PreviewPcm`]),
    /// at most [`MAX_PREVIEW_SECS`]. Blocking (decodes the file).
    ///
    /// # Errors
    /// [`Error::FileNotFound`], [`Error::Decode`] /
    /// [`Error::UnsupportedFormat`] / [`Error::Io`] for the audio file.
    pub fn preview_pcm(&self, id: i64) -> Result<PreviewPcm> {
        let path = self.existing_path(id)?;
        // Header only: compressed files are decoded once, below.
        let rate = probe_sample_rate(&path)?.clamp(MIN_SAMPLE_RATE, MAX_SAMPLE_RATE);
        let opts = DecodeOptions {
            target_rate: rate,
            max_analysis_secs: Some(MAX_PREVIEW_SECS),
            cancel: None,
        };
        let decoded = decode_file(&path, &opts)?;
        Ok(PreviewPcm {
            sample_rate: decoded.sample_rate,
            channels: 1,
            truncated: decoded.truncated_for_analysis,
            samples: decoded.samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_cover_the_signal_in_equal_buckets() {
        let samples = [0.0, 0.5, -0.25, 1.0, -1.0, 0.1, 0.2, 0.3];
        let p = waveform_peaks(&samples, 4);
        assert_eq!(p, vec![(0.0, 0.5), (-0.25, 1.0), (-1.0, 0.1), (0.2, 0.3)]);
    }

    #[test]
    fn peaks_handle_short_empty_and_zero() {
        assert_eq!(waveform_peaks(&[0.5, -0.5], 4).len(), 4);
        assert!(waveform_peaks(&[0.5, -0.5], 4)
            .iter()
            .all(|&(lo, hi)| lo == hi && lo.abs() == 0.5));
        assert_eq!(waveform_peaks(&[], 3), vec![(0.0, 0.0); 3]);
        assert!(waveform_peaks(&[1.0], 0).is_empty());
    }

    #[test]
    fn all_nan_bucket_is_silence_not_sentinels() {
        let p = waveform_peaks(&[f32::NAN, f32::NAN, 0.5, -0.5], 2);
        assert_eq!(p, vec![(0.0, 0.0), (-0.5, 0.5)]);
    }

    #[test]
    fn peaks_of_a_long_signal_bound_every_sample() {
        let samples: Vec<f32> = (0..10_007).map(|i| (i as f32 * 0.01).sin()).collect();
        let p = waveform_peaks(&samples, 100);
        assert_eq!(p.len(), 100);
        let lo = p.iter().map(|b| b.0).fold(f32::MAX, f32::min);
        let hi = p.iter().map(|b| b.1).fold(f32::MIN, f32::max);
        assert_eq!(lo, samples.iter().copied().fold(f32::MAX, f32::min));
        assert_eq!(hi, samples.iter().copied().fold(f32::MIN, f32::max));
        assert!(p.iter().all(|&(a, b)| a <= b));
    }
}

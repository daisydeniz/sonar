//! Frame-level DSP features on the analysis signal (mono `f32` at
//! [`crate::decode::ANALYSIS_RATE`], first 30 s; `docs/ARCHITECTURE.md`
//! §4–5): STFT power spectrogram ([`stft`]), log-mel ([`mel`]), MFCC and
//! deltas ([`mfcc`]), onset strength envelope ([`onset`]).
//!
//! [`Analyzer`] computes the spectrogram **once** per file and derives the
//! rest from it; later features (spectral stats, chroma, tempo) take the
//! [`Spectrogram`] / [`FeatureMatrix`] values from [`Features`] instead of
//! re-running the FFT. An analyzer owns its FFT plan, window, filterbank
//! and DCT matrix: keep one per worker thread.
//!
//! Every input (empty, shorter than one frame, silent, non-finite) gives
//! at least one frame of finite values; silence maps to the dB floor
//! ([`mel::POWER_FLOOR`]) and a zero onset envelope.

pub mod mel;
pub mod mfcc;
pub mod onset;
pub mod stft;

#[cfg(test)]
mod mel_tests;
#[cfg(test)]
mod mfcc_tests;
#[cfg(test)]
mod onset_tests;
#[cfg(test)]
mod stft_tests;
#[cfg(test)]
mod tests;

pub use mel::{MelFilterbank, MelNorm};
pub use mfcc::{deltas, Dct, DELTA_WIDTH};
pub use onset::onset_strength;
pub use stft::{Spectrogram, Stft};

use crate::Result;

/// Version of the DSP feature definitions below; bump when any parameter
/// or formula changes so stored features are recomputed.
pub const DSP_VERSION: u32 = 1;
/// FFT size (93 ms at 22.05 kHz; 10.8 Hz bins).
pub const N_FFT: usize = 2048;
/// Hop between frames (23.2 ms at 22.05 kHz; ≈ 43.07 frames/s).
pub const HOP: usize = 512;
/// Mel bands.
pub const N_MELS: usize = 64;
/// Lowest mel edge (Hz); excludes DC and sub-audio rumble.
pub const MEL_FMIN: f64 = 20.0;
/// MFCC coefficients kept, `c0..c19` (`c0` tracks frame loudness).
pub const N_MFCC: usize = 20;

/// A `n_frames × n_cols` row-major matrix of per-frame features.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FeatureMatrix {
    /// Rows (frames).
    pub n_frames: usize,
    /// Columns (bands / coefficients).
    pub n_cols: usize,
    /// `data[t * n_cols + c]`.
    pub data: Vec<f32>,
}

impl FeatureMatrix {
    /// All-zero matrix.
    pub fn zeros(n_frames: usize, n_cols: usize) -> Self {
        Self {
            n_frames,
            n_cols,
            data: vec![0.0; n_frames * n_cols],
        }
    }

    /// Row `t` (empty past the end).
    pub fn row(&self, t: usize) -> &[f32] {
        self.data
            .get(t * self.n_cols..(t + 1) * self.n_cols)
            .unwrap_or(&[])
    }

    /// Rows in time order.
    pub fn rows(&self) -> std::slice::ChunksExact<'_, f32> {
        self.data.chunks_exact(self.n_cols.max(1))
    }

    /// Mutable rows in time order.
    pub fn rows_mut(&mut self) -> std::slice::ChunksExactMut<'_, f32> {
        self.data.chunks_exact_mut(self.n_cols.max(1))
    }

    /// Per-column mean and (population) standard deviation over frames;
    /// zeros when there are no frames.
    pub fn column_stats(&self) -> (Vec<f32>, Vec<f32>) {
        let mut sum = vec![0.0f64; self.n_cols];
        let mut sq = vec![0.0f64; self.n_cols];
        for row in self.rows() {
            for ((s, q), &v) in sum.iter_mut().zip(sq.iter_mut()).zip(row) {
                *s += f64::from(v);
                *q += f64::from(v) * f64::from(v);
            }
        }
        let n = self.n_frames.max(1) as f64;
        let mean: Vec<f32> = sum.iter().map(|s| (s / n) as f32).collect();
        let std = sum
            .iter()
            .zip(&sq)
            .map(|(s, q)| (q / n - (s / n) * (s / n)).max(0.0).sqrt() as f32)
            .collect();
        (mean, std)
    }
}

/// Frame-level features of one signal, all on the same frame grid
/// (frame `t` centred at `t · HOP / sample_rate` s).
#[derive(Debug, Clone, PartialEq)]
pub struct Features {
    /// STFT power spectrogram (`N_FFT`, `HOP`, periodic Hann).
    pub spectrogram: Spectrogram,
    /// Log-mel spectrogram in dB (`N_MELS` Slaney-normalized bands from
    /// [`MEL_FMIN`] to Nyquist).
    pub log_mel: FeatureMatrix,
    /// MFCC `c0..c{N_MFCC-1}`.
    pub mfcc: FeatureMatrix,
    /// Onset strength envelope, one value per frame.
    pub onset: Vec<f32>,
}

impl Features {
    /// Frames per second of every per-frame feature.
    pub fn frame_rate(&self) -> f32 {
        self.spectrogram.frame_rate()
    }
}

/// Reusable feature extractor for one sample rate (FFT plan, window, mel
/// filterbank and DCT built once).
#[derive(Debug)]
pub struct Analyzer {
    sample_rate: u32,
    stft: Stft,
    mel: MelFilterbank,
    dct: Dct,
}

impl Analyzer {
    /// Analyzer with the module's parameters for signals at `sample_rate`
    /// (normally [`crate::decode::ANALYSIS_RATE`]). Fails if the rate is too
    /// low for the mel range (`sample_rate / 2 ≤ MEL_FMIN`).
    pub fn new(sample_rate: u32) -> Result<Self> {
        let nyquist = f64::from(sample_rate) / 2.0;
        Ok(Self {
            sample_rate,
            stft: Stft::new(N_FFT, HOP)?,
            mel: MelFilterbank::new(
                sample_rate,
                N_FFT,
                N_MELS,
                MEL_FMIN,
                nyquist,
                MelNorm::Slaney,
            )?,
            dct: Dct::new(N_MELS, N_MFCC)?,
        })
    }

    /// Sample rate this analyzer was built for.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The mel filterbank (for consumers that need band frequencies).
    pub fn mel(&self) -> &MelFilterbank {
        &self.mel
    }

    /// Spectrogram, log-mel, MFCC and onset envelope of `samples`.
    pub fn analyze(&mut self, samples: &[f32]) -> Result<Features> {
        let spectrogram = self.stft.power(samples, self.sample_rate)?;
        let log_mel = self.mel.log_mel(&spectrogram);
        let mfcc = self.dct.mfcc(&log_mel);
        let onset = onset_strength(&log_mel);
        Ok(Features {
            spectrogram,
            log_mel,
            mfcc,
            onset,
        })
    }
}

//! Mel filterbank and log-mel spectrogram.
//!
//! Mel scale: Slaney's form (Auditory Toolbox, Slaney 1998, after Stevens,
//! Volkmann & Newman 1937): linear below 1 kHz (`m = 3f/200`), logarithmic
//! above (`m = 15 + 27·ln(f/1000)/ln 6.4`). This is librosa's default
//! (`htk=False`); the HTK formula `2595·log10(1 + f/700)` is not used.
//!
//! Filters: `n_mels` triangles with edges on `n_mels + 2` points equally
//! spaced in mel between `fmin` and `fmax`, evaluated at the FFT bin
//! centres. [`MelNorm::Slaney`] scales each triangle by `2 / (f_hi − f_lo)`
//! (constant energy per band for white noise, librosa `norm="slaney"`);
//! [`MelNorm::Peak`] keeps peak 1, so overlapping triangles sum to 1
//! between the first and last centre (partition of unity).

use super::stft::Spectrogram;
use super::FeatureMatrix;
use crate::{Error, Result};

/// Power floor before the dB conversion (−100 dB): silence and empty
/// bands map to this finite value (librosa `amin`).
pub const POWER_FLOOR: f32 = 1.0e-10;

/// Hz → mel (Slaney).
pub fn hz_to_mel(f: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const BREAK_HZ: f64 = 1000.0;
    let min_log_mel = BREAK_HZ / F_SP;
    if f < BREAK_HZ {
        f / F_SP
    } else {
        min_log_mel + (f / BREAK_HZ).ln() / log_step()
    }
}

/// Mel → Hz (Slaney), inverse of [`hz_to_mel`].
pub fn mel_to_hz(m: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const BREAK_HZ: f64 = 1000.0;
    let min_log_mel = BREAK_HZ / F_SP;
    if m < min_log_mel {
        m * F_SP
    } else {
        BREAK_HZ * (log_step() * (m - min_log_mel)).exp()
    }
}

fn log_step() -> f64 {
    6.4f64.ln() / 27.0
}

/// Triangle normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MelNorm {
    /// Area normalization `2 / (f_hi − f_lo)` (Slaney / librosa default).
    Slaney,
    /// Peak 1 (triangles form a partition of unity).
    Peak,
}

/// One triangular filter: weights for bins `start..start + weights.len()`.
#[derive(Debug, Clone, PartialEq)]
struct Band {
    start: usize,
    weights: Vec<f32>,
}

/// Sparse mel filterbank for one (sample rate, FFT size).
#[derive(Debug, Clone, PartialEq)]
pub struct MelFilterbank {
    n_bins: usize,
    edges_hz: Vec<f64>,
    bands: Vec<Band>,
}

impl MelFilterbank {
    /// Builds `n_mels` filters over `fmin..fmax` Hz for an `n_fft`-point
    /// spectrum at `sample_rate`. Requires `0 ≤ fmin < fmax ≤ sr/2`.
    pub fn new(
        sample_rate: u32,
        n_fft: usize,
        n_mels: usize,
        fmin: f64,
        fmax: f64,
        norm: MelNorm,
    ) -> Result<Self> {
        let nyquist = f64::from(sample_rate) / 2.0;
        if n_mels == 0 || n_fft < 2 {
            return Err(Error::Dsp {
                reason: format!("need n_mels ≥ 1 and n_fft ≥ 2 (got {n_mels}, {n_fft})"),
            });
        }
        if !(fmin >= 0.0 && fmin < fmax && fmax <= nyquist) {
            return Err(Error::Dsp {
                reason: format!("mel range {fmin}..{fmax} Hz invalid for {sample_rate} Hz"),
            });
        }
        let (m_lo, m_hi) = (hz_to_mel(fmin), hz_to_mel(fmax));
        let edges_hz: Vec<f64> = (0..n_mels + 2)
            .map(|i| mel_to_hz(m_lo + (m_hi - m_lo) * i as f64 / (n_mels + 1) as f64))
            .collect();
        let n_bins = n_fft / 2 + 1;
        let bin_hz = f64::from(sample_rate) / n_fft as f64;
        let bands = edges_hz
            .windows(3)
            .map(|e| {
                let (lo, mid, hi) = (e[0], e[1], e[2]);
                let scale = match norm {
                    MelNorm::Slaney => 2.0 / (hi - lo),
                    MelNorm::Peak => 1.0,
                };
                let w = |k: usize| {
                    let f = k as f64 * bin_hz;
                    let rise = (f - lo) / (mid - lo);
                    let fall = (hi - f) / (hi - mid);
                    rise.min(fall).max(0.0) * scale
                };
                let start = (0..n_bins).find(|&k| w(k) > 0.0).unwrap_or(0);
                let end = (start..n_bins).find(|&k| w(k) <= 0.0).unwrap_or(n_bins);
                let weights = (start..end).map(|k| w(k) as f32).collect();
                Band { start, weights }
            })
            .collect();
        Ok(Self {
            n_bins,
            edges_hz,
            bands,
        })
    }

    /// Number of bands.
    pub fn n_mels(&self) -> usize {
        self.bands.len()
    }

    /// Spectrum length this bank expects (`n_fft / 2 + 1`).
    pub fn n_bins(&self) -> usize {
        self.n_bins
    }

    /// Centre frequency of band `b` (Hz).
    pub fn center_hz(&self, b: usize) -> Option<f64> {
        self.edges_hz.get(b + 1).copied()
    }

    /// Dense weights of band `b` (`n_bins` values, mostly zero).
    pub fn weights(&self, b: usize) -> Option<Vec<f32>> {
        let band = self.bands.get(b)?;
        let mut dense = vec![0.0; self.n_bins];
        dense[band.start..band.start + band.weights.len()].copy_from_slice(&band.weights);
        Some(dense)
    }

    /// Band energies `out[b] = Σ_k w_b[k] · frame[k]`. `frame` must hold
    /// [`Self::n_bins`] values and `out` [`Self::n_mels`]; extra entries
    /// are ignored, missing bins count as 0.
    pub fn apply(&self, frame: &[f32], out: &mut [f32]) {
        for (o, band) in out.iter_mut().zip(&self.bands) {
            let src = frame.get(band.start..).unwrap_or(&[]);
            *o = band.weights.iter().zip(src).map(|(w, p)| w * p).sum();
        }
    }

    /// Log-mel spectrogram in dB: `10·log10(max(mel power, POWER_FLOOR))`
    /// per frame (absolute scale, no top-dB clipping; silence = −100 dB).
    pub fn log_mel(&self, spec: &Spectrogram) -> FeatureMatrix {
        let mut out = FeatureMatrix::zeros(spec.n_frames, self.n_mels());
        for (frame, row) in spec.frames().zip(out.rows_mut()) {
            self.apply(frame, row);
            for v in row.iter_mut() {
                *v = power_to_db(*v);
            }
        }
        out
    }
}

/// `10·log10(max(p, POWER_FLOOR))`; NaN maps to the floor.
#[inline]
pub fn power_to_db(p: f32) -> f32 {
    10.0 * p.max(POWER_FLOOR).log10()
}

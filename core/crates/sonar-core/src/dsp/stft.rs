//! Short-time Fourier transform → power spectrogram.
//!
//! Centered framing (frame `t` is centered on sample `t · hop`, zero
//! padding of `n_fft / 2` on both ends, as `librosa.stft(center=True,
//! pad_mode="constant")`), periodic Hann window (Harris 1978, "On the use
//! of windows for harmonic analysis with the discrete Fourier transform",
//! Proc. IEEE 66(1)), real-input FFT via `realfft`. The output is the
//! unnormalized power `|X_t[k]|²` for `k = 0..=n_fft/2`, so a full-scale
//! sine on a bin centre has magnitude `A · Σw / 2 = A · n_fft / 4`.
//!
//! One [`Stft`] owns its FFT plan, window and scratch buffers; framing a
//! file allocates only the output spectrogram.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};

use crate::{Error, Result};

/// Input samples are clamped to ±this before windowing (and NaN becomes
/// 0), so power, mel and dB values stay finite for any `f32` input.
/// 10,000 = +80 dB above full scale: real audio is never touched.
pub const SAMPLE_CLAMP: f32 = 1.0e4;

/// Power spectrogram of one signal, frame-major
/// (`power[t * n_bins + k]` = power of bin `k` in frame `t`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spectrogram {
    /// Sample rate of the analyzed signal (Hz).
    pub sample_rate: u32,
    /// FFT size (samples per frame).
    pub n_fft: usize,
    /// Hop between frame centres (samples).
    pub hop: usize,
    /// Number of frames (`1 + len / hop`, at least 1).
    pub n_frames: usize,
    /// `|X_t[k]|²`, `n_frames × n_bins` values.
    pub power: Vec<f32>,
}

impl Spectrogram {
    /// Bins per frame (`n_fft / 2 + 1`, DC to Nyquist).
    pub fn n_bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    /// Power of frame `t` (`None` past the end).
    pub fn frame(&self, t: usize) -> Option<&[f32]> {
        let n = self.n_bins();
        self.power.get(t * n..(t + 1) * n)
    }

    /// Iterator over frames in time order.
    pub fn frames(&self) -> std::slice::ChunksExact<'_, f32> {
        self.power.chunks_exact(self.n_bins())
    }

    /// Centre frequency of bin `k` (Hz).
    pub fn bin_hz(&self, k: usize) -> f32 {
        k as f32 * self.sample_rate as f32 / self.n_fft.max(1) as f32
    }

    /// Frames per second (`sample_rate / hop`).
    pub fn frame_rate(&self) -> f32 {
        self.sample_rate as f32 / self.hop.max(1) as f32
    }

    /// Time of the centre of frame `t` (s).
    pub fn frame_time(&self, t: usize) -> f32 {
        (t * self.hop) as f32 / self.sample_rate.max(1) as f32
    }
}

/// Reusable STFT analyzer (plan + window + scratch).
pub struct Stft {
    n_fft: usize,
    hop: usize,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl std::fmt::Debug for Stft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stft")
            .field("n_fft", &self.n_fft)
            .field("hop", &self.hop)
            .finish_non_exhaustive()
    }
}

impl Stft {
    /// Plans an STFT with an even `n_fft ≥ 2` and `1 ≤ hop ≤ n_fft`.
    pub fn new(n_fft: usize, hop: usize) -> Result<Self> {
        if n_fft < 2 || !n_fft.is_multiple_of(2) {
            return Err(Error::Dsp {
                reason: format!("n_fft must be even and ≥ 2, got {n_fft}"),
            });
        }
        if hop == 0 || hop > n_fft {
            return Err(Error::Dsp {
                reason: format!("hop must be in 1..={n_fft}, got {hop}"),
            });
        }
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n_fft);
        let spectrum = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        Ok(Self {
            n_fft,
            hop,
            window: hann_periodic(n_fft),
            input: vec![0.0; n_fft],
            fft,
            spectrum,
            scratch,
        })
    }

    /// FFT size.
    pub fn n_fft(&self) -> usize {
        self.n_fft
    }

    /// Hop size.
    pub fn hop(&self) -> usize {
        self.hop
    }

    /// The analysis window (periodic Hann, `n_fft` values).
    pub fn window(&self) -> &[f32] {
        &self.window
    }

    /// Frames produced for `len` samples: `1 + len / hop` (centered
    /// framing; an empty signal still yields one all-zero frame).
    pub fn n_frames(&self, len: usize) -> usize {
        1 + len / self.hop
    }

    /// Power spectrogram of `samples` (recorded as being at `sample_rate`).
    pub fn power(&mut self, samples: &[f32], sample_rate: u32) -> Result<Spectrogram> {
        let n_frames = self.n_frames(samples.len());
        let n_bins = self.n_fft / 2 + 1;
        let mut power = vec![0.0f32; n_frames * n_bins];
        let half = self.n_fft / 2;
        for (t, out) in power.chunks_exact_mut(n_bins).enumerate() {
            // Frame t covers samples [t·hop − half, t·hop + half).
            let centre = t * self.hop;
            let lo = half.saturating_sub(centre); // first in-signal window index
            let first = centre.saturating_sub(half); // its sample index
            let avail = samples.len().saturating_sub(first);
            let hi = (lo + avail).min(self.n_fft).max(lo);
            self.input[..lo].fill(0.0);
            self.input[hi..].fill(0.0);
            let src = &samples[first.min(samples.len())..];
            for ((dst, &w), &x) in self.input[lo..hi]
                .iter_mut()
                .zip(&self.window[lo..hi])
                .zip(src)
            {
                *dst = w * sanitize(x);
            }
            self.fft
                .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
                .map_err(|e| Error::Internal {
                    reason: format!("FFT failed: {e}"),
                })?;
            for (p, c) in out.iter_mut().zip(&self.spectrum) {
                *p = c.re * c.re + c.im * c.im;
            }
        }
        Ok(Spectrogram {
            sample_rate,
            n_fft: self.n_fft,
            hop: self.hop,
            n_frames,
            power,
        })
    }
}

/// Clamp to ±[`SAMPLE_CLAMP`]; NaN → 0.
#[inline]
fn sanitize(x: f32) -> f32 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(-SAMPLE_CLAMP, SAMPLE_CLAMP)
    }
}

/// Periodic Hann window `w[n] = 0.5 − 0.5·cos(2πn/N)`, `n = 0..N` (the
/// DFT-even form: `Σw = N/2`, and overlap-adds to a constant at hop N/4).
pub fn hann_periodic(n: usize) -> Vec<f32> {
    let step = std::f64::consts::TAU / n.max(1) as f64;
    (0..n)
        .map(|i| (0.5 - 0.5 * (step * i as f64).cos()) as f32)
        .collect()
}

//! YIN fundamental-frequency estimator (A. de Cheveigné & H. Kawahara,
//! "YIN, a fundamental frequency estimator for speech and music", JASA
//! 111(4), 2002).
//!
//! Per frame (window `W`, lags `τ ≤ τ_max`), steps 2–5 of the paper:
//! 1. difference function `d(τ) = Σ_{j<W} (x_j − x_{j+τ})²`, expanded as
//!    `e(0) + e(τ) − 2·r(τ)` with the window energies `e` from a running sum
//!    and the cross-correlation `r` from one FFT product (Wiener–Khinchin),
//!    so a frame costs `O(N log N)` instead of `O(W·τ_max)`;
//! 2. cumulative mean normalized difference `d'(τ) = d(τ)·τ / Σ_{j≤τ} d(j)`,
//!    `d'(0) = 1`;
//! 3. absolute threshold: the first `τ ≥ τ_min` with `d'(τ) <` threshold,
//!    followed down to its local minimum; if none, the global minimum of
//!    `d'` over the lag range (reported but marked unvoiced);
//! 4. parabolic interpolation of `d` around the chosen lag (the paper
//!    interpolates the raw difference function to avoid the bias of `d'`).
//!
//! Step 6 (best local estimate) is not applied. Frame `t` analyses samples
//! `[t·hop, t·hop + W + τ_max + 1)` (zero-padded past the end, so any input
//! — even empty — yields at least one frame). Frames quieter than
//! [`YIN_SILENCE`] (mean square) are unvoiced without running the search.
//! A [`Yin`] owns its FFT plans and buffers: no allocation per frame.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use super::stft::SAMPLE_CLAMP;
use crate::{Error, Result};

/// Default absolute threshold on `d'` (the paper uses 0.1; 0.15 is slightly
/// more permissive for sampled instruments with noise and reverb).
pub const YIN_THRESHOLD: f32 = 0.15;
/// Frames whose mean square is below this (−100 dBFS) are silent/unvoiced.
pub const YIN_SILENCE: f64 = 1.0e-10;

/// YIN parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YinConfig {
    /// Sample rate of the analyzed signal (Hz).
    pub sample_rate: u32,
    /// Lowest detectable f0 (Hz); sets `τ_max = ⌈sr / fmin⌉`.
    pub fmin: f32,
    /// Highest detectable f0 (Hz); sets `τ_min = ⌊sr / fmax⌋`.
    pub fmax: f32,
    /// Integration window `W` (samples).
    pub window: usize,
    /// Hop between frame starts (samples).
    pub hop: usize,
    /// Absolute threshold on `d'` (voiced if the chosen dip is below it).
    pub threshold: f32,
}

impl YinConfig {
    /// Settings used by the Pitch aspect: 30 Hz–2 kHz, `W` = 1024 samples
    /// (46 ms at 22.05 kHz), hop 1024, threshold [`YIN_THRESHOLD`].
    pub fn aspects(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            fmin: 30.0,
            fmax: 2000.0,
            window: 1024,
            hop: 1024,
            threshold: YIN_THRESHOLD,
        }
    }
}

/// One analysed frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YinFrame {
    /// Estimated f0 (Hz); 0 for silent frames. Set (best dip) even when
    /// unvoiced.
    pub f0_hz: f32,
    /// `d'` at the chosen lag, clamped to `[0, 1]` (0 = perfectly periodic);
    /// `1 − aperiodicity` is the usual YIN confidence.
    pub aperiodicity: f32,
    /// The dip is below the threshold (and the frame is not silent).
    pub voiced: bool,
    /// Mean square of the frame's integration window.
    pub energy: f32,
}

impl YinFrame {
    const SILENT: Self = Self {
        f0_hz: 0.0,
        aperiodicity: 1.0,
        voiced: false,
        energy: 0.0,
    };
}

/// Reusable YIN tracker (FFT plans + scratch buffers).
pub struct Yin {
    cfg: YinConfig,
    tau_min: usize,
    tau_max: usize,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    buf_a: Vec<f32>,
    buf_b: Vec<f32>,
    frame: Vec<f32>,
    spec_a: Vec<Complex32>,
    spec_b: Vec<Complex32>,
    corr: Vec<f32>,
    scratch: Vec<Complex32>,
    diff: Vec<f64>,
    cmnd: Vec<f64>,
}

impl std::fmt::Debug for Yin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Yin")
            .field("cfg", &self.cfg)
            .field("tau_min", &self.tau_min)
            .field("tau_max", &self.tau_max)
            .finish_non_exhaustive()
    }
}

impl Yin {
    /// Plans a tracker; fails unless `0 < fmin < fmax`, `τ_min ≥ 2`,
    /// `W ≥ 1` and `hop ≥ 1`.
    pub fn new(cfg: YinConfig) -> Result<Self> {
        let sr = cfg.sample_rate as f32;
        let ok_range = cfg.fmin > 0.0 && cfg.fmax > cfg.fmin && cfg.fmax.is_finite();
        let tau_min = if ok_range {
            (sr / cfg.fmax) as usize
        } else {
            0
        };
        let tau_max = if ok_range {
            (sr / cfg.fmin).ceil() as usize
        } else {
            0
        };
        if !ok_range || tau_min < 2 || tau_max <= tau_min || tau_max > 1 << 20 {
            return Err(Error::Dsp {
                reason: format!(
                    "YIN f0 range {}–{} Hz unusable at {} Hz",
                    cfg.fmin, cfg.fmax, cfg.sample_rate
                ),
            });
        }
        if cfg.window == 0 || cfg.window > 1 << 20 || cfg.hop == 0 {
            return Err(Error::Dsp {
                reason: format!("YIN window {} / hop {} must be ≥ 1", cfg.window, cfg.hop),
            });
        }
        let frame_len = cfg.window + tau_max + 1;
        let n_fft = frame_len.next_power_of_two();
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(n_fft);
        let inv = planner.plan_fft_inverse(n_fft);
        let scratch_len = fwd.get_scratch_len().max(inv.get_scratch_len());
        Ok(Self {
            cfg,
            tau_min,
            tau_max,
            buf_a: vec![0.0; n_fft],
            buf_b: vec![0.0; n_fft],
            frame: vec![0.0; frame_len],
            spec_a: fwd.make_output_vec(),
            spec_b: fwd.make_output_vec(),
            corr: vec![0.0; n_fft],
            scratch: vec![Complex32::default(); scratch_len],
            diff: vec![0.0; tau_max + 2],
            cmnd: vec![0.0; tau_max + 2],
            fwd,
            inv,
        })
    }

    /// The configuration.
    pub fn config(&self) -> &YinConfig {
        &self.cfg
    }

    /// Lag range `(τ_min, τ_max)` in samples.
    pub fn lag_range(&self) -> (usize, usize) {
        (self.tau_min, self.tau_max)
    }

    /// Samples read per frame (`W + τ_max + 1`).
    pub fn frame_len(&self) -> usize {
        self.frame.len()
    }

    /// Frames produced for `len` samples: 1 up to one full frame, then one
    /// per `hop` that still fits entirely. Up to one hop of tail after the
    /// last full frame is not analysed (callers needing the very end, e.g.
    /// short one-shots in T3.5, should pad or account for it).
    pub fn n_frames(&self, len: usize) -> usize {
        let fl = self.frame_len();
        if len <= fl {
            1
        } else {
            1 + (len - fl) / self.cfg.hop
        }
    }

    /// Start time of frame `t` (s).
    pub fn frame_time(&self, t: usize) -> f32 {
        (t * self.cfg.hop) as f32 / self.cfg.sample_rate.max(1) as f32
    }

    /// Tracks `samples` (non-finite values count as 0, magnitudes are
    /// clamped like the STFT input).
    pub fn track(&mut self, samples: &[f32]) -> Result<Vec<YinFrame>> {
        let n = self.n_frames(samples.len());
        let mut out = Vec::with_capacity(n);
        for t in 0..n {
            out.push(self.analyze_frame(samples, t * self.cfg.hop)?);
        }
        Ok(out)
    }

    /// Analyses the frame starting at sample `start`.
    fn analyze_frame(&mut self, samples: &[f32], start: usize) -> Result<YinFrame> {
        let w = self.cfg.window;
        let src = samples.get(start..).unwrap_or(&[]);
        let n_src = src.len().min(self.frame.len());
        for (d, &x) in self.frame.iter_mut().zip(&src[..n_src]) {
            *d = sanitize(x);
        }
        self.frame[n_src..].fill(0.0);

        let e0: f64 = self.frame[..w].iter().map(|&x| f64::from(x).powi(2)).sum();
        let energy = e0 / w as f64;
        if energy < YIN_SILENCE {
            return Ok(YinFrame {
                energy: energy as f32,
                ..YinFrame::SILENT
            });
        }
        self.correlate()?;

        // Difference function and its cumulative-mean normalization.
        let fl = self.frame.len();
        let mut e_tau = e0;
        let mut running = 0.0f64;
        self.diff[0] = 0.0;
        self.cmnd[0] = 1.0;
        for tau in 1..=(self.tau_max + 1).min(fl - w) {
            let out_old = f64::from(self.frame[tau - 1]);
            let in_new = f64::from(self.frame[tau + w - 1]);
            e_tau = (e_tau - out_old * out_old + in_new * in_new).max(0.0);
            let d = (e0 + e_tau - 2.0 * f64::from(self.corr[tau])).max(0.0);
            self.diff[tau] = d;
            running += d;
            // A (near-)constant signal has d ≈ 0 at every lag: no period.
            self.cmnd[tau] = if running > 1.0e-6 * e0 * tau as f64 {
                d * tau as f64 / running
            } else {
                1.0
            };
        }

        let thr = f64::from(self.cfg.threshold);
        let (lo, hi) = (self.tau_min, self.tau_max);
        let mut chosen = None;
        let mut tau = lo;
        while tau <= hi {
            if self.cmnd[tau] < thr {
                while tau < hi && self.cmnd[tau + 1] < self.cmnd[tau] {
                    tau += 1;
                }
                chosen = Some(tau);
                break;
            }
            tau += 1;
        }
        let voiced = chosen.is_some();
        let tau = chosen.unwrap_or_else(|| {
            (lo..=hi).fold(lo, |best, t| {
                if self.cmnd[t] < self.cmnd[best] {
                    t
                } else {
                    best
                }
            })
        });
        let period = tau as f64 + parabolic_shift(&self.diff, tau);
        Ok(YinFrame {
            f0_hz: (f64::from(self.cfg.sample_rate) / period.max(1.0)) as f32,
            aperiodicity: self.cmnd[tau].clamp(0.0, 1.0) as f32,
            voiced,
            energy: energy as f32,
        })
    }

    /// `corr[τ] = Σ_{j<W} frame[j]·frame[j+τ]` via one FFT product
    /// (`n_fft ≥ W + τ_max + 1`, so the circular correlation does not wrap).
    fn correlate(&mut self) -> Result<()> {
        let w = self.cfg.window;
        let fl = self.frame.len();
        self.buf_a[..w].copy_from_slice(&self.frame[..w]);
        self.buf_a[w..].fill(0.0);
        self.buf_b[..fl].copy_from_slice(&self.frame);
        self.buf_b[fl..].fill(0.0);
        let fft_err = |e: realfft::FftError| Error::Internal {
            reason: format!("YIN FFT failed: {e}"),
        };
        self.fwd
            .process_with_scratch(&mut self.buf_a, &mut self.spec_a, &mut self.scratch)
            .map_err(fft_err)?;
        self.fwd
            .process_with_scratch(&mut self.buf_b, &mut self.spec_b, &mut self.scratch)
            .map_err(fft_err)?;
        for (b, a) in self.spec_b.iter_mut().zip(&self.spec_a) {
            *b *= a.conj();
        }
        // DC and Nyquist are real for real inputs; clear rounding residue.
        if let Some(c) = self.spec_b.first_mut() {
            c.im = 0.0;
        }
        if let Some(c) = self.spec_b.last_mut() {
            c.im = 0.0;
        }
        self.inv
            .process_with_scratch(&mut self.spec_b, &mut self.corr, &mut self.scratch)
            .map_err(fft_err)?;
        let scale = 1.0 / self.corr.len() as f32;
        for c in &mut self.corr {
            *c *= scale;
        }
        Ok(())
    }
}

/// Offset in `[-1, 1]` of the vertex of the parabola through
/// `y[τ−1], y[τ], y[τ+1]` (0 when flat or at the edges).
fn parabolic_shift(y: &[f64], tau: usize) -> f64 {
    let (Some(&a), Some(&b), Some(&c)) = (
        tau.checked_sub(1).and_then(|i| y.get(i)),
        y.get(tau),
        y.get(tau + 1),
    ) else {
        return 0.0;
    };
    let denom = a - 2.0 * b + c;
    if denom > 0.0 {
        (0.5 * (a - c) / denom).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Clamp to ±[`SAMPLE_CLAMP`]; NaN → 0.
#[inline]
pub(crate) fn sanitize(x: f32) -> f32 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(-SAMPLE_CLAMP, SAMPLE_CLAMP)
    }
}

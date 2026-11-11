//! Periodicity front end of the tempo estimator: autocorrelation of the
//! detrended onset strength envelope and a comb ("harmonic sum") salience
//! over candidate tempi.
//!
//! - **Envelope**: the accent envelope ([`super::accent`]), low-passed by
//!   a short triangular kernel ([`SMOOTH_KERNEL`]) so pulses at fractional
//!   frame offsets still meet at the beat lag, then made locally zero-mean
//!   by removing a 1 s centred moving average — the high-pass step of D.
//!   Ellis, "Beat tracking by dynamic programming" (J. New Music Research
//!   36(1), 2007, §3.1).
//! - **Autocorrelation** `r(τ)`, normalized so `r(0) = 1`. When the window
//!   holds the whole file (a loop joins its own start) it is *circular*,
//!   so every lag uses every frame; otherwise the unbiased linear estimate
//!   `Σ x[t]x[t+τ] / (N−τ)` over `Σ x² / N`. Lags up to [`MAX_LAG_S`] and
//!   half the window.
//! - **Salience** of a tempo with beat period `T` frames: the mean of
//!   `r(k·T)` over every multiple `k·T` within the lag range — the
//!   autocorrelation reading of the tempogram / comb-filter tempo
//!   estimators (Scheirer, JASA 103(1), 1998; Grosche & Müller,
//!   "Extracting predominant local pulse information from music
//!   recordings", IEEE TASLP 19(6), 2011, autocorrelation tempogram).
//!   Long multiples give sub-frame tempo resolution; fractional lags are
//!   read by linear interpolation.

use crate::classify::scores::detrend;

/// Longest autocorrelation lag used (s).
pub const MAX_LAG_S: f32 = 8.0;
/// Width of the moving average removed from the onset envelope (s).
pub const DETREND_S: f32 = 1.0;
/// Triangular smoothing kernel applied to the envelope (≈ ±46 ms).
pub const SMOOTH_KERNEL: [f32; 5] = [1.0, 2.0, 3.0, 2.0, 1.0];

/// Normalized autocorrelation of one file's detrended onset envelope.
#[derive(Debug, Clone)]
pub struct Periodicity {
    /// `r[τ]` for `τ = 0..=max_lag` frames (`r[0] = 1`).
    r: Vec<f32>,
    /// Frames per second of the envelope.
    fps: f32,
}

impl Periodicity {
    /// Autocorrelation of `onset` (frames at `fps`), circular when
    /// `circular`. `None` when the envelope is flat (silence, steady
    /// sound), too short for a lag of 2 frames, or `fps` is not positive.
    pub fn new(onset: &[f32], fps: f32, circular: bool) -> Option<Self> {
        if !(fps.is_finite() && fps > 0.0) {
            return None;
        }
        let clean: Vec<f32> = onset
            .iter()
            .map(|&o| if o.is_finite() { o.max(0.0) } else { 0.0 })
            .collect();
        let smooth = smooth(&clean, circular);
        let width = ((DETREND_S * fps).round() as usize) | 1;
        let x = detrend(&smooth, width);
        let n = x.len();
        let max_lag = ((MAX_LAG_S * fps) as usize).min(n / 2);
        let energy: f64 = x.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
        if max_lag < 2 || energy.is_nan() || energy <= 1e-12 {
            return None;
        }
        let mean_sq = energy / n as f64;
        let mut r = Vec::with_capacity(max_lag + 1);
        for tau in 0..=max_lag {
            let s = if circular {
                let (head, tail) = x.split_at(tau);
                // x[t]·x[(t+τ) mod n] = x[..n−τ]·x[τ..] + x[n−τ..]·x[..τ]
                dot(&x[..n - tau], tail) + dot(&x[n - tau..], head)
            } else {
                dot(&x[..n - tau], &x[tau..]) * n as f64 / (n - tau) as f64
            };
            let v = (s / n as f64 / mean_sq) as f32;
            r.push(if v.is_finite() { v } else { 0.0 });
        }
        Some(Self { r, fps })
    }

    /// Longest lag available (frames).
    pub fn max_lag(&self) -> usize {
        self.r.len() - 1
    }

    /// `r` at a fractional lag (linear interpolation; 0 outside the range).
    pub fn at(&self, lag: f32) -> f32 {
        if lag.is_nan() || lag < 0.0 {
            return 0.0;
        }
        let i = lag.floor() as usize;
        let frac = lag - i as f32;
        match (self.r.get(i), self.r.get(i + 1)) {
            (Some(&a), Some(&b)) => a + (b - a) * frac,
            (Some(&a), None) if frac == 0.0 => a,
            _ => 0.0,
        }
    }

    /// Beat period of `bpm` in frames.
    pub fn period(&self, bpm: f32) -> f32 {
        60.0 * self.fps / bpm
    }

    /// Comb salience of `bpm`: mean of `r(k·T)` over the multiples of the
    /// beat period `T` within the lag range (0 when not even `T` fits).
    pub fn salience(&self, bpm: f32) -> f32 {
        let t = self.period(bpm);
        let max = self.max_lag() as f32;
        if t.is_nan() || t <= 0.0 || t > max {
            return 0.0;
        }
        let k_max = (max / t).floor() as usize;
        let sum: f32 = (1..=k_max).map(|k| self.at(k as f32 * t)).sum();
        sum / k_max as f32
    }
}

/// Triangular low-pass ([`SMOOTH_KERNEL`]) so pulses at fractional frame
/// offsets still overlap at the beat lag; wraps around when `circular`,
/// else the kernel is truncated at the edges.
fn smooth(x: &[f32], circular: bool) -> Vec<f32> {
    let n = x.len();
    let half = SMOOTH_KERNEL.len() / 2;
    let norm: f32 = SMOOTH_KERNEL.iter().sum();
    (0..n)
        .map(|t| {
            let mut acc = 0.0f32;
            for (j, &w) in SMOOTH_KERNEL.iter().enumerate() {
                let i = t + j;
                let idx = if i < half {
                    if circular {
                        Some(i + n - half)
                    } else {
                        None
                    }
                } else if i - half < n {
                    Some(i - half)
                } else if circular {
                    Some(i - half - n)
                } else {
                    None
                };
                if let Some(v) = idx.and_then(|k| x.get(k)) {
                    acc += w * v;
                }
            }
            acc / norm
        })
        .collect()
}

fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| f64::from(x) * f64::from(y))
        .sum()
}

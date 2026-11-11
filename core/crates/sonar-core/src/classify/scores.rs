//! Heuristic evidence for loop vs one-shot, computed from the DSP
//! [`Features`] of the analysis window (no second STFT) and the file's true
//! duration.
//!
//! - **Periodicity** (rhythm): normalized autocorrelation of the onset
//!   strength envelope at beat-range lags, after removing its local mean —
//!   the tempo-estimation front end of D. Ellis, "Beat tracking by dynamic
//!   programming" (J. New Music Research 36(1), 2007, §3.1: the envelope is
//!   high-pass filtered to be locally zero-mean, then autocorrelated) and
//!   E. Scheirer, "Tempo and beat analysis of acoustic musical signals"
//!   (JASA 103(1), 1998). Scaled by the **pulse strength** (standard
//!   deviation of the detrended envelope), because normalized
//!   autocorrelation is scale-free and would otherwise reward the
//!   numerically tiny frame-to-frame ripple of a steady tone.
//! - **Onset count**: local maxima of the envelope above
//!   [`ONSET_PEAK_REL`] of its maximum (simple peak picking, cf. Bello et
//!   al., "A tutorial on onset detection in music signals", IEEE TSAP
//!   13(5), 2005, §5).
//! - **Tail**: loudest frame of the last [`TAIL_FRAC`] of the window
//!   relative to the loudest frame overall (dB). A loop is sustained to its
//!   end (it must join its own start); a one-shot decays. Unknown when the
//!   file is longer than the analysis window.

use serde::Serialize;

use crate::dsp::Features;

/// Shortest beat-range lag searched (s): 240 BPM.
pub const MIN_PERIOD_S: f32 = 0.25;
/// Longest beat-range lag searched (s): 40 BPM.
pub const MAX_PERIOD_S: f32 = 1.5;
/// Width of the centred moving average removed from the onset envelope (s).
pub const DETREND_S: f32 = 1.0;
/// Onset peaks must reach this fraction of the envelope maximum.
pub const ONSET_PEAK_REL: f32 = 0.3;
/// Minimum spacing of counted onset peaks (s).
pub const ONSET_MIN_GAP_S: f32 = 0.06;
/// Fraction of the analysis window treated as the tail.
pub const TAIL_FRAC: f32 = 0.15;
/// Shortest tail window (s).
pub const TAIL_MIN_S: f32 = 0.25;
/// Floor for frame levels and the tail value (dB).
pub const LEVEL_FLOOR_DB: f32 = -100.0;
/// A window whose loudest STFT frame has at most this power (Σ|X|²,
/// unnormalized, ≈ −117 dB re a full-scale sine frame) is silent; same
/// threshold as [`crate::dsp::aspects::ACTIVE_FLOOR`].
pub const SILENT_FRAME_POWER: f32 = crate::dsp::aspects::ACTIVE_FLOOR;
/// The window counts as truncated when the true duration exceeds it by
/// more than this (s).
pub const TRUNCATION_SLACK_S: f32 = 0.1;

/// Measured evidence for one file. Every field is finite.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct KindScores {
    /// True duration of the file (s).
    pub duration_s: f32,
    /// Length of the analysis window the features cover (s).
    pub analysis_s: f32,
    /// The file is longer than the analysis window (tail unknown).
    pub truncated: bool,
    /// No frame above [`SILENT_FRAME_POWER`]: there is no audio evidence.
    pub silent: bool,
    /// Normalized autocorrelation of the detrended onset envelope at its
    /// best beat-range peak, `0..=1` (0 when there is no peak or the window
    /// holds fewer than two periods).
    pub periodicity: f32,
    /// Lag of that peak (s), 0 if none.
    pub period_s: f32,
    /// Standard deviation of the detrended onset envelope (dB per band).
    pub pulse_db: f32,
    /// Onset peaks counted.
    pub onsets: u32,
    /// Loudest tail frame relative to the loudest frame (dB, `≤ 0`,
    /// floored at [`LEVEL_FLOOR_DB`]); `None` when truncated or silent.
    pub tail_db: Option<f32>,
}

/// Scores the analysis window in `features` for a file of `duration_s`
/// seconds (non-finite or shorter values fall back to the window length).
pub fn kind_scores(features: &Features, duration_s: f64) -> KindScores {
    let spec = &features.spectrogram;
    let fps = finite_or(features.frame_rate(), 0.0);
    let n = spec.n_frames.min(features.onset.len());
    let analysis_s = if fps > 0.0 {
        n.saturating_sub(1) as f32 / fps
    } else {
        0.0
    };
    let dur = duration_s as f32;
    let duration_s = if dur.is_finite() && dur > analysis_s {
        dur
    } else {
        analysis_s
    };
    let truncated = duration_s > analysis_s + TRUNCATION_SLACK_S;

    let levels: Vec<f32> = spec.frames().take(n).map(frame_db).collect();
    let peak = levels.iter().copied().fold(LEVEL_FLOOR_DB, f32::max);
    let silent = n == 0 || peak <= power_db(SILENT_FRAME_POWER);
    let mut s = KindScores {
        duration_s,
        analysis_s,
        truncated,
        silent,
        ..KindScores::default()
    };
    if silent || fps <= 0.0 {
        return s;
    }
    let onset: Vec<f32> = features.onset[..n]
        .iter()
        .map(|&o| finite_or(o, 0.0).max(0.0))
        .collect();
    let detrended = detrend(&onset, odd_frames(DETREND_S, fps));
    s.pulse_db = std_dev(&detrended);
    let (r, lag) = best_period(&detrended, fps);
    s.periodicity = r;
    s.period_s = lag as f32 / fps;
    s.onsets = count_onsets(&onset, ((ONSET_MIN_GAP_S * fps).round() as usize).max(1));
    if !truncated {
        let tail = ((TAIL_FRAC * n as f32) as usize).max((TAIL_MIN_S * fps) as usize);
        // The last frame is half zero padding (centred STFT): skip it.
        let end = n.saturating_sub(1).max(1);
        let start = end.saturating_sub(tail);
        let tail_peak = levels[start..end]
            .iter()
            .copied()
            .fold(LEVEL_FLOOR_DB, f32::max);
        s.tail_db = Some((tail_peak - peak).clamp(LEVEL_FLOOR_DB, 0.0));
    }
    s
}

fn finite_or(x: f32, fallback: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        fallback
    }
}

fn power_db(p: f32) -> f32 {
    (10.0 * finite_or(p, 0.0).max(1e-30).log10()).max(2.0 * LEVEL_FLOOR_DB)
}

fn frame_db(frame: &[f32]) -> f32 {
    power_db(frame.iter().map(|&p| finite_or(p, 0.0)).sum())
}

fn odd_frames(secs: f32, fps: f32) -> usize {
    ((secs * fps).round() as usize) | 1
}

/// `x` minus its centred moving average over `width` frames (shrinking at
/// the edges): a crude high-pass that keeps beat-rate pulses.
fn detrend(x: &[f32], width: usize) -> Vec<f32> {
    let half = width / 2;
    let mut prefix = Vec::with_capacity(x.len() + 1);
    prefix.push(0.0f64);
    let mut acc = 0.0f64;
    for &v in x {
        acc += f64::from(v);
        prefix.push(acc);
    }
    (0..x.len())
        .map(|t| {
            let lo = t.saturating_sub(half);
            let hi = (t + half + 1).min(x.len());
            let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
            (f64::from(x[t]) - mean) as f32
        })
        .collect()
}

fn std_dev(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    let n = x.len() as f64;
    let mean = x.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let var = x
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / n;
    finite_or(var.sqrt() as f32, 0.0)
}

/// Highest local maximum of the biased normalized autocorrelation
/// `r(τ) = Σ x[t]·x[t+τ] / Σ x[t]²` over beat-range lags that fit at least
/// twice in the window. Returns `(r, τ)`, `(0, 0)` if there is none.
fn best_period(x: &[f32], fps: f32) -> (f32, usize) {
    let energy: f64 = x.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
    let lo = ((MIN_PERIOD_S * fps).floor() as usize).max(2);
    let hi = ((MAX_PERIOD_S * fps).ceil() as usize).min(x.len() / 2);
    if energy <= 0.0 || hi < lo + 1 {
        return (0.0, 0);
    }
    let ac = |tau: usize| -> f64 {
        let s: f64 = x[..x.len() - tau]
            .iter()
            .zip(&x[tau..])
            .map(|(&a, &b)| f64::from(a) * f64::from(b))
            .sum();
        s / energy
    };
    let r: Vec<f64> = (lo - 1..=hi + 1)
        .map(|t| if t < x.len() { ac(t) } else { 0.0 })
        .collect();
    let mut best = (0.0f64, 0usize);
    for (i, w) in r.windows(3).enumerate() {
        if w[1] > w[0] && w[1] >= w[2] && w[1] > best.0 {
            best = (w[1], lo + i);
        }
    }
    (finite_or(best.0 as f32, 0.0).clamp(0.0, 1.0), best.1)
}

/// Local maxima of `x` above [`ONSET_PEAK_REL`]·max, at least `gap` frames
/// apart (the earlier peak wins).
fn count_onsets(x: &[f32], gap: usize) -> u32 {
    let max = x.iter().copied().fold(0.0f32, f32::max);
    if max <= 0.0 {
        return 0;
    }
    let thr = ONSET_PEAK_REL * max;
    let mut count = 0u32;
    let mut last: Option<usize> = None;
    for t in 0..x.len() {
        let prev = if t > 0 { x[t - 1] } else { 0.0 };
        let next = x.get(t + 1).copied().unwrap_or(0.0);
        let is_peak = x[t] >= thr && x[t] > prev && x[t] >= next;
        if is_peak && last.is_none_or(|l| t - l >= gap) {
            count += 1;
            last = Some(t);
        }
    }
    count
}

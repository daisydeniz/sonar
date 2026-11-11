//! Tempo (BPM) of loops (`docs/ARCHITECTURE.md` §4, ADR-021).
//!
//! [`estimate_tempo`] works on the DSP [`Features`] of the analysis window
//! (no second STFT) plus the file's **true** duration:
//!
//! 1. **Envelope**: μ-law compressed band-power flux ([`accent`]; Klapuri
//!    et al. 2006), so kick/snare accents dominate quiet subdivisions.
//!    When the window holds the whole file (a loop) everything wraps.
//! 2. **Tempo family**: comb salience of every tempo on a
//!    [`GRID_STEP_BPM`] grid over [`MIN_BPM`]..=[`MAX_BPM`] from the
//!    autocorrelation of the envelope ([`salience`]; Ellis 2007, Scheirer
//!    1998, Grosche & Müller 2011); local maxima, parabolic refinement,
//!    best weighted peak wins.
//! 3. **Octave correction**: the ×¼…×4 alternatives of that peak are
//!    re-scored by **clock fit** ([`clock`]; Povel & Essens 1985) — the
//!    autocorrelation alone ranks the half-bar of a backbeat above the beat.
//! 4. **Weights** (steps 2 and 3): a log-Gaussian tempo prior around
//!    [`PRIOR_CENTER_BPM`] ([`PRIOR_SIGMA_OCT`] octaves, Ellis 2007 §3.1
//!    weights the autocorrelation the same way); a **loop-length prior**
//!    when the window holds the whole file — a loop is a whole number of
//!    beats, usually whole 4/4 bars, so `b·duration/60 ≈ n` with `n` a power
//!    of two times 4 ([`bar_weight`]; off-grid tempi get
//!    [`OFF_GRID_WEIGHT`]); an optional filename BPM ([`NAME_WEIGHT`] within
//!    [`NAME_TOL_REL`]).
//! 5. **Refinement**: a winner that fits the loop length is snapped to
//!    exactly `n·60/duration` (sub-integer precision for free); otherwise
//!    the parabolic estimate stands.
//!
//! The filename prior is optional (the eval hides names): it breaks octave
//! ties, and supplies the value (source [`TempoSource::Filename`]) when
//! detection is missing or weaker than [`WEAK_CONFIDENCE`]. [`loop_tempo`]
//! gates on the loop/one-shot decision: one-shots never get a tempo. Pure,
//! deterministic, never panics, finite outputs.

pub mod accent;
pub mod clock;
pub mod salience;

#[cfg(test)]
mod tests;

use serde::Serialize;

pub use salience::Periodicity;

use crate::classify::scores::TRUNCATION_SLACK_S;
use crate::classify::{Kind, KindResult, KindSource};
use crate::dsp::Features;
use crate::text::BpmPrior;

/// Version of the tempo definitions (salience, priors, weights); bump when
/// any changes so stored BPMs are recomputed.
pub const TEMPO_VERSION: u32 = 1;
/// Slowest tempo searched.
pub const MIN_BPM: f32 = 40.0;
/// Fastest tempo searched.
pub const MAX_BPM: f32 = 250.0;
/// Spacing of the salience grid (BPM).
pub const GRID_STEP_BPM: f32 = 0.1;
/// Centre of the log-Gaussian tempo prior (BPM).
pub const PRIOR_CENTER_BPM: f32 = 110.0;
/// Width of the tempo prior (octaves).
pub const PRIOR_SIGMA_OCT: f32 = 1.5;
/// Candidates need at least this comb salience.
pub const MIN_SALIENCE: f32 = 0.05;
/// Salience at which detection strength saturates (confidence factor 1).
pub const SALIENCE_REF: f32 = 0.5;
/// A tempo fits the loop length when `b·duration/60` is within this many
/// beats of a whole number …
pub const SNAP_MAX_BEATS: f32 = 0.2;
/// … and snapping moves it by at most this fraction.
pub const SNAP_MAX_REL: f32 = 0.03;
/// Loop-length weight of a tempo that fits no whole number of beats.
pub const OFF_GRID_WEIGHT: f32 = 0.3;
/// Weight of candidates within [`NAME_TOL_REL`] of the filename BPM.
pub const NAME_WEIGHT: f32 = 3.0;
/// Relative tolerance for "agrees with the filename BPM".
pub const NAME_TOL_REL: f32 = 0.03;
/// Detection below this confidence yields to a filename BPM.
pub const WEAK_CONFIDENCE: f32 = 0.2;
/// Confidence of a tempo taken from an explicit (`120bpm`) filename BPM.
pub const NAME_CONF_EXPLICIT: f32 = 0.6;
/// Confidence of a tempo taken from a bare-number filename BPM.
pub const NAME_CONF_BARE: f32 = 0.4;
/// Comb peaks closer than this (relative) to an octave target stand for it.
const SAME_PEAK_REL: f32 = 0.03;
/// Metrical alternatives of the comb winner compared by clock fit.
const OCTAVES: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// Where a [`Tempo`] came from (`analysis.bpm_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TempoSource {
    /// Measured from the audio (a filename BPM may have broken an octave
    /// tie or agreed).
    Detected,
    /// The filename BPM: detection failed or was weak.
    Filename,
}

impl TempoSource {
    /// Value stored in `analysis.bpm_source`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Filename => "filename",
        }
    }
}

/// Tempo of one file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Tempo {
    /// Beats per minute (finite, > 0).
    pub bpm: f32,
    /// `0..=1`: detection strength × margin over the best other octave /
    /// candidate, combined with an agreeing filename BPM.
    pub confidence: f32,
    /// What decided the value.
    pub source: TempoSource,
    /// Whole beats in the file when the tempo was snapped to the loop
    /// length.
    pub beats: Option<u32>,
}

/// Tempo of `features`' analysis window for a file whose true duration is
/// `duration_s` (the window may be capped at 30 s), with an optional
/// filename BPM. `None` when there is no periodicity and no prior.
pub fn estimate_tempo(
    features: &Features,
    duration_s: f64,
    prior: Option<BpmPrior>,
) -> Option<Tempo> {
    let detected = detect(features, duration_s, prior.map(|p| p.bpm));
    let Some(p) = prior.filter(|p| p.bpm.is_finite() && p.bpm > 0.0) else {
        return detected;
    };
    let name_conf = if p.explicit {
        NAME_CONF_EXPLICIT
    } else {
        NAME_CONF_BARE
    };
    match detected {
        Some(t) if t.confidence >= WEAK_CONFIDENCE => {
            let agrees = (t.bpm - p.bpm).abs() <= NAME_TOL_REL * p.bpm;
            let confidence = if agrees {
                1.0 - (1.0 - t.confidence) * (1.0 - name_conf)
            } else {
                t.confidence
            };
            Some(Tempo { confidence, ..t })
        }
        _ => Some(Tempo {
            bpm: p.bpm,
            confidence: name_conf,
            source: TempoSource::Filename,
            beats: None,
        }),
    }
}

/// [`estimate_tempo`] gated on the loop/one-shot decision: `None` unless
/// `kind` is a [`Kind::Loop`] (one-shots and silent fallbacks never get a
/// tempo).
pub fn loop_tempo(
    kind: &KindResult,
    features: &Features,
    duration_s: f64,
    prior: Option<BpmPrior>,
) -> Option<Tempo> {
    if kind.kind != Kind::Loop || kind.source == KindSource::Fallback {
        return None;
    }
    estimate_tempo(features, duration_s, prior)
}

/// One salience peak.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    bpm: f32,
    salience: f32,
    score: f32,
}

/// Audio-only estimate (the prior BPM, if any, only weights candidates).
fn detect(features: &Features, duration_s: f64, prior_bpm: Option<f32>) -> Option<Tempo> {
    let fps = features.frame_rate();
    let n_frames = features.spectrogram.n_frames;
    if !(fps.is_finite() && fps > 0.0) || n_frames < 2 {
        return None;
    }
    let window_s = (n_frames - 1) as f32 / fps;
    // Unknown (non-finite, non-positive) durations: the window is the file.
    let dur = Some(duration_s as f32)
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(window_s);
    let whole = dur <= window_s + TRUNCATION_SLACK_S;
    let loop_s = dur.max(window_s);
    let mut env = Vec::new();
    accent::accent_envelope(&features.log_mel, whole, &mut env);
    // Centred frames: for a whole loop the last frame sits on the loop end,
    // i.e. on frame 0 again, so one cycle is frames 0..n−1 spanning
    // `loop_s·fps` frames.
    let n = n_frames.min(env.len()) - usize::from(whole);
    let span = if whole {
        (loop_s * fps).max(n as f32)
    } else {
        n as f32
    };
    let per = Periodicity::new(&env[..n], fps, whole)?;
    let weight = |bpm: f32| -> f32 {
        let mut w = tempo_prior(bpm);
        if whole {
            w *= loop_fit(bpm, loop_s).map_or(OFF_GRID_WEIGHT, |(n, _)| bar_weight(n));
        }
        if prior_bpm.is_some_and(|p| (bpm - p).abs() <= NAME_TOL_REL * p) {
            w *= NAME_WEIGHT;
        }
        w
    };

    // 1. Tempo family: the comb peak with the best weighted salience.
    let steps = ((MAX_BPM - MIN_BPM) / GRID_STEP_BPM).round() as usize;
    let grid: Vec<f32> = (0..=steps)
        .map(|i| per.salience(MIN_BPM + i as f32 * GRID_STEP_BPM))
        .collect();
    let mut cands: Vec<Candidate> = Vec::new();
    for (i, w) in grid.windows(3).enumerate() {
        if !(w[1] > w[0] && w[1] >= w[2] && w[1] >= MIN_SALIENCE) {
            continue;
        }
        let denom = w[0] - 2.0 * w[1] + w[2];
        let off = if denom < 0.0 {
            (0.5 * (w[0] - w[2]) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let bpm = MIN_BPM + (i as f32 + 1.0 + off) * GRID_STEP_BPM;
        let salience = per.salience(bpm).max(w[1]);
        cands.push(Candidate {
            bpm,
            salience,
            score: salience * weight(bpm),
        });
    }
    let family = *cands.iter().max_by(|a, b| a.score.total_cmp(&b.score))?;

    // 2. Octave: clock fit of the ×½, ×1, ×2 alternatives.
    let events = clock::events(&env[..n], whole);
    let mut ticks = Vec::new();
    let mut alts: Vec<Candidate> = Vec::with_capacity(OCTAVES.len());
    for m in OCTAVES {
        let target = family.bpm * m;
        if !(MIN_BPM..=MAX_BPM).contains(&target) {
            continue;
        }
        let near = cands
            .iter()
            .filter(|c| (c.bpm - target).abs() <= SAME_PEAK_REL * target)
            .max_by(|a, b| a.salience.total_cmp(&b.salience));
        let (bpm, salience) = near.map_or((target, per.salience(target)), |c| (c.bpm, c.salience));
        let fit = clock::fit(&events, per.period(bpm), span, whole, &mut ticks);
        alts.push(Candidate {
            bpm,
            salience,
            score: fit * weight(bpm),
        });
    }
    let best = *alts.iter().max_by(|a, b| a.score.total_cmp(&b.score))?;
    let runner_up = alts
        .iter()
        .filter(|c| c.bpm != best.bpm)
        .map(|c| c.score)
        .fold(0.0f32, f32::max);
    let margin = if best.score > 0.0 {
        best.score / (best.score + runner_up)
    } else {
        0.0
    };
    let strength = (best.salience / SALIENCE_REF).clamp(0.0, 1.0);
    let confidence = (strength * margin).clamp(0.0, 1.0);
    let (bpm, beats) = match whole.then(|| loop_fit(best.bpm, loop_s)).flatten() {
        Some((n, snapped)) => (snapped, Some(n)),
        None => (best.bpm, None),
    };
    (bpm.is_finite() && bpm > 0.0 && confidence.is_finite()).then_some(Tempo {
        bpm,
        confidence,
        source: TempoSource::Detected,
        beats,
    })
}

/// Log-Gaussian tempo prior (1 at [`PRIOR_CENTER_BPM`]).
pub fn tempo_prior(bpm: f32) -> f32 {
    let oct = (bpm / PRIOR_CENTER_BPM).log2() / PRIOR_SIGMA_OCT;
    (-0.5 * oct * oct).exp()
}

/// Whole beats `n` and the exact tempo `n·60/loop_s` when `bpm` fits a
/// loop of `loop_s` seconds (see [`SNAP_MAX_BEATS`], [`SNAP_MAX_REL`]).
pub fn loop_fit(bpm: f32, loop_s: f32) -> Option<(u32, f32)> {
    if !(loop_s > 0.0 && bpm > 0.0) {
        return None;
    }
    let beats = bpm * loop_s / 60.0;
    let n = beats.round();
    if n < 1.0 || n > u32::MAX as f32 {
        return None;
    }
    let snapped = n * 60.0 / loop_s;
    ((beats - n).abs() <= SNAP_MAX_BEATS && (snapped - bpm).abs() <= SNAP_MAX_REL * bpm)
        .then_some((n as u32, snapped))
}

/// Loop-length weight of a whole number of beats: whole 4/4 bars in a
/// power-of-two count (1, 2, 4, 8 … bars) are the norm for sample loops,
/// other whole 4/4 bars and whole 3/4 bars are common, anything else is
/// rare.
pub fn bar_weight(beats: u32) -> f32 {
    if beats.is_multiple_of(4) && (beats / 4).is_power_of_two() {
        1.0
    } else if beats.is_multiple_of(4) {
        0.85
    } else if beats.is_multiple_of(3) {
        0.7
    } else {
        0.4
    }
}

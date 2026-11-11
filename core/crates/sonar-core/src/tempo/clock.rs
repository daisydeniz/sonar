//! Octave correction by **clock fit**: how well a beat grid of a candidate
//! period explains the accent events of the file.
//!
//! After Povel & Essens, "Perception of temporal patterns" (Music
//! Perception 2(4), 1985): the induced clock is the one whose ticks
//! coincide with accented events and seldom fall on silence. For a period
//! `P` and the best phase `φ`, ticks at `φ + k·P`:
//! - **recall** = accent mass of the events within [`tolerance`] of a tick
//!   over the accent mass of all events (a too-slow grid misses events);
//! - **precision** = mean over ticks of the strongest event on the tick,
//!   relative to the strongest event in the file (a too-fast grid has
//!   empty ticks);
//! - **fit** = recall × precision.
//!
//! Applied only to the metrical alternatives (×¼ … ×4) of the comb
//! winner, where autocorrelation alone cannot tell a backbeat at the beat
//! level from its half-bar periodicity (`docs/DECISIONS.md` ADR-021).

/// Events must reach this fraction of the strongest accent peak.
pub const EVENT_REL: f32 = 0.1;
/// Events at or above this fraction of the strongest are "strong"; their
/// median weight is a full tick.
pub const STRONG_REL: f32 = 0.3;
/// Phase search step (frames).
pub const PHASE_STEP: f32 = 0.5;
/// Grids with more ticks than this are rejected (fit 0).
pub const MAX_TICKS: f32 = 10_000.0;

/// Accent events: `(time in frames, weight)` of the local maxima of
/// `env` above [`EVENT_REL`] of its maximum, with parabolic sub-frame
/// timing. `circular`: the first and last frames are neighbours.
pub fn events(env: &[f32], circular: bool) -> Vec<(f32, f32)> {
    let max = env.iter().copied().fold(0.0f32, f32::max);
    let n = env.len();
    if max.is_nan() || max <= 0.0 || n < 3 {
        return Vec::new();
    }
    let thr = EVENT_REL * max;
    let mut out = Vec::new();
    let range = if circular { 0..n } else { 1..n - 1 };
    for t in range {
        let (a, b, c) = (env[(t + n - 1) % n], env[t], env[(t + 1) % n]);
        if b >= thr && b > a && b >= c {
            let denom = a - 2.0 * b + c;
            let off = if denom < 0.0 {
                (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            out.push((t as f32 + off, b));
        }
    }
    out
}

/// Median weight of the strong events (≥ [`STRONG_REL`] of the strongest):
/// the weight of a "full" tick, robust to one outsized accent.
pub fn strong_median(events: &[(f32, f32)]) -> f32 {
    let max = events.iter().map(|e| e.1).fold(0.0f32, f32::max);
    let mut strong: Vec<f32> = events
        .iter()
        .map(|e| e.1)
        .filter(|&w| w >= STRONG_REL * max)
        .collect();
    if strong.is_empty() {
        return 0.0;
    }
    strong.sort_by(f32::total_cmp);
    strong[strong.len() / 2]
}

/// Tick tolerance (frames) for a grid of period `p` frames.
pub fn tolerance(p: f32) -> f32 {
    (0.15 * p).clamp(1.0, 2.5)
}

/// Clock fit (`0..=1`) of a grid with period `p` frames over `span` frames
/// of `events` (see the module docs). `circular`: the span is one loop
/// cycle, so ticks wrap around. `ticks` is scratch space.
pub fn fit(events: &[(f32, f32)], p: f32, span: f32, circular: bool, ticks: &mut Vec<f32>) -> f32 {
    let total: f32 = events.iter().map(|e| e.1).sum();
    let reference = strong_median(events);
    // Finite span and period (ticks stay below MAX_TICKS); NaN fails.
    let usable = p >= 1.0 && span >= 1.0 && span / p <= MAX_TICKS && total > 0.0 && reference > 0.0;
    if !usable {
        return 0.0;
    }
    let tol = tolerance(p);
    let mut best = 0.0f32;
    let mut phase = 0.0f32;
    while phase < p {
        // Ticks in the window; a circular grid that does not divide the
        // window evenly still gets ⌈span/p⌉ ticks (the last may straddle
        // the wrap).
        let n_ticks = ((span - phase) / p).ceil().max(1.0) as usize;
        ticks.clear();
        ticks.resize(n_ticks, 0.0);
        let mut on = 0.0f32;
        for &(t, w) in events {
            let k = ((t - phase) / p).round();
            let mut d = (t - (phase + k * p)).abs();
            let mut idx = k;
            if circular {
                // Distance to the first tick across the wrap.
                let wrap = (span + phase - t).abs();
                if wrap < d {
                    d = wrap;
                    idx = 0.0;
                }
            }
            if d <= tol && idx >= 0.0 && (idx as usize) < n_ticks {
                on += w;
                let slot = &mut ticks[idx as usize];
                *slot = slot.max(w);
            }
        }
        let recall = on / total;
        let filled: f32 = ticks.iter().map(|&w| (w / reference).min(1.0)).sum();
        let precision = filled / n_ticks as f32;
        best = best.max(recall * precision);
        phase += PHASE_STEP;
    }
    if best.is_finite() {
        best.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

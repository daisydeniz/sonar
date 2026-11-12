//! The DSP side of the category classifier: a Gaussian naive-Bayes score
//! (R. Duda, P. Hart, D. Stork, "Pattern Classification", 2nd ed. 2001,
//! §2.6 / §3.2: class-conditional normal densities, features treated as
//! independent) over the [`DspCues`], plus a loop/one-shot prior.
//!
//! For category `c`:
//!
//! `L_c = ln(p_loop·π_c + (1 − p_loop)·(1 − π_c))`
//! `    + Σ_spectral ln N(x; μ_c, σ_c)`
//! `    + tonal_w · ln N(f0; μ_c, σ_c)`
//! `    + (1 − p_loop) · Σ_envelope ln N(x; μ_c, σ_c)`
//!
//! with `π_c` = P(loop | c), `tonal_w = min(1, tonal / TONAL_FULL)` (an
//! unpitched file's f0 says nothing) and the envelope cues (attack, decay,
//! effective duration, true duration) counted for one-shots only (a loop's
//! envelope is its pattern, not its source). Means and spreads
//! ([`PROFILES`]) are the per-class mean and standard deviation of each cue
//! on the training half of the local silver set (`docs/DECISIONS.md`
//! ADR-028: 22 of 47 genre folders, ≥ 30 files per class; π_c = share of
//! loops; envelope cues over one-shots; f0 over pitched files), with
//! hand-set rows (marked) for classes the silver set lacks or has too few
//! of. Each σ has a floor (0.3 oct, 3 dB, 0.1, 0.2 decades, 0.5 oct of
//! duration) so no single cue can dominate. The naive independence
//! assumption over-counts correlated cues; the fusion weights and the
//! temperature ([`super::W_DSP`], [`super::T_DSP`]) absorb that.

use super::cues::DspCues;
use super::taxonomy::{Category, N_CATEGORIES};

/// Tonality (`voiced × confidence`) at which the f0 term counts fully.
pub const TONAL_FULL: f32 = 0.5;

/// Spectral / timbral cues (always counted).
pub const SPECTRAL_CUES: usize = 6;
/// Envelope cues (one-shots only).
pub const ENVELOPE_CUES: usize = 4;

/// Per-category expectations: `(μ, σ)` per cue.
#[derive(Debug, Clone, Copy)]
pub struct Profile {
    /// Category.
    pub category: Category,
    /// P(loop | category).
    pub p_loop: f32,
    /// centroid, rolloff, flatness, low, high, tonal.
    pub spectral: [(f32, f32); SPECTRAL_CUES],
    /// f0 (octaves from A4), gated by tonality.
    pub f0: (f32, f32),
    /// attack, decay, effective (log10 s), log2 duration.
    pub envelope: [(f32, f32); ENVELOPE_CUES],
}

/// The spectral cue values of `c` in [`Profile::spectral`] order.
fn spectral(c: &DspCues) -> [f32; SPECTRAL_CUES] {
    [c.centroid, c.rolloff, c.flatness, c.low, c.high, c.tonal]
}

/// The envelope cue values of `c` in [`Profile::envelope`] order.
fn envelope(c: &DspCues) -> [f32; ENVELOPE_CUES] {
    [c.attack, c.decay, c.effective, c.log2_dur]
}

/// `ln N(x; μ, σ)` without the constant `−½ ln 2π`.
fn log_normal(x: f32, (mu, sigma): (f32, f32)) -> f32 {
    let s = sigma.max(1e-3);
    let z = (x - mu) / s;
    -0.5 * z * z - s.ln()
}

/// Naive-Bayes log score per category (see the module docs), shifted so
/// the best is 0. All zero for a silent file or missing vectors.
pub fn dsp_scores(cues: &DspCues) -> [f32; N_CATEGORIES] {
    let mut out = [0.0f32; N_CATEGORIES];
    if cues.silent || cues.missing {
        return out;
    }
    let sx = spectral(cues);
    let ex = envelope(cues);
    let p_loop = cues.p_loop.clamp(0.0, 1.0);
    let tonal_w = (cues.tonal / TONAL_FULL).clamp(0.0, 1.0);
    for prof in &PROFILES {
        let prior = p_loop * prof.p_loop + (1.0 - p_loop) * (1.0 - prof.p_loop);
        let mut l = prior.max(1e-4).ln();
        l += sx
            .iter()
            .zip(&prof.spectral)
            .map(|(&x, &p)| log_normal(x, p))
            .sum::<f32>();
        l += tonal_w * log_normal(cues.f0, prof.f0);
        l += (1.0 - p_loop)
            * ex.iter()
                .zip(&prof.envelope)
                .map(|(&x, &p)| log_normal(x, p))
                .sum::<f32>();
        out[prof.category.index()] = if l.is_finite() { l } else { f32::MIN / 4.0 };
    }
    let max = out.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    for v in &mut out {
        *v -= max;
    }
    out
}

/// Class profiles, in [`Category`] order. Columns: P(loop); centroid
/// (oct re 1 kHz), roll-off (oct re 1 kHz), flatness (dB), low and high
/// band level (dB), tonality; f0 (oct re A4); attack, decay, effective
/// duration (log10 s), duration (log2 s). Rows marked `hand` (and the f0 of
/// rows marked `f0 hand`) are set by hand from related fitted classes.
#[rustfmt::skip]
pub const PROFILES: [Profile; N_CATEGORIES] = [
    p(Category::Kick, 0.08, [(-2.09, 1.27), (-3.57, 0.55), (-46.09, 7.91), (13.92, 3.90), (-6.04, 3.00), (0.35, 0.23)], (-3.16, 0.50), [(-1.90, 0.60), (-0.78, 0.20), (-0.85, 0.21), (-1.16, 0.63)]),
    p(Category::Snare, 0.05, [(1.82, 0.37), (1.55, 0.93), (-11.11, 4.65), (0.57, 6.08), (-4.47, 3.96), (0.03, 0.11)], (-0.39, 1.02), [(-2.63, 0.56), (-0.94, 0.21), (-1.25, 0.20), (-0.95, 0.98)]),
    // f0 hand
    p(Category::Clap, 0.07, [(1.80, 0.32), (1.91, 0.59), (-10.05, 4.21), (-8.16, 7.01), (-5.53, 4.30), (0.00, 0.10)], (-1.00, 1.50), [(-2.46, 0.66), (-1.01, 0.22), (-1.22, 0.20), (-1.15, 0.98)]),
    // f0 hand
    p(Category::Hat, 0.05, [(2.63, 0.30), (2.99, 0.40), (-10.73, 4.15), (-18.27, 10.81), (13.27, 7.82), (0.00, 0.10)], (1.00, 1.50), [(-2.79, 0.43), (-1.02, 0.37), (-1.27, 0.38), (-1.35, 1.05)]),
    // f0 hand
    p(Category::Cymbal, 0.05, [(2.43, 0.34), (2.80, 0.44), (-12.87, 4.40), (-18.30, 7.15), (8.44, 7.64), (0.00, 0.10)], (1.00, 1.50), [(-2.34, 0.77), (-0.19, 0.25), (-0.58, 0.38), (1.04, 0.93)]),
    p(Category::Tom, 0.05, [(-0.02, 1.04), (-2.36, 0.73), (-33.35, 9.41), (14.43, 3.77), (-8.30, 3.39), (0.56, 0.30)], (-1.77, 0.69), [(-2.49, 0.60), (-0.71, 0.24), (-0.99, 0.29), (-0.56, 0.67)]),
    p(Category::Perc, 0.05, [(0.99, 1.15), (0.12, 1.89), (-22.26, 12.07), (6.01, 11.04), (-7.38, 7.31), (0.16, 0.29)], (-0.84, 1.42), [(-2.49, 0.67), (-1.03, 0.32), (-1.31, 0.33), (-1.19, 1.04)]),
    // f0 hand
    p(Category::Drums, 0.83, [(2.18, 0.80), (1.93, 1.94), (-12.00, 6.67), (-7.76, 12.52), (5.57, 9.66), (0.05, 0.12)], (-1.00, 1.50), [(-1.20, 1.09), (-0.54, 0.34), (-0.55, 0.26), (0.71, 0.50)]),
    p(Category::Bass, 0.15, [(-1.65, 1.75), (-3.03, 1.26), (-44.87, 12.98), (22.61, 7.97), (-12.22, 5.75), (0.77, 0.25)], (-3.13, 0.51), [(-1.74, 0.79), (0.01, 0.56), (-0.08, 0.62), (0.73, 1.64)]),
    // hand
    p(Category::Lead, 0.50, [(0.80, 1.20), (0.50, 1.60), (-30.00, 12.00), (6.00, 10.00), (-8.00, 7.00), (0.55, 0.35)], (-0.50, 1.20), [(-1.80, 0.90), (-0.20, 0.60), (-0.30, 0.60), (1.00, 1.60)]),
    // hand
    p(Category::Pad, 0.40, [(-0.30, 1.20), (-1.00, 1.50), (-35.00, 12.00), (10.00, 8.00), (-12.00, 6.00), (0.50, 0.35)], (-1.00, 1.30), [(-0.80, 0.80), (0.30, 0.50), (0.30, 0.50), (2.00, 1.50)]),
    p(Category::Keys, 0.51, [(0.09, 1.26), (-0.60, 1.17), (-36.51, 14.88), (14.31, 5.80), (-10.53, 3.82), (0.50, 0.43)], (-0.92, 1.67), [(-2.33, 0.63), (-0.03, 0.44), (-0.53, 0.48), (2.10, 1.46)]),
    // hand
    p(Category::Pluck, 0.30, [(0.30, 1.20), (-0.50, 1.50), (-35.00, 12.00), (8.00, 9.00), (-9.00, 6.00), (0.55, 0.35)], (-0.80, 1.30), [(-2.40, 0.50), (-0.60, 0.40), (-0.90, 0.40), (0.00, 1.50)]),
    p(Category::Synth, 0.16, [(0.64, 1.21), (-0.11, 1.60), (-29.34, 13.21), (9.46, 11.21), (-9.98, 7.35), (0.48, 0.36)], (-1.19, 1.35), [(-2.02, 0.94), (-0.37, 0.58), (-0.48, 0.67), (0.17, 1.77)]),
    // hand
    p(Category::Vocal, 0.40, [(0.30, 1.00), (-0.30, 1.50), (-30.00, 10.00), (5.00, 8.00), (-10.00, 6.00), (0.55, 0.30)], (-0.70, 0.80), [(-1.80, 0.60), (-0.30, 0.60), (-0.50, 0.60), (0.80, 1.50)]),
    p(Category::Fx, 0.09, [(-0.02, 1.93), (-0.91, 2.56), (-30.36, 16.57), (5.63, 13.58), (-6.99, 6.77), (0.11, 0.23)], (-1.98, 1.82), [(-0.83, 1.13), (-0.05, 0.49), (-0.09, 0.55), (2.61, 1.13)]),
    // hand
    p(Category::Texture, 0.50, [(0.30, 1.50), (0.00, 2.00), (-25.00, 14.00), (8.00, 10.00), (-6.00, 8.00), (0.20, 0.30)], (-1.00, 1.60), [(-0.60, 1.00), (0.40, 0.60), (0.30, 0.60), (2.50, 1.30)]),
];

/// Short constructor for [`PROFILES`] rows.
const fn p(
    category: Category,
    p_loop: f32,
    spectral: [(f32, f32); SPECTRAL_CUES],
    f0: (f32, f32),
    envelope: [(f32, f32); ENVELOPE_CUES],
) -> Profile {
    Profile {
        category,
        p_loop,
        spectral,
        f0,
        envelope,
    }
}

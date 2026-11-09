//! K-weighting filter of ITU-R BS.1770-4 for any sample rate.
//!
//! BS.1770-4 (Annex 1, Tables 1 and 2) publishes the two biquads only for
//! 48 kHz: a high-shelf "pre-filter" (+4 dB above ~1.7 kHz, models the
//! acoustic effect of the head) followed by the "RLB" high-pass (~38 Hz).
//! For other rates the analogue prototypes are recovered from the 48 kHz
//! coefficients and re-discretised with the bilinear transform (RBJ
//! "Audio EQ Cookbook" shelf/high-pass forms with pre-warping
//! `K = tan(π·f0/fs)`). The prototype parameters below (`f0`, gain, `Q`,
//! shelf exponent) are the published derivation by Brecht De Man ("Filter
//! design for ITU-R BS.1770", used by pyloudnorm, MIT) and libebur128
//! (MIT); they reproduce the BS.1770 48 kHz table to ~1e-8. Written from
//! those formulas, not ported from any implementation.

use std::f64::consts::PI;

/// Normalised biquad `H(z) = (b0 + b1 z⁻¹ + b2 z⁻²) / (1 + a1 z⁻¹ + a2 z⁻²)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    /// Feed-forward coefficient of `z⁰`.
    pub b0: f64,
    /// Feed-forward coefficient of `z⁻¹`.
    pub b1: f64,
    /// Feed-forward coefficient of `z⁻²`.
    pub b2: f64,
    /// Feedback coefficient of `z⁻¹` (sign as in the denominator above).
    pub a1: f64,
    /// Feedback coefficient of `z⁻²`.
    pub a2: f64,
}

impl Biquad {
    /// Magnitude response `|H(e^{jω})|` at `freq` Hz for sample rate `rate`.
    pub fn magnitude(&self, freq: f64, rate: f64) -> f64 {
        let w = 2.0 * PI * freq / rate;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        (num_re.hypot(num_im)) / (den_re.hypot(den_im))
    }
}

/// Pre-filter (high shelf) prototype: centre frequency (Hz).
const SHELF_F0: f64 = 1_681.974_450_955_533;
/// Pre-filter shelf gain (dB).
const SHELF_GAIN_DB: f64 = 3.999_843_853_973_347;
/// Pre-filter quality factor.
const SHELF_Q: f64 = 0.707_175_236_955_419_6;
/// Exponent relating the shelf's band gain `Vb` to its high gain `Vh`.
const SHELF_VB_EXP: f64 = 0.499_666_774_154_541_6;
/// RLB high-pass prototype: corner frequency (Hz).
const HIGHPASS_F0: f64 = 38.135_470_876_024_44;
/// RLB high-pass quality factor.
const HIGHPASS_Q: f64 = 0.500_327_037_323_877_3;

/// The two K-weighting stages `[pre-filter, RLB high-pass]` at `rate` Hz.
pub fn k_weighting(rate: f64) -> [Biquad; 2] {
    let k = (PI * SHELF_F0 / rate).tan();
    let vh = 10f64.powf(SHELF_GAIN_DB / 20.0);
    let vb = vh.powf(SHELF_VB_EXP);
    let a0 = 1.0 + k / SHELF_Q + k * k;
    let shelf = Biquad {
        b0: (vh + vb * k / SHELF_Q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / SHELF_Q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / SHELF_Q + k * k) / a0,
    };
    let k = (PI * HIGHPASS_F0 / rate).tan();
    let a0 = 1.0 + k / HIGHPASS_Q + k * k;
    // Numerator 1, −2, 1 un-normalised at every rate, exactly as the
    // BS.1770 48 kHz table, libebur128, pyloudnorm and ffmpeg's ebur128
    // (so they can serve as references). Its pass-band gain is `a0`, which
    // depends on the rate: +0.043 dB at 48 kHz (part of the standard's
    // −0.691 calibration), +0.094 dB at 22.05 kHz, +0.022 dB at 96 kHz —
    // i.e. ≤ +0.05 LU against 48 kHz, inside the ±0.1 LU of Tech 3341.
    let highpass = Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / HIGHPASS_Q + k * k) / a0,
    };
    [shelf, highpass]
}

/// Gain of the full K-weighting at `freq` Hz (dB) for sample rate `rate`.
/// About +0.69 dB at 1 kHz, which the −0.691 term of BS.1770 cancels.
pub fn k_weighting_gain_db(freq: f64, rate: f64) -> f64 {
    let [a, b] = k_weighting(rate);
    20.0 * (a.magnitude(freq, rate) * b.magnitude(freq, rate)).log10()
}

/// Filter memory of the cascade: `[x₁, x₂, y₁, y₂, z₁, z₂]` (input,
/// pre-filter output = high-pass input, high-pass output; `₁` = previous
/// sample).
pub(super) type FilterState = [f64; 6];

/// Runs `x` through both stages and returns `Σ z²` of the K-weighted
/// output; `st` carries the memory across calls. Direct form I in `f64`:
/// only `a1·y[n−1]` is on the recursive path, so the loop is about twice
/// as fast as transposed form II, and it has no internal overflow issues
/// in floating point. Non-finite input samples are treated as 0 so one
/// bad float sample cannot poison the filter state.
#[inline]
pub(super) fn filter_energy(x: &[f32], st: &mut FilterState, f: &[Biquad; 2]) -> f64 {
    let [p, h] = f;
    let [mut x1, mut x2, mut y1, mut y2, mut z1, mut z2] = *st;
    let mut acc = 0.0;
    for &s in x {
        let s = if s.is_finite() { f64::from(s) } else { 0.0 };
        let y = p.b0 * s + p.b1 * x1 + p.b2 * x2 - p.a2 * y2 - p.a1 * y1;
        let z = h.b0 * y + h.b1 * y1 + h.b2 * y2 - h.a2 * z2 - h.a1 * z1;
        (x2, x1, y2, y1, z2, z1) = (x1, s, y1, y, z1, z);
        acc += z * z;
    }
    *st = [x1, x2, y1, y2, z1, z2];
    acc
}

/// [`filter_energy`] for two channels at once (e.g. L/R): the two
/// independent recursions interleave and hide each other's latency, ~1.8×
/// the throughput of two single-channel passes. Bit-identical to calling
/// [`filter_energy`] on each. Uses the shorter of `a`/`b`.
#[inline]
pub(super) fn filter_energy_pair(
    a: &[f32],
    b: &[f32],
    sa: &mut FilterState,
    sb: &mut FilterState,
    f: &[Biquad; 2],
) -> (f64, f64) {
    let [p, h] = f;
    let [mut ax1, mut ax2, mut ay1, mut ay2, mut az1, mut az2] = *sa;
    let [mut bx1, mut bx2, mut by1, mut by2, mut bz1, mut bz2] = *sb;
    let (mut acc_a, mut acc_b) = (0.0, 0.0);
    for (&xa, &xb) in a.iter().zip(b) {
        let xa = if xa.is_finite() { f64::from(xa) } else { 0.0 };
        let xb = if xb.is_finite() { f64::from(xb) } else { 0.0 };
        let ya = p.b0 * xa + p.b1 * ax1 + p.b2 * ax2 - p.a2 * ay2 - p.a1 * ay1;
        let yb = p.b0 * xb + p.b1 * bx1 + p.b2 * bx2 - p.a2 * by2 - p.a1 * by1;
        let za = h.b0 * ya + h.b1 * ay1 + h.b2 * ay2 - h.a2 * az2 - h.a1 * az1;
        let zb = h.b0 * yb + h.b1 * by1 + h.b2 * by2 - h.a2 * bz2 - h.a1 * bz1;
        (ax2, ax1, ay2, ay1, az2, az1) = (ax1, xa, ay1, ya, az1, za);
        (bx2, bx1, by2, by1, bz2, bz1) = (bx1, xb, by1, yb, bz1, zb);
        acc_a += za * za;
        acc_b += zb * zb;
    }
    *sa = [ax1, ax2, ay1, ay2, az1, az2];
    *sb = [bx1, bx2, by1, by2, bz1, bz2];
    (acc_a, acc_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BS.1770-4 Annex 1, Tables 1 and 2 (48 kHz), as published.
    #[test]
    fn coefficients_match_the_bs1770_48k_table() {
        let [p, h] = k_weighting(48_000.0);
        let table_p = [
            1.535_124_859_586_97,
            -2.691_696_189_406_38,
            1.198_392_810_852_85,
            -1.690_659_293_182_41,
            0.732_480_774_215_85,
        ];
        let got_p = [p.b0, p.b1, p.b2, p.a1, p.a2];
        for (g, w) in got_p.iter().zip(table_p) {
            assert!((g - w).abs() < 1e-8, "pre-filter {g} vs {w}");
        }
        let table_h = [1.0, -2.0, 1.0, -1.990_047_454_833_98, 0.990_072_250_366_21];
        let got_h = [h.b0, h.b1, h.b2, h.a1, h.a2];
        for (g, w) in got_h.iter().zip(table_h) {
            assert!((g - w).abs() < 1e-8, "high-pass {g} vs {w}");
        }
    }

    /// Fixed anchors: gain of the cascade at 48 kHz, evaluated independently
    /// (Python `cmath`, outside this code) from the *published* Table 1/2
    /// coefficients: 100 Hz −1.133498 dB, 1 kHz +0.697704 dB, 10 kHz
    /// +4.041882 dB.
    #[test]
    fn response_matches_published_table_anchors() {
        for (f, want) in [
            (100.0, -1.133_498),
            (1_000.0, 0.697_704),
            (10_000.0, 4.041_882),
        ] {
            let got = k_weighting_gain_db(f, 48_000.0);
            assert!((got - want).abs() < 1e-5, "{f} Hz: {got} vs {want}");
        }
    }

    /// Below Nyquist the response is the same at every rate up to the
    /// rate-dependent pass-band gain of the literal 1, −2, 1 high-pass
    /// numerator (≤ +0.05 dB vs 48 kHz at 22.05 kHz) and shelf warping.
    #[test]
    fn response_shape_is_rate_independent_below_nyquist() {
        for f in [50.0, 200.0, 1_000.0, 4_000.0] {
            let ref_db = k_weighting_gain_db(f, 48_000.0);
            for rate in [22_050.0, 44_100.0, 96_000.0, 192_000.0] {
                let db = k_weighting_gain_db(f, rate);
                assert!(
                    (db - ref_db).abs() < 0.1,
                    "{f} Hz @ {rate}: {db} vs {ref_db}"
                );
            }
        }
        // ~+0.69 dB at 1 kHz (cancelled by BS.1770's −0.691), ~+4 dB high.
        assert!((k_weighting_gain_db(1_000.0, 48_000.0) - 0.691).abs() < 0.01);
        assert!((k_weighting_gain_db(10_000.0, 48_000.0) - 4.0).abs() < 0.1);
        assert!(k_weighting_gain_db(10.0, 48_000.0) < -10.0);
    }

    #[test]
    fn filter_ignores_non_finite_samples_and_carries_state() {
        let f = k_weighting(48_000.0);
        let x: Vec<f32> = (0..960).map(|i| (i as f32 * 0.13).sin()).collect();
        let mut st = [0.0; 6];
        let whole = filter_energy(&x, &mut st, &f);
        let mut st2 = [0.0; 6];
        let split = filter_energy(&x[..333], &mut st2, &f) + filter_energy(&x[333..], &mut st2, &f);
        assert!((whole - split).abs() < 1e-9 * whole);
        assert_eq!(st, st2);
        let mut st3 = [0.0; 6];
        let e = filter_energy(&[f32::NAN, f32::INFINITY, 0.5], &mut st3, &f);
        assert!(e.is_finite() && st3.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn pair_kernel_is_bit_identical_to_single() {
        let f = k_weighting(44_100.0);
        let a: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.07).sin()).collect();
        let b: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.31).cos() * 0.2).collect();
        let (mut s1, mut s2, mut p1, mut p2) = ([0.0; 6], [0.0; 6], [0.0; 6], [0.0; 6]);
        let single = (
            filter_energy(&a, &mut s1, &f),
            filter_energy(&b, &mut s2, &f),
        );
        let pair = filter_energy_pair(&a, &b, &mut p1, &mut p2, &f);
        assert_eq!(single, pair);
        assert_eq!((s1, s2), (p1, p2));
    }
}

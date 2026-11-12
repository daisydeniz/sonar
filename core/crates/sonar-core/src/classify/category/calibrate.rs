//! Confidence calibration: softmax with a temperature, temperature fitting
//! by negative log-likelihood (C. Guo, G. Pleiss, Y. Sun, K. Weinberger,
//! "On calibration of modern neural networks", ICML 2017, §4.2
//! "temperature scaling") and the expected calibration error (ibid. §2,
//! equal-width confidence bins).

/// Confidence bins of [`ece`] (Guo et al. use 15).
pub const ECE_BINS: usize = 15;
/// Search range of [`fit_temperature`].
pub const T_RANGE: (f32, f32) = (0.05, 20.0);

/// `softmax(z / t)` into `out` (same length as `z`); a non-positive or
/// non-finite `t` counts as 1. Uniform when every logit is equal or `z` has
/// a non-finite value.
pub fn softmax(z: &[f32], t: f32, out: &mut [f32]) {
    let t = if t.is_finite() && t > 0.0 { t } else { 1.0 };
    let n = z.len().min(out.len());
    if n == 0 {
        return;
    }
    let max = z[..n].iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() || z[..n].iter().any(|v| !v.is_finite()) {
        out[..n].fill(1.0 / n as f32);
        return;
    }
    let mut sum = 0.0f64;
    for (o, &v) in out[..n].iter_mut().zip(&z[..n]) {
        let e = f64::from((v - max) / t).exp();
        *o = e as f32;
        sum += e;
    }
    for o in &mut out[..n] {
        *o = (f64::from(*o) / sum) as f32;
    }
}

/// Mean negative log-likelihood of the true classes under `softmax(z / t)`.
pub fn nll(samples: &[(&[f32], usize)], t: f32) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut p = Vec::new();
    let mut total = 0.0f64;
    for (z, y) in samples {
        p.resize(z.len(), 0.0);
        softmax(z, t, &mut p);
        let py = p.get(*y).copied().unwrap_or(0.0);
        total -= f64::from(py.max(1e-12)).ln();
    }
    total / samples.len() as f64
}

/// Temperature minimising [`nll`] over [`T_RANGE`] (golden-section search
/// on `ln t`; NLL is unimodal in `t` for practical data). 1 when there are
/// no samples.
pub fn fit_temperature(samples: &[(&[f32], usize)]) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }
    let (mut a, mut b) = (T_RANGE.0.ln(), T_RANGE.1.ln());
    let g = (5f32.sqrt() - 1.0) / 2.0;
    let f = |x: f32| nll(samples, x.exp());
    let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..60 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    ((a + b) / 2.0).exp()
}

/// Expected calibration error in percent over `(confidence, correct)`
/// pairs: `Σ_b |B_b|/n · |acc(B_b) − conf(B_b)|` with [`ECE_BINS`]
/// equal-width bins on `[0, 1]`. 0 for no samples.
pub fn ece(samples: &[(f32, bool)]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut count = [0usize; ECE_BINS];
    let mut conf = [0.0f64; ECE_BINS];
    let mut hits = [0usize; ECE_BINS];
    for &(c, ok) in samples {
        let c = if c.is_finite() {
            c.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let b = ((c * ECE_BINS as f32) as usize).min(ECE_BINS - 1);
        count[b] += 1;
        conf[b] += f64::from(c);
        hits[b] += usize::from(ok);
    }
    let n = samples.len() as f64;
    let gap: f64 = (0..ECE_BINS)
        .filter(|&b| count[b] > 0)
        .map(|b| {
            let k = count[b] as f64;
            (k / n) * (hits[b] as f64 / k - conf[b] / k).abs()
        })
        .sum();
    100.0 * gap
}

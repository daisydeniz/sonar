//! MFCC (Davis & Mermelstein 1980, "Comparison of parametric
//! representations for monosyllabic word recognition in continuously
//! spoken sentences", IEEE Trans. ASSP 28(4)) and delta features.
//!
//! MFCCs are the orthonormal DCT-II of each log-mel (dB) frame, keeping
//! the first `n_out` coefficients including `c0` (as
//! `librosa.feature.mfcc(dct_type=2, norm="ortho")`):
//! `c_k = s_k · Σ_n x_n · cos(π·k·(2n+1) / 2N)`, `s_0 = √(1/N)`,
//! `s_k = √(2/N)`. Deltas use the HTK regression formula (Young et al.,
//! *The HTK Book*, §5.9; Furui 1986).

use super::FeatureMatrix;
use crate::{Error, Result};

/// Orthonormal DCT-II as a precomputed `n_out × n_in` matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct Dct {
    n_in: usize,
    n_out: usize,
    matrix: Vec<f32>,
}

impl Dct {
    /// Plans a DCT of `n_in` inputs keeping `n_out ≤ n_in` coefficients.
    pub fn new(n_in: usize, n_out: usize) -> Result<Self> {
        if n_in == 0 || n_out == 0 || n_out > n_in {
            return Err(Error::Dsp {
                reason: format!("DCT needs 1 ≤ n_out ≤ n_in (got {n_out}, {n_in})"),
            });
        }
        let n = n_in as f64;
        let mut matrix = Vec::with_capacity(n_out * n_in);
        for k in 0..n_out {
            let s = if k == 0 {
                (1.0 / n).sqrt()
            } else {
                (2.0 / n).sqrt()
            };
            for i in 0..n_in {
                let arg = std::f64::consts::PI * k as f64 * (2 * i + 1) as f64 / (2.0 * n);
                matrix.push((s * arg.cos()) as f32);
            }
        }
        Ok(Self {
            n_in,
            n_out,
            matrix,
        })
    }

    /// Input length.
    pub fn n_in(&self) -> usize {
        self.n_in
    }

    /// Output length.
    pub fn n_out(&self) -> usize {
        self.n_out
    }

    /// Basis row `k` (`n_in` values).
    pub fn row(&self, k: usize) -> Option<&[f32]> {
        self.matrix.get(k * self.n_in..(k + 1) * self.n_in)
    }

    /// `out[k] = Σ_i D[k,i] · x[i]` for `k < min(out.len(), n_out)`
    /// (missing inputs count as 0).
    pub fn apply(&self, x: &[f32], out: &mut [f32]) {
        for (o, row) in out.iter_mut().zip(self.matrix.chunks_exact(self.n_in)) {
            *o = row.iter().zip(x).map(|(d, v)| d * v).sum();
        }
    }

    /// MFCC matrix (`n_frames × n_out`) of a log-mel matrix.
    pub fn mfcc(&self, log_mel: &FeatureMatrix) -> FeatureMatrix {
        let mut out = FeatureMatrix::zeros(log_mel.n_frames, self.n_out);
        for (src, dst) in log_mel.rows().zip(out.rows_mut()) {
            self.apply(src, dst);
        }
        out
    }
}

/// Regression half-width `N` for [`deltas`] (HTK `DELTAWINDOW = 2`).
pub const DELTA_WIDTH: usize = 2;

/// Delta (time-derivative) features per column:
/// `d_t = Σ_{n=1..N} n·(c_{t+n} − c_{t−n}) / (2·Σ n²)`, with frames past
/// either end replaced by the first/last frame. `width = 0` → all zeros.
pub fn deltas(m: &FeatureMatrix, width: usize) -> FeatureMatrix {
    let mut out = FeatureMatrix::zeros(m.n_frames, m.n_cols);
    if width == 0 || m.n_frames == 0 {
        return out;
    }
    let denom = 2.0 * (1..=width).map(|n| (n * n) as f32).sum::<f32>();
    let last = m.n_frames - 1;
    for (t, dst) in out.rows_mut().enumerate() {
        for n in 1..=width {
            let fwd = m.row((t + n).min(last));
            let back = m.row(t.saturating_sub(n));
            for ((d, a), b) in dst.iter_mut().zip(fwd).zip(back) {
                *d += n as f32 * (a - b);
            }
        }
        for d in dst.iter_mut() {
            *d /= denom;
        }
    }
    out
}

//! **Timbre** aspect: MFCC statistics over active frames (the "bag of
//! frames" timbre model of J.-J. Aucouturier & F. Pachet, "Music similarity
//! measures: what's the use?", ISMIR 2002, reduced to mean/std).
//!
//! `c0` is dropped (ADR-019): it is the frame's mean log-mel level, i.e.
//! loudness, which the Amplitude aspect already covers; keeping it would
//! make two identical sounds at different gain look different in timbre.
//!
//! | index | content |
//! |---|---|
//! | `0..19` | mean of MFCC `c1..c19` (of the log-mel clipped at [`super::TOP_DB`]) |
//! | `19..38` | std of `c1..c19` |
//! | `38..57` | std of the HTK deltas of `c1..c19` (width [`DELTA_WIDTH`]) |
//!
//! Only the delta **std** is kept: the delta mean telescopes to
//! `(c_last − c_first) / n` and is ≈ 0 for almost every file, while the
//! std measures how fast the timbre moves. Without active frames every
//! value is 0.

use super::MeanStd;
use crate::dsp::{deltas, FeatureMatrix, DELTA_WIDTH, N_MFCC};

/// MFCC coefficients used (`c1..c{N_MFCC-1}`).
pub const TIMBRE_COEFFS: usize = N_MFCC - 1;
/// Timbre vector dimension.
pub const TIMBRE_DIM: usize = 3 * TIMBRE_COEFFS;

/// Timbre vector of the MFCC matrix `mfcc` (from the top-dB clipped
/// log-mel) over the `active` frames.
pub(super) fn compute(mfcc: &FeatureMatrix, active: &[bool]) -> Vec<f32> {
    let mut out = vec![0.0f32; TIMBRE_DIM];
    if mfcc.n_cols < N_MFCC || !active.iter().any(|&a| a) {
        return out;
    }
    let d = deltas(mfcc, DELTA_WIDTH);
    let mut c = [MeanStd::default(); TIMBRE_COEFFS];
    let mut dc = [MeanStd::default(); TIMBRE_COEFFS];
    for ((row, drow), _) in mfcc.rows().zip(d.rows()).zip(active).filter(|(_, &a)| a) {
        for (k, (acc, dacc)) in c.iter_mut().zip(dc.iter_mut()).enumerate() {
            acc.push(f64::from(row[k + 1]));
            dacc.push(f64::from(drow[k + 1]));
        }
    }
    for k in 0..TIMBRE_COEFFS {
        out[k] = c[k].mean() as f32;
        out[TIMBRE_COEFFS + k] = c[k].std() as f32;
        out[2 * TIMBRE_COEFFS + k] = dc[k].std() as f32;
    }
    out
}

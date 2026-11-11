//! Accent envelope for tempo: half-wave rectified flux of μ-law compressed
//! mel band powers, summed over bands.
//!
//! Method: A. Klapuri, A. Eronen, J. Astola, "Analysis of the meter of
//! acoustic musical signals" (IEEE TASLP 14(1), 2006, §III-A): band powers
//! normalized and compressed with `y = ln(1 + μ·x) / ln(1 + μ)`, then
//! differentiated and half-wave rectified. Unlike the dB-domain onset
//! envelope of [`crate::dsp::onset`] (which treats a ghost note rising from
//! silence almost like a full hit), the compression keeps level
//! differences, so kick/snare accents dominate quiet subdivisions — what
//! makes the beat level stand out from 8th/16th-note hats and ghost notes.
//!
//! Computed from the log-mel matrix of [`crate::dsp::Features`] (no second
//! STFT). `out[0]` is the flux from the last frame when `wrap` (the window
//! is one loop cycle, so a downbeat at the very start is not lost), else 0.

use crate::dsp::FeatureMatrix;

/// μ of the compression (Klapuri et al. 2006 use 100).
pub const MU: f32 = 100.0;

/// Accent envelope of `log_mel` (dB) into `out` (resized to the frame
/// count; all zeros for silence or fewer than two frames). Band powers are
/// normalized by the loudest band power in the window.
pub fn accent_envelope(log_mel: &FeatureMatrix, wrap: bool, out: &mut Vec<f32>) {
    out.clear();
    out.resize(log_mel.n_frames, 0.0);
    let cols = log_mel.n_cols;
    if cols == 0 || log_mel.n_frames < 2 || log_mel.data.len() < cols * log_mel.n_frames {
        return;
    }
    let max_db = log_mel
        .data
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f32::NEG_INFINITY, f32::max);
    if !max_db.is_finite() {
        return;
    }
    let norm = 1.0 / (1.0 + MU).ln();
    // x = 10^((L − max)/10) = exp((L − max)·ln10/10)
    let k = std::f32::consts::LN_10 / 10.0;
    let compress = |db: f32| -> f32 {
        let x = if db.is_finite() {
            ((db - max_db) * k).exp()
        } else {
            0.0
        };
        (MU * x).ln_1p() * norm
    };
    let first = if wrap { log_mel.n_frames - 1 } else { 0 };
    let mut prev: Vec<f32> = log_mel.row(first).iter().map(|&v| compress(v)).collect();
    let mut cur = vec![0.0f32; cols];
    for (t, o) in out.iter_mut().enumerate().skip(usize::from(!wrap)) {
        let mut flux = 0.0f32;
        for ((c, p), &db) in cur.iter_mut().zip(&prev).zip(log_mel.row(t)) {
            *c = compress(db);
            flux += (*c - p).max(0.0);
        }
        *o = flux;
        std::mem::swap(&mut prev, &mut cur);
    }
}

//! Onset strength envelope: half-wave rectified spectral flux on the
//! log-mel spectrogram, averaged over bands.
//!
//! Method: Ellis 2007 ("Beat tracking by dynamic programming", J. New
//! Music Research 36(1)) and Böck & Widmer 2013 ("Maximum filter vibrato
//! suppression for onset detection", DAFx-13) as summarized in librosa's
//! `onset_strength` (ISC; method only, no code taken):
//! `o[t] = mean_b max(0, L[t,b] − L[t−1,b])`, with `L` the log-mel in dB
//! clipped from below at `max(L) − ONSET_TOP_DB` so the noise floor of
//! quiet passages does not produce flux. `o[0] = 0`. Lag 1, no maximum
//! filter, no detrending.
//!
//! With centered Hann frames, an impulse at frame `t`'s centre first
//! shows (at half window weight) in frame `t − 1`, so its flux peak is at
//! `t − 1` or `t` (within one frame of the event).

use super::FeatureMatrix;

/// Dynamic range kept below the loudest log-mel value (dB).
pub const ONSET_TOP_DB: f32 = 80.0;

/// Onset strength per frame (same length as `log_mel.n_frames`, values
/// ≥ 0 in dB per band; all zeros for silence or a single frame).
pub fn onset_strength(log_mel: &FeatureMatrix) -> Vec<f32> {
    let mut env = vec![0.0f32; log_mel.n_frames];
    if log_mel.n_cols == 0 || log_mel.n_frames < 2 {
        return env;
    }
    let max = log_mel
        .data
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let floor = max - ONSET_TOP_DB;
    let scale = 1.0 / log_mel.n_cols as f32;
    let mut rows = log_mel.rows();
    let Some(mut prev) = rows.next() else {
        return env;
    };
    for (o, cur) in env[1..].iter_mut().zip(rows) {
        let flux: f32 = cur
            .iter()
            .zip(prev)
            .map(|(&c, &p)| (c.max(floor) - p.max(floor)).max(0.0))
            .sum();
        *o = flux * scale;
        prev = cur;
    }
    env
}

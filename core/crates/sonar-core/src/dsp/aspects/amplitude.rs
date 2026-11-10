//! **Amplitude** aspect: the energy envelope and dynamics of the analysis
//! samples. The envelope is the mean square of consecutive
//! [`ENV_BLOCK`]-sample blocks (1.45 ms at 22.05 kHz), smoothed by a
//! centred moving average of [`ENV_SMOOTH`] blocks (16 ms: one period down
//! to ≈ 63 Hz, so low notes do not dip at zero crossings), in dB relative
//! to its loudest block. Time descriptors follow G. Peeters, "A large set
//! of audio features for sound description" (CUIDADO report, IRCAM 2004):
//! attack time, decay time, effective duration (time above 40 % of the
//! maximum amplitude); times are `log10(seconds)`, floored at [`T_FLOOR`],
//! with ≈ 16 ms effective resolution. "Sound start" is the first block
//! within 40 dB of the peak.
//!
//! | index | content | silent value |
//! |---|---|---|
//! | [`ENV`]`..+16` | envelope (dB rel. peak, floor −80, mapped to `[0, 1]`) averaged in 16 log-time bins after the sound start: `[0, 5 ms)` then geometric edges 5 ms … 20 s | 0 |
//! | [`ATTACK`] | log attack time: first block ≥ −20 dB → first block ≥ −1 dB ("attack end") | −3 |
//! | [`DECAY`] | log decay time: attack end → first later block ≤ −20 dB (or the end of the signal) | −3 |
//! | [`EFFECTIVE`] | log effective duration: total time of blocks ≥ −8 dB | −3 |
//! | [`CREST`] | crest factor: sample peak / gated RMS (dB, `[0, 60]`) | 0 |
//! | [`LOUDNESS`] | gated RMS level (dBFS, `[−100, 80]`) | −100 |
//! | [`FLUCTUATION`] | std of the envelope level over gated blocks (dB) | 0 |
//!
//! *Gated* blocks are those within 40 dB of the peak block (cf. the
//! relative gate of ITU-R BS.1770), so silent padding does not lower the
//! level or raise the crest factor.

use super::super::yin::sanitize;

/// Samples per envelope block.
pub const ENV_BLOCK: usize = 32;
/// Width of the centred moving average over blocks (odd).
pub const ENV_SMOOTH: usize = 11;
/// Log-time envelope bins.
pub const ENV_POINTS: usize = 16;
/// First geometric bin edge (s).
pub const ENV_T_MIN: f64 = 0.005;
/// Last geometric bin edge (s).
pub const ENV_T_MAX: f64 = 20.0;
/// Envelope floor (dB relative to the peak block).
pub const ENV_FLOOR_DB: f64 = -80.0;
/// Shortest reported time (s); `log10` = −3.
pub const T_FLOOR: f64 = 0.001;
/// Index of the first envelope bin.
pub const ENV: usize = 0;
/// Index: log attack time.
pub const ATTACK: usize = ENV_POINTS;
/// Index: log decay time.
pub const DECAY: usize = ENV_POINTS + 1;
/// Index: log effective duration.
pub const EFFECTIVE: usize = ENV_POINTS + 2;
/// Index: crest factor (dB).
pub const CREST: usize = ENV_POINTS + 3;
/// Index: gated loudness (dBFS).
pub const LOUDNESS: usize = ENV_POINTS + 4;
/// Index: level fluctuation (dB).
pub const FLUCTUATION: usize = ENV_POINTS + 5;
/// Amplitude vector dimension.
pub const AMPLITUDE_DIM: usize = ENV_POINTS + 6;

/// Gate / sound-start threshold (dB below the peak block).
const GATE_DB: f64 = -40.0;
/// Mean square below which the whole signal counts as silent (−100 dBFS).
const SILENCE_MS: f64 = 1.0e-10;

/// Amplitude vector of `samples` at `sample_rate`.
pub(super) fn compute(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let mut out = vec![0.0f32; AMPLITUDE_DIM];
    let log_floor = T_FLOOR.log10() as f32;
    out[ATTACK] = log_floor;
    out[DECAY] = log_floor;
    out[EFFECTIVE] = log_floor;
    out[LOUDNESS] = -100.0;

    let ms = smoothed_blocks(samples);
    let peak_ms = ms.iter().copied().fold(0.0f64, f64::max);
    if peak_ms < SILENCE_MS {
        return out;
    }
    let bd = ENV_BLOCK as f64 / f64::from(sample_rate.max(1));
    let db: Vec<f64> = ms
        .iter()
        .map(|&m| (10.0 * (m / peak_ms).max(1e-30).log10()).max(ENV_FLOOR_DB))
        .collect();
    let first_at = |thr: f64| db.iter().position(|&d| d >= thr).unwrap_or(0);
    let log_t = |secs: f64| secs.max(T_FLOOR).log10() as f32;

    envelope(&db, first_at(GATE_DB), bd, &mut out[ENV..ENV + ENV_POINTS]);

    let attack_end = first_at(-1.0);
    out[ATTACK] = log_t(attack_end.saturating_sub(first_at(-20.0)) as f64 * bd);
    let tail = db.get(attack_end..).unwrap_or(&[]);
    let decay = tail.iter().position(|&d| d <= -20.0).unwrap_or(tail.len());
    out[DECAY] = log_t(decay as f64 * bd);
    out[EFFECTIVE] = log_t(db.iter().filter(|&&d| d >= -8.0).count() as f64 * bd);

    let (mut n, mut sum_ms, mut sum_db, mut sq_db) = (0usize, 0.0f64, 0.0f64, 0.0f64);
    for (&m, &d) in ms.iter().zip(&db).filter(|(_, &d)| d >= GATE_DB) {
        n += 1;
        sum_ms += m;
        sum_db += d;
        sq_db += d * d;
    }
    let n = n.max(1) as f64;
    let gated_ms = sum_ms / n;
    let mean_db = sum_db / n;
    let sample_peak = samples
        .iter()
        .map(|&x| f64::from(sanitize(x)).abs())
        .fold(0.0f64, f64::max);
    out[CREST] = (20.0 * (sample_peak / gated_ms.sqrt()).log10()).clamp(0.0, 60.0) as f32;
    out[LOUDNESS] = (10.0 * gated_ms.log10()).clamp(-100.0, 80.0) as f32;
    out[FLUCTUATION] = (sq_db / n - mean_db * mean_db).max(0.0).sqrt() as f32;
    out
}

/// Block mean squares smoothed by the centred [`ENV_SMOOTH`]-block moving
/// average (truncated at the ends).
fn smoothed_blocks(samples: &[f32]) -> Vec<f64> {
    let ms: Vec<f64> = samples
        .chunks(ENV_BLOCK)
        .map(|b| {
            b.iter()
                .map(|&x| f64::from(sanitize(x)).powi(2))
                .sum::<f64>()
                / b.len() as f64
        })
        .collect();
    let mut prefix = Vec::with_capacity(ms.len() + 1);
    prefix.push(0.0f64);
    for &m in &ms {
        prefix.push(prefix[prefix.len() - 1] + m);
    }
    let half = ENV_SMOOTH / 2;
    (0..ms.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(ms.len()));
            ((prefix[hi] - prefix[lo]) / (hi - lo) as f64).max(0.0)
        })
        .collect()
}

/// Envelope `db` (block levels) averaged in the log-time bins after block
/// `start`, mapped from `[ENV_FLOOR_DB, 0]` to `[0, 1]`; blocks past the
/// end count as the floor.
fn envelope(db: &[f64], start: usize, bd: f64, out: &mut [f32]) {
    let ratio = (ENV_T_MAX / ENV_T_MIN).powf(1.0 / (ENV_POINTS - 1) as f64);
    let edge = |k: usize| {
        if k == 0 {
            0.0
        } else {
            ENV_T_MIN * ratio.powi(k as i32 - 1)
        }
    };
    for (k, o) in out.iter_mut().enumerate() {
        let (t0, t1) = (edge(k), edge(k + 1));
        let (mut i0, mut i1) = ((t0 / bd).ceil() as usize, (t1 / bd).ceil() as usize);
        if i1 <= i0 {
            // Bin narrower than a block: the block under its centre.
            i0 = (0.5 * (t0 + t1) / bd) as usize;
            i1 = i0 + 1;
        }
        let (a, b) = (start.saturating_add(i0), start.saturating_add(i1));
        let inside = db.get(a.min(db.len())..b.min(db.len())).unwrap_or(&[]);
        let n = (b - a).max(1);
        let n_out = n.saturating_sub(inside.len());
        let sum = inside.iter().sum::<f64>() + n_out as f64 * ENV_FLOOR_DB;
        let mean = sum / n as f64;
        *o = (1.0 - mean / ENV_FLOOR_DB).clamp(0.0, 1.0) as f32;
    }
}

//! **Spectrum** aspect: the shape of the long-term spectral envelope and
//! the classic frame descriptors (G. Peeters, "A large set of audio
//! features for sound description", CUIDADO report, IRCAM 2004), over
//! active frames.
//!
//! | index | content |
//! |---|---|
//! | `0..16` | band mean (dB, log-mel clipped at [`super::TOP_DB`]) of 16 groups of 4 adjacent log-mel bands, minus the mean over the 16 groups (level-invariant shape) |
//! | `16..32` | band std over frames (dB) |
//! | `32, 33` | spectral centroid of `|X|`, `log2(c / 1 kHz)`: mean, std |
//! | `34, 35` | 85 % power roll-off, `log2(f / 1 kHz)`: mean, std |
//! | `36, 37` | spectral flatness (Wiener entropy: geometric / arithmetic mean of power, bins 1..Nyquist, each floored 60 dB below the frame mean) in dB ∈ [−60, 0]: mean, std |
//!
//! Without active frames every value is 0.

use super::MeanStd;
use crate::dsp::{FeatureMatrix, Spectrogram};

/// Groups of adjacent log-mel bands.
pub const SPECTRUM_BANDS: usize = 16;
/// Spectrum vector dimension.
pub const SPECTRUM_DIM: usize = 2 * SPECTRUM_BANDS + 6;
/// Roll-off fraction of total frame power.
pub const ROLLOFF: f64 = 0.85;
/// Flatness floor (dB) and per-bin power floor relative to the frame mean.
pub const FLATNESS_FLOOR_DB: f64 = -60.0;
/// Index of the centroid mean (std follows).
pub const CENTROID: usize = 2 * SPECTRUM_BANDS;
/// Index of the roll-off mean (std follows).
pub const ROLLOFF_IDX: usize = CENTROID + 2;
/// Index of the flatness mean (std follows).
pub const FLATNESS: usize = CENTROID + 4;

/// Spectrum vector from the (top-dB clipped) log-mel `mel` and power
/// spectrogram `spec` over the `active` frames.
pub(super) fn compute(mel: &FeatureMatrix, spec: &Spectrogram, active: &[bool]) -> Vec<f32> {
    let mut out = vec![0.0f32; SPECTRUM_DIM];
    let n_cols = mel.n_cols;
    let mut bands = [MeanStd::default(); SPECTRUM_BANDS];
    let (mut cent, mut roll, mut flat) =
        (MeanStd::default(), MeanStd::default(), MeanStd::default());
    let n_bins = spec.n_bins();
    let df = spec.sample_rate as f64 / spec.n_fft.max(1) as f64;
    let mut any = false;
    for ((row, power), _) in mel
        .rows()
        .zip(spec.frames())
        .zip(active)
        .filter(|(_, &a)| a)
    {
        any = true;
        for (b, acc) in bands.iter_mut().enumerate() {
            let (lo, hi) = (
                b * n_cols / SPECTRUM_BANDS,
                (b + 1) * n_cols / SPECTRUM_BANDS,
            );
            let cols = row.get(lo..hi).unwrap_or(&[]);
            if !cols.is_empty() {
                acc.push(cols.iter().map(|&v| f64::from(v)).sum::<f64>() / cols.len() as f64);
            }
        }
        if let Some((c, r, fl)) = frame_descriptors(&power[1..n_bins], df) {
            cent.push(c);
            roll.push(r);
            flat.push(fl);
        }
    }
    if !any {
        return out;
    }
    let level = bands.iter().map(MeanStd::mean).sum::<f64>() / SPECTRUM_BANDS as f64;
    for (b, acc) in bands.iter().enumerate() {
        out[b] = (acc.mean() - level) as f32;
        out[SPECTRUM_BANDS + b] = acc.std() as f32;
    }
    for (i, acc) in [(CENTROID, cent), (ROLLOFF_IDX, roll), (FLATNESS, flat)] {
        out[i] = acc.mean() as f32;
        out[i + 1] = acc.std() as f32;
    }
    out
}

/// Centroid and roll-off (`log2(Hz / 1000)`) and flatness (dB) of one
/// power frame without its DC bin (`bins[i]` is bin `i + 1`); `None` when
/// the frame has no energy.
fn frame_descriptors(bins: &[f32], df: f64) -> Option<(f64, f64, f64)> {
    let (mut p_sum, mut m_sum, mut m_moment) = (0.0f64, 0.0f64, 0.0f64);
    for (i, &p) in bins.iter().enumerate() {
        let p = f64::from(p.max(0.0));
        let m = p.sqrt();
        p_sum += p;
        m_sum += m;
        m_moment += (i + 1) as f64 * m;
    }
    if !(p_sum > 0.0 && p_sum.is_finite()) || bins.is_empty() {
        return None;
    }
    let centroid_hz = (m_moment / m_sum * df).max(df);
    let target = ROLLOFF * p_sum;
    let mut acc = 0.0f64;
    let mut k = bins.len();
    for (i, &p) in bins.iter().enumerate() {
        acc += f64::from(p.max(0.0));
        if acc >= target {
            k = i + 1;
            break;
        }
    }
    let rolloff_hz = k as f64 * df;
    let mean = p_sum / bins.len() as f64;
    let floor = ((mean * 10f64.powf(FLATNESS_FLOOR_DB / 10.0)) as f32).max(f32::MIN_POSITIVE);
    let log_sum: f64 = bins.iter().map(|&p| f64::from(p.max(floor).ln())).sum();
    let geo = (log_sum / bins.len() as f64).exp();
    let flat_db = (10.0 * (geo / mean).log10()).clamp(FLATNESS_FLOOR_DB, 0.0);
    Some((
        (centroid_hz / 1000.0).log2(),
        (rolloff_hz / 1000.0).log2(),
        flat_db,
    ))
}

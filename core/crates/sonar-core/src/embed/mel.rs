//! Slaney mel filterbank of the CLAP front end (ADR-026): 64 triangular
//! bands, 50-14,000 Hz, Slaney mel scale and Slaney area normalisation
//! (librosa `htk=False, norm="slaney"`), evaluated at the 513 FFT bin
//! frequencies of a 1024-point FFT at 48 kHz.

use super::front::{FMAX, FMIN, N_BINS, N_FFT, N_MELS, SAMPLE_RATE};

/// One band: `weights[j]` applies to FFT bin `first + j`.
#[derive(Debug, Clone)]
pub(super) struct Band {
    pub first: usize,
    pub weights: Vec<f32>,
}

/// Slaney mel scale: linear below 1 kHz, logarithmic above.
fn hz_to_mel(f: f64) -> f64 {
    if f < 1000.0 {
        3.0 * f / 200.0
    } else {
        15.0 + (f / 1000.0).ln() * 27.0 / 6.4f64.ln()
    }
}

fn mel_to_hz(m: f64) -> f64 {
    if m < 15.0 {
        200.0 * m / 3.0
    } else {
        1000.0 * ((m - 15.0) * 6.4f64.ln() / 27.0).exp()
    }
}

/// The 64 bands, as sparse (first bin, weights) rows.
pub(super) fn filterbank() -> Vec<Band> {
    let (mmin, mmax) = (hz_to_mel(FMIN), hz_to_mel(FMAX));
    let points: Vec<f64> = (0..N_MELS + 2)
        .map(|i| mel_to_hz(mmin + (mmax - mmin) * i as f64 / (N_MELS + 1) as f64))
        .collect();
    let nyquist = f64::from(SAMPLE_RATE) / 2.0;
    debug_assert_eq!(N_BINS, N_FFT / 2 + 1);
    (0..N_MELS)
        .map(|m| {
            let (lo, centre, hi) = (points[m], points[m + 1], points[m + 2]);
            let norm = 2.0 / (hi - lo);
            let mut first = None;
            let mut weights = Vec::new();
            for k in 0..N_BINS {
                let f = nyquist * k as f64 / (N_BINS - 1) as f64;
                let v = ((f - lo) / (centre - lo))
                    .min((hi - f) / (hi - centre))
                    .max(0.0)
                    * norm;
                if v > 0.0 {
                    first.get_or_insert(k);
                    weights.push(v as f32);
                } else if first.is_some() {
                    break;
                }
            }
            Band {
                first: first.unwrap_or(0),
                weights,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slaney_scale_round_trips_and_is_linear_below_1khz() {
        for f in [50.0, 440.0, 999.0, 1000.0, 5000.0, 14_000.0] {
            assert!((mel_to_hz(hz_to_mel(f)) - f).abs() < 1e-6, "{f}");
        }
        assert!((hz_to_mel(1000.0) - 15.0).abs() < 1e-12);
        assert!((hz_to_mel(200.0) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn bands_cover_the_range_in_order_with_area_normalisation() {
        let bands = filterbank();
        assert_eq!(bands.len(), N_MELS);
        let bin_hz = f64::from(SAMPLE_RATE) / N_FFT as f64;
        let mut last_first = 0;
        for (m, b) in bands.iter().enumerate() {
            assert!(!b.weights.is_empty(), "band {m} is empty");
            assert!(b.first >= last_first, "bands must move up in frequency");
            last_first = b.first;
            assert!(b.first + b.weights.len() <= N_BINS);
            assert!(b.weights.iter().all(|w| w.is_finite() && *w > 0.0));
            // Slaney normalisation: the sampled triangle's area is about 1
            // (per-Hz weights integrate to 1; sampled at `bin_hz`).
            let area: f64 = b.weights.iter().map(|&w| f64::from(w)).sum::<f64>() * bin_hz;
            assert!((0.6..1.4).contains(&area), "band {m}: area {area}");
        }
        let lowest_hz = bands[0].first as f64 * bin_hz;
        let highest = &bands[N_MELS - 1];
        let highest_hz = (highest.first + highest.weights.len()) as f64 * bin_hz;
        assert!((0.0..150.0).contains(&lowest_hz), "{lowest_hz}");
        assert!((13_000.0..=14_100.0).contains(&highest_hz), "{highest_hz}");
    }
}

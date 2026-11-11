//! Tonal chroma for key finding: a time-averaged pitch-class profile built
//! from **sinusoidal spectral peaks** only, with a global tuning correction.
//!
//! Per frame of the existing STFT power spectrogram (no second FFT):
//! 1. local maxima between [`KEY_FMIN`] and [`KEY_FMAX`] that stand at least
//!    [`PEAK_PROMINENCE`] above the bins 3–4 bins away on both sides (outside
//!    the Hann main lobe) — partials of pitched sounds pass, noise and drum
//!    hits mostly do not;
//! 2. the peak frequency is refined by quadratic interpolation of the log
//!    power (J. O. Smith & X. Serra, "PARSHL", ICMC 1987) and its magnitude
//!    (`√` of the three-bin power) is added to a 120-bin (10 cent) pitch
//!    class histogram;
//! 3. the reference tuning is the magnitude-weighted circular mean of the
//!    peaks' deviation from equal temperament (C. Harte & M. Sandler,
//!    "Automatic chord identification using a quantised chromagram", AES
//!    118, 2005; Gómez 2006 §3.4), so a sample tuned 30 cents sharp keeps
//!    its pitch classes; the histogram is then folded into 12 classes
//!    shifted by that deviation (soft, linear split like [`crate::dsp::chroma`]).
//!
//! This is the HPCP idea of E. Gómez (*Tonal Description of Music Audio
//! Signals*, 2006, ch. 3: peaks, tuning, magnitude weighting) without the
//! harmonic weighting, which the key profiles model instead
//! ([`super::profiles::harmonic_expand`]). Frames more than
//! [`FRAME_FLOOR_DB`] below the loudest are skipped.
//!
//! The **tonal ratio** (power in prominent peaks ÷ all power in the band)
//! says how much of the sound is pitched: near 1 for synths and sines,
//! low for noise and drums.

use crate::dsp::chroma::N_CHROMA;
use crate::dsp::Spectrogram;

/// Lowest peak frequency used (Hz): below the fixture/real kick tails
/// (≈ 40–60 Hz), above which bass notes are resolved by peak interpolation.
pub const KEY_FMIN: f32 = 80.0;
/// Highest peak frequency used (Hz).
pub const KEY_FMAX: f32 = 5000.0;
/// A peak must exceed the mean power 3–4 bins away by this factor (10 dB).
pub const PEAK_PROMINENCE: f32 = 10.0;
/// Frames more than this far below the loudest frame are skipped (dB).
pub const FRAME_FLOOR_DB: f32 = 60.0;
/// Histogram resolution (bins per semitone).
const FINE: usize = 10;
/// Histogram bins (one octave).
const N_FINE: usize = 12 * FINE;

/// Time-averaged tonal chroma of one file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TonalChroma {
    /// Pitch-class profile C..B (L1 normalized; all zero without peaks).
    pub chroma: [f32; N_CHROMA],
    /// Reference tuning: deviation of the partials from A = 440 Hz equal
    /// temperament (semitones, `−0.5..0.5`).
    pub tuning: f32,
    /// Power in prominent peaks ÷ all power in [`KEY_FMIN`]..[`KEY_FMAX`]
    /// over the analysed frames (`0..=1`).
    pub tonal_ratio: f32,
}

impl TonalChroma {
    /// No pitched content.
    pub const EMPTY: Self = Self {
        chroma: [0.0; N_CHROMA],
        tuning: 0.0,
        tonal_ratio: 0.0,
    };
}

/// Tonal chroma of a power spectrogram (see the module docs). Never panics;
/// silence, empty and non-finite input give [`TonalChroma::EMPTY`].
pub fn tonal_chroma(spec: &Spectrogram) -> TonalChroma {
    let n_bins = spec.n_bins();
    if spec.n_frames == 0 || n_bins < 16 || spec.sample_rate == 0 {
        return TonalChroma::EMPTY;
    }
    let df = spec.sample_rate as f32 / spec.n_fft.max(1) as f32;
    let lo = ((KEY_FMIN / df).floor() as usize).max(4);
    let hi = ((KEY_FMAX / df).ceil() as usize).min(n_bins.saturating_sub(5));
    if lo >= hi {
        return TonalChroma::EMPTY;
    }
    let band_power = |frame: &[f32]| -> f32 {
        frame[lo..=hi]
            .iter()
            .map(|&p| if p.is_finite() { p.max(0.0) } else { 0.0 })
            .sum()
    };
    let totals: Vec<f32> = spec.frames().map(band_power).collect();
    let loudest = totals.iter().copied().fold(0.0f32, f32::max);
    if !(loudest > 0.0 && loudest.is_finite()) {
        return TonalChroma::EMPTY;
    }
    let floor = loudest * 10f32.powf(-FRAME_FLOOR_DB / 10.0);
    // A4 = MIDI 69 at 440 Hz; pitch class of frequency f (semitones above C).
    let pc_of = |f: f32| (69.0 + 12.0 * (f / 440.0).log2()).rem_euclid(12.0);

    let mut hist = [0.0f64; N_FINE];
    let (mut peak_power, mut total_power) = (0.0f64, 0.0f64);
    for (frame, &total) in spec.frames().zip(&totals) {
        if !(total >= floor && total > 0.0) {
            continue;
        }
        total_power += f64::from(total);
        let p = |k: usize| {
            let v = frame[k];
            if v.is_finite() {
                v.max(0.0)
            } else {
                0.0
            }
        };
        for k in lo..=hi {
            let c = p(k);
            if !(c > p(k - 1) && c >= p(k + 1) && c > 0.0) {
                continue;
            }
            let side = 0.25 * (p(k - 3) + p(k - 4) + p(k + 3) + p(k + 4));
            if c < PEAK_PROMINENCE * side {
                continue;
            }
            let (a, b) = (p(k - 1), p(k + 1));
            let power = a + c + b;
            peak_power += f64::from(power);
            let shift = log_parabola_shift(a, c, b);
            let f = (k as f32 + shift) * df;
            if !(KEY_FMIN..=KEY_FMAX).contains(&f) {
                continue;
            }
            let pos = pc_of(f) * FINE as f32;
            let i0 = pos.floor();
            let w1 = f64::from(pos - i0);
            let i0 = (i0 as usize) % N_FINE;
            let m = f64::from(power.sqrt());
            hist[i0] += (1.0 - w1) * m;
            hist[(i0 + 1) % N_FINE] += w1 * m;
        }
    }
    let tonal_ratio = if total_power > 0.0 {
        (peak_power / total_power).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let tuning = tuning_of(&hist);
    let chroma = fold(&hist, tuning);
    if chroma.iter().all(|&c| c == 0.0) {
        return TonalChroma {
            tonal_ratio,
            ..TonalChroma::EMPTY
        };
    }
    TonalChroma {
        chroma,
        tuning,
        tonal_ratio,
    }
}

/// Vertex offset (bins, `−0.5..0.5`) of the parabola through the log powers
/// of three bins around a peak.
fn log_parabola_shift(a: f32, b: f32, c: f32) -> f32 {
    if !(a > 0.0 && b > 0.0 && c > 0.0) {
        return 0.0;
    }
    let (la, lb, lc) = (a.ln(), b.ln(), c.ln());
    let denom = la - 2.0 * lb + lc;
    if denom < 0.0 {
        (0.5 * (la - lc) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    }
}

/// Weighted circular mean of the histogram's deviation from the semitone
/// grid (semitones, `−0.5..0.5`).
fn tuning_of(hist: &[f64; N_FINE]) -> f32 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for (i, &w) in hist.iter().enumerate() {
        let phase = std::f64::consts::TAU * (i % FINE) as f64 / FINE as f64;
        s += w * phase.sin();
        c += w * phase.cos();
    }
    if s * s + c * c <= 0.0 {
        return 0.0;
    }
    let dev = s.atan2(c) / std::f64::consts::TAU;
    dev.clamp(-0.5, 0.5) as f32
}

/// Folds the fine histogram into 12 classes centred on `tuning`, L1
/// normalized.
fn fold(hist: &[f64; N_FINE], tuning: f32) -> [f32; N_CHROMA] {
    let mut out = [0.0f64; N_CHROMA];
    for (i, &w) in hist.iter().enumerate() {
        if w <= 0.0 {
            continue;
        }
        let q = (i as f64 / FINE as f64 - f64::from(tuning)).rem_euclid(12.0);
        let i0 = q.floor();
        let w1 = q - i0;
        let i0 = (i0 as usize) % N_CHROMA;
        out[i0] += (1.0 - w1) * w;
        out[(i0 + 1) % N_CHROMA] += w1 * w;
    }
    let sum: f64 = out.iter().sum();
    let mut chroma = [0.0f32; N_CHROMA];
    if sum > 0.0 && sum.is_finite() {
        for (c, v) in chroma.iter_mut().zip(out) {
            *c = (v / sum) as f32;
        }
    }
    chroma
}

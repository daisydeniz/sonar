//! **Pitch** aspect: f0 from [`crate::dsp::yin`] (de Cheveigné & Kawahara
//! 2002) and a 12-bin chroma summary ([`crate::dsp::chroma`], Fujishima
//! 1999). Chroma is **not** key-rotated: C stays at index 0.
//!
//! YIN frames are *active* when their energy is within
//! [`super::ACTIVE_RANGE_DB`] of the loudest YIN frame and not silent.
//!
//! | index | content | unvoiced / silent value |
//! |---|---|---|
//! | [`VOICED`] | voiced active frames / active frames | 0 |
//! | [`CONFIDENCE`] | mean YIN confidence `1 − d'` over voiced frames | 0 |
//! | [`F0`] | median voiced f0 as `log2(f0 / 440 Hz)` (octaves from A4; scale-invariant) | 0 |
//! | [`F0_SPREAD`] | inter-quartile range of voiced f0 (octaves) | 0 |
//! | [`PC_COS`], [`PC_SIN`] | pitch class of the median f0 on the unit circle, `cos/sin(2π·pc/12)`, `pc` 0 = C | 0, 0 |
//! | [`CHROMA`]`..+12` | mean per-frame L1-normalized chroma over active STFT frames, C..B | 1/12 each |

use super::super::chroma::{ChromaMap, N_CHROMA};
use super::super::yin::Yin;
use super::ACTIVE_RANGE_DB;
use crate::dsp::Features;
use crate::Result;

/// Index: voiced fraction.
pub const VOICED: usize = 0;
/// Index: mean confidence of voiced frames.
pub const CONFIDENCE: usize = 1;
/// Index: median f0, octaves relative to 440 Hz.
pub const F0: usize = 2;
/// Index: f0 inter-quartile range (octaves).
pub const F0_SPREAD: usize = 3;
/// Index: cosine of the median's pitch-class angle.
pub const PC_COS: usize = 4;
/// Index: sine of the median's pitch-class angle.
pub const PC_SIN: usize = 5;
/// Index of the first chroma bin (C).
pub const CHROMA: usize = 6;
/// Pitch vector dimension.
pub const PITCH_DIM: usize = CHROMA + N_CHROMA;

/// Pitch vector of `samples` (YIN) and `f` (chroma over `active` frames).
pub(super) fn compute(
    yin: &mut Yin,
    chroma: &ChromaMap,
    samples: &[f32],
    f: &Features,
    active: &[bool],
) -> Result<Vec<f32>> {
    let mut out = vec![0.0f32; PITCH_DIM];

    let frames = yin.track(samples)?;
    let max_e = frames.iter().map(|fr| fr.energy).fold(0.0f32, f32::max);
    let gate = max_e * 10f32.powf(-ACTIVE_RANGE_DB / 10.0);
    let mut n_active = 0usize;
    let mut conf = 0.0f64;
    let mut f0_oct: Vec<f32> = Vec::new();
    for fr in frames
        .iter()
        .filter(|fr| fr.energy > gate && fr.f0_hz > 0.0)
    {
        n_active += 1;
        if fr.voiced {
            conf += f64::from(1.0 - fr.aperiodicity);
            f0_oct.push((fr.f0_hz / 440.0).log2());
        }
    }
    if !f0_oct.is_empty() {
        let n_voiced = f0_oct.len();
        f0_oct.sort_by(f32::total_cmp);
        let median = percentile(&f0_oct, 0.5);
        out[VOICED] = n_voiced as f32 / n_active.max(1) as f32;
        out[CONFIDENCE] = (conf / n_voiced as f64) as f32;
        out[F0] = median;
        out[F0_SPREAD] = percentile(&f0_oct, 0.75) - percentile(&f0_oct, 0.25);
        // MIDI 69 (A4) is pitch class 9.
        let pc = (9.0 + 12.0 * median).rem_euclid(12.0);
        let angle = std::f32::consts::TAU * pc / 12.0;
        out[PC_COS] = angle.cos();
        out[PC_SIN] = angle.sin();
    }

    let mut acc = [0.0f64; N_CHROMA];
    let mut frame = [0.0f32; N_CHROMA];
    let mut n = 0usize;
    for (power, _) in f.spectrogram.frames().zip(active).filter(|(_, &a)| a) {
        if chroma.frame(power, &mut frame) {
            n += 1;
            for (a, &c) in acc.iter_mut().zip(&frame) {
                *a += f64::from(c);
            }
        }
    }
    for (o, a) in out[CHROMA..].iter_mut().zip(&acc) {
        *o = if n == 0 {
            1.0 / N_CHROMA as f32
        } else {
            (a / n as f64) as f32
        };
    }
    Ok(out)
}

/// Linear-interpolated percentile `q ∈ [0, 1]` of sorted, non-empty `v`.
fn percentile(v: &[f32], q: f32) -> f32 {
    let pos = q * (v.len().saturating_sub(1)) as f32;
    let i = pos.floor() as usize;
    let frac = pos - i as f32;
    match (v.get(i), v.get(i + 1)) {
        (Some(&a), Some(&b)) => a + frac * (b - a),
        (Some(&a), None) => a,
        _ => 0.0,
    }
}

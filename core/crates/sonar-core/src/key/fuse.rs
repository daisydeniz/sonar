//! Audio key candidates and their fusion with the filename key (T10.2,
//! ADR-041): every non-silent file gets its best key plus a graded
//! confidence and a [`KeySource`].
//!
//! **Audio candidate** ([`chroma_candidate`]). The 24 Krumhansl–Schmuckler
//! correlations of the tonal chroma (Temperley profiles, [`super::profiles`])
//! pick the key; a filename key adds [`NAME_BONUS`] to matching keys first,
//! so it decides close calls. Its confidence is
//!
//! `a = r · min(1, (r − r₂) / MARGIN_REF) · tonality`
//!
//! with `r` the best and `r₂` the runner-up score. The former tonality
//! gates of ADR-022 are now linear factors in `tonality` (product of three
//! ramps): tonal ratio from [`TONAL_RATIO_FLOOR`] (0) to
//! [`MIN_TONAL_RATIO`] (1), present pitch classes from 1 (0) to
//! [`MIN_PITCH_CLASSES`] (1), best score from [`CORRELATION_FLOOR`] (0) to
//! [`MIN_CORRELATION`] (1). Material that passed every old gate keeps its
//! old confidence; drums and noise keep their best guess at a confidence
//! near 0. When the tonal chroma is empty (no sinusoidal peaks: noise,
//! clicks) the candidate comes from the plain Fujishima chroma
//! ([`crate::dsp::chroma`]) with confidence 0.
//!
//! **Fusion** ([`fuse`]) of audio `(k, a)` and a name of strength `n`
//! ([`NAME_CONF_EXPLICIT`] / [`NAME_CONF_BARE`]), treating both as
//! independent probabilities of being right:
//! - agree (same root; same mode unless one has none) with real audio
//!   support (`a ≥` [`AUDIO_SUPPORT`]): noisy-or `1 − (1 − a)(1 − n)`,
//!   source [`KeySource::Both`]; weaker agreement (often made by the name
//!   bonus on non-tonal audio) counts as the name alone, `n`;
//! - disagree: the stronger wins and is discounted by the other, `w·(1 − l)`
//!   (winner right and loser wrong); ties go to the name;
//! - only one side: its own value (`a`, or `n`).

use super::chroma::TonalChroma;
use super::profiles;
use super::{KeyEstimate, KeySource};
use crate::dsp::chroma::{ChromaMap, N_CHROMA};
use crate::dsp::Spectrogram;
use crate::text::{Key, KeyPrior};

/// Tonal ratio at which the tonality factor reaches 1 (the ADR-022 gate).
pub const MIN_TONAL_RATIO: f32 = 0.5;
/// Tonal ratio at (and below) which the tonality factor is 0.
pub const TONAL_RATIO_FLOOR: f32 = 0.25;
/// A pitch class counts as present at this fraction of the strongest.
pub const PRESENT_FRACTION: f32 = 0.25;
/// Present pitch classes at which the factor reaches 1 (one note is not a
/// key: a single class gives 0).
pub const MIN_PITCH_CLASSES: usize = 3;
/// Best correlation at which the factor reaches 1 (the ADR-022 gate).
pub const MIN_CORRELATION: f32 = 0.5;
/// Best correlation at (and below) which the factor is 0.
pub const CORRELATION_FLOOR: f32 = 0.25;
/// Correlation margin over the runner-up at which confidence saturates.
pub const MARGIN_REF: f32 = 0.1;
/// Added to the correlation of keys matching the filename key.
pub const NAME_BONUS: f32 = 0.1;
/// Audio confidence from which an agreeing audio key counts as support
/// (source `both` rather than `filename`).
pub const AUDIO_SUPPORT: f32 = 0.2;
/// Strength of an explicit (`Am`, `F# minor`, `(C)`) filename key.
pub const NAME_CONF_EXPLICIT: f32 = 0.5;
/// Strength of a bare note letter (`Loop_120_C`).
pub const NAME_CONF_BARE: f32 = 0.35;

/// Best audio key of a chroma and its confidence (see the module docs);
/// `None` when the spectrogram has no energy in the chroma range.
pub(crate) fn chroma_candidate(
    tc: &TonalChroma,
    spec: &Spectrogram,
    prior: Option<KeyPrior>,
) -> Option<(Key, f32)> {
    let tonal = has_energy(&tc.chroma);
    let chroma = if tonal {
        tc.chroma
    } else {
        plain_chroma(spec)?
    };
    let mut scores = profiles::key_scores(&chroma);
    if let Some(p) = prior {
        for (i, s) in scores.iter_mut().enumerate() {
            if matches_prior(profiles::key_of_index(i), &p) {
                *s += NAME_BONUS;
            }
        }
    }
    let best = (0..scores.len()).fold(0, |b, i| if scores[i] > scores[b] { i } else { b });
    let runner_up = scores
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != best)
        .map(|(_, &s)| s)
        .fold(f32::MIN, f32::max);
    let r = scores[best];
    let margin = ((r - runner_up) / MARGIN_REF).clamp(0.0, 1.0);
    let factor = if tonal { tonality(tc, r) } else { 0.0 };
    let a = r.clamp(0.0, 1.0) * margin * factor;
    Some((profiles::key_of_index(best), finite01(a)))
}

/// Product of the three tonality ramps (see the module docs); 1 exactly
/// when the ADR-022 gates pass.
pub fn tonality(tc: &TonalChroma, best_score: f32) -> f32 {
    let pcs = present_classes(&tc.chroma) as f32;
    ramp(tc.tonal_ratio, TONAL_RATIO_FLOOR, MIN_TONAL_RATIO)
        * ramp(pcs, 1.0, MIN_PITCH_CLASSES as f32)
        * ramp(best_score, CORRELATION_FLOOR, MIN_CORRELATION)
}

/// Pitch classes of `chroma` at least [`PRESENT_FRACTION`] of the strongest.
pub fn present_classes(chroma: &[f32; N_CHROMA]) -> usize {
    let max = chroma.iter().copied().fold(0.0f32, f32::max);
    chroma
        .iter()
        .filter(|&&c| max > 0.0 && c >= PRESENT_FRACTION * max)
        .count()
}

/// Fused key of an audio candidate and a filename key (module docs).
pub(crate) fn fuse(audio: Option<(Key, f32)>, prior: Option<KeyPrior>) -> Option<KeyEstimate> {
    let est = |key, confidence, source| KeyEstimate {
        key,
        confidence: finite01(confidence),
        source,
        audio: None,
    };
    let name_key = |p: &KeyPrior| Key {
        root: p.key.root % 12,
        mode: p.key.mode,
    };
    match (audio, prior) {
        (None, None) => None,
        (Some((k, a)), None) => Some(est(k, a, KeySource::Detected)),
        (None, Some(p)) => Some(est(name_key(&p), name_conf(&p), KeySource::Filename)),
        (Some((k, a)), Some(p)) => {
            let n = name_conf(&p);
            Some(if matches_prior(k, &p) && a >= AUDIO_SUPPORT {
                let key = Key {
                    root: k.root,
                    mode: k.mode.or(p.key.mode),
                };
                est(key, 1.0 - (1.0 - a) * (1.0 - n), KeySource::Both)
            } else if matches_prior(k, &p) {
                // Agreement too weak to count (often made by the name
                // bonus itself): the name alone.
                est(name_key(&p), n, KeySource::Filename)
            } else if a > n {
                est(k, a * (1.0 - n), KeySource::Detected)
            } else {
                // The name outweighed a disagreeing audio key: keep the
                // audio's key so the UI can show both.
                KeyEstimate {
                    audio: Some(k),
                    ..est(name_key(&p), n * (1.0 - a), KeySource::Filename)
                }
            })
        }
    }
}

/// `key` agrees with the prior: same root, and same mode when both have one.
pub(crate) fn matches_prior(key: Key, prior: &KeyPrior) -> bool {
    key.root % 12 == prior.key.root % 12
        && (prior.key.mode.is_none() || key.mode.is_none() || prior.key.mode == key.mode)
}

/// Strength of a filename key.
pub(crate) fn name_conf(p: &KeyPrior) -> f32 {
    if p.explicit {
        NAME_CONF_EXPLICIT
    } else {
        NAME_CONF_BARE
    }
}

/// Time-averaged Fujishima chroma (all frames weighted equally); `None`
/// without energy in the chroma range.
fn plain_chroma(spec: &Spectrogram) -> Option<[f32; N_CHROMA]> {
    let map = ChromaMap::new(spec.sample_rate, spec.n_fft);
    let (mut acc, mut frame) = ([0.0f32; N_CHROMA], [0.0f32; N_CHROMA]);
    for power in spec.frames() {
        if map.frame(power, &mut frame) {
            acc.iter_mut().zip(&frame).for_each(|(a, f)| *a += f);
        }
    }
    has_energy(&acc).then_some(acc)
}

fn has_energy(chroma: &[f32; N_CHROMA]) -> bool {
    let sum: f32 = chroma.iter().sum();
    sum > 0.0 && sum.is_finite()
}

/// Linear ramp from 0 at `lo` to 1 at `hi`; NaN gives 0.
fn ramp(x: f32, lo: f32, hi: f32) -> f32 {
    let v = (x - lo) / (hi - lo);
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

/// Clamps to `0..=1`; non-finite gives 0.
fn finite01(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

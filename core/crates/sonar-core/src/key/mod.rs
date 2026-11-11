//! Musical key and note (`docs/ARCHITECTURE.md` §4, ADR-022).
//!
//! - **Key** ([`estimate_key`]): tonal chroma of the analysis window
//!   ([`chroma`]: sinusoidal peaks of the existing STFT, tuning-corrected)
//!   correlated with the 24 harmonic-expanded Temperley key profiles
//!   ([`profiles`]; Krumhansl–Schmuckler). Only **tonal** material gets a
//!   key: at least [`MIN_TONAL_RATIO`] of the band power in sinusoidal
//!   peaks, at least [`MIN_PITCH_CLASSES`] clearly present pitch classes
//!   (one note is not a key) and a best correlation of
//!   [`MIN_CORRELATION`]. Drums, noise, clicks and silence get none.
//! - **Note** ([`note`]): YIN pitch of **one-shots** (loops have melodies,
//!   not a note). A one-shot with a note has the note's pitch class as its
//!   key, without a mode (a single note does not imply major or minor),
//!   so the key filter finds bass/808 hits tuned to the song's key.
//! - **Filename prior** (optional; the eval hides names): a key in the name
//!   adds [`NAME_BONUS`] to the correlation of matching keys (both modes
//!   when the name gives none), lends its mode to a note on the same root,
//!   and is the value (source [`KeySource::Filename`], confidence
//!   [`NAME_CONF_EXPLICIT`] / [`NAME_CONF_BARE`]) when the audio has no key
//!   or a weaker one than [`WEAK_CONFIDENCE`].
//!
//! [`key_and_note`] applies the loop/one-shot gating used by the `dsp`
//! job stage. Pure, deterministic, never panics, finite outputs.

pub mod chroma;
pub mod note;
pub mod profiles;

#[cfg(test)]
mod tests;

use serde::Serialize;

pub use chroma::{tonal_chroma, TonalChroma};
pub use note::{Note, NoteTracker};

use crate::classify::{Kind, KindResult, KindSource};
use crate::dsp::chroma::N_CHROMA;
use crate::dsp::Features;
use crate::text::{Key, KeyPrior};
use crate::Result;

/// Version of the key/note definitions; bump when any changes so stored
/// keys and notes are recomputed.
pub const KEY_VERSION: u32 = 1;
/// Minimum share of the band power in sinusoidal peaks for a key.
pub const MIN_TONAL_RATIO: f32 = 0.5;
/// A pitch class counts as present at this fraction of the strongest.
pub const PRESENT_FRACTION: f32 = 0.25;
/// Minimum present pitch classes for a key.
pub const MIN_PITCH_CLASSES: usize = 3;
/// Minimum correlation of the best key.
pub const MIN_CORRELATION: f32 = 0.5;
/// Correlation margin over the runner-up at which confidence saturates.
pub const MARGIN_REF: f32 = 0.1;
/// Added to the correlation of keys matching the filename key.
pub const NAME_BONUS: f32 = 0.1;
/// Audio keys below this confidence yield to a filename key.
pub const WEAK_CONFIDENCE: f32 = 0.2;
/// Confidence of a key taken from an explicit (`Am`, `F# minor`) name.
pub const NAME_CONF_EXPLICIT: f32 = 0.6;
/// Confidence of a key taken from a bare note letter (`Loop_120_C`).
pub const NAME_CONF_BARE: f32 = 0.4;

/// Where a [`KeyEstimate`] came from (`analysis.key_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// Measured from the audio (a filename key may have decided between
    /// close candidates or lent its mode).
    Detected,
    /// The filename key: the audio had none or a weak one.
    Filename,
}

impl KeySource {
    /// Value stored in `analysis.key_source`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Filename => "filename",
        }
    }
}

/// Key of one file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct KeyEstimate {
    /// Tonic and mode (`mode: None` for a single note's pitch class or a
    /// mode-less filename key).
    pub key: Key,
    /// `0..=1`.
    pub confidence: f32,
    /// What decided the value.
    pub source: KeySource,
}

/// Key and note of one file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct KeyNote {
    /// Key, if any.
    pub key: Option<KeyEstimate>,
    /// Note (one-shots only).
    pub note: Option<Note>,
}

/// Key of `features`' analysis window from its chroma, with an optional
/// filename key (see the module docs). `None` when the audio is not tonal
/// and there is no prior.
pub fn estimate_key(features: &Features, prior: Option<KeyPrior>) -> Option<KeyEstimate> {
    let detected = detect_key(&tonal_chroma(&features.spectrogram), prior);
    with_prior(detected, prior)
}

/// Key and note of one file as the `dsp` stage stores them: one-shots get
/// a note when they have a stable pitch (and then the note's pitch class as
/// key), everything else a chroma key when tonal; silent files (fallback
/// kind) only the filename key. `samples` are the analysis window at the
/// tracker's rate.
///
/// # Errors
/// Only internal FFT failures of the note tracker.
pub fn key_and_note(
    kind: &KindResult,
    features: &Features,
    samples: &[f32],
    tracker: &mut NoteTracker,
    prior: Option<KeyPrior>,
) -> Result<KeyNote> {
    if kind.source == KindSource::Fallback {
        return Ok(KeyNote {
            key: with_prior(None, prior),
            note: None,
        });
    }
    let note = if kind.kind == Kind::OneShot {
        tracker.detect(samples)?
    } else {
        None
    };
    let key = match note {
        Some(n) => with_prior(Some(note_key(&n, prior)), prior),
        None => estimate_key(features, prior),
    };
    Ok(KeyNote { key, note })
}

/// The pitch class of a note as a mode-less key; a filename key on the same
/// root lends its mode.
fn note_key(note: &Note, prior: Option<KeyPrior>) -> KeyEstimate {
    let root = note.pitch_class();
    let (mode, confidence) = match prior.filter(|p| p.key.root % 12 == root) {
        Some(p) => (p.key.mode, raise(note.confidence, name_conf(&p))),
        None => (None, note.confidence),
    };
    KeyEstimate {
        key: Key { root, mode },
        confidence,
        source: KeySource::Detected,
    }
}

/// Audio key from a tonal chroma (prior only weights candidates).
fn detect_key(tc: &TonalChroma, prior: Option<KeyPrior>) -> Option<KeyEstimate> {
    if !is_tonal(tc) {
        return None;
    }
    let mut scores = profiles::key_scores(&tc.chroma);
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
    if r.is_nan() || r < MIN_CORRELATION {
        return None;
    }
    let margin = ((r - runner_up) / MARGIN_REF).clamp(0.0, 1.0);
    let confidence = (r.min(1.0) * margin).clamp(0.0, 1.0);
    let key = profiles::key_of_index(best);
    let confidence = match prior {
        Some(p) if matches_prior(key, &p) => raise(confidence, name_conf(&p)),
        _ => confidence,
    };
    confidence.is_finite().then_some(KeyEstimate {
        key,
        confidence,
        source: KeySource::Detected,
    })
}

/// Tonality gate (see the module docs).
pub fn is_tonal(tc: &TonalChroma) -> bool {
    let present = present_classes(&tc.chroma);
    tc.tonal_ratio >= MIN_TONAL_RATIO && present >= MIN_PITCH_CLASSES
}

/// Pitch classes of `chroma` at least [`PRESENT_FRACTION`] of the strongest.
pub fn present_classes(chroma: &[f32; N_CHROMA]) -> usize {
    let max = chroma.iter().copied().fold(0.0f32, f32::max);
    chroma
        .iter()
        .filter(|&&c| max > 0.0 && c >= PRESENT_FRACTION * max)
        .count()
}

/// Detected key, or the filename key when detection is missing/weak.
fn with_prior(detected: Option<KeyEstimate>, prior: Option<KeyPrior>) -> Option<KeyEstimate> {
    match (detected, prior) {
        (Some(k), _) if k.confidence >= WEAK_CONFIDENCE => Some(k),
        (_, Some(p)) => Some(KeyEstimate {
            key: Key {
                root: p.key.root % 12,
                mode: p.key.mode,
            },
            confidence: name_conf(&p),
            source: KeySource::Filename,
        }),
        (k, None) => k,
    }
}

/// `key` agrees with the prior: same root and mode (any mode when the name
/// has none).
fn matches_prior(key: Key, prior: &KeyPrior) -> bool {
    key.root == prior.key.root % 12 && (prior.key.mode.is_none() || prior.key.mode == key.mode)
}

fn name_conf(p: &KeyPrior) -> f32 {
    if p.explicit {
        NAME_CONF_EXPLICIT
    } else {
        NAME_CONF_BARE
    }
}

/// Combines two independent confidences (noisy-or).
fn raise(a: f32, b: f32) -> f32 {
    (1.0 - (1.0 - a.clamp(0.0, 1.0)) * (1.0 - b.clamp(0.0, 1.0))).clamp(0.0, 1.0)
}

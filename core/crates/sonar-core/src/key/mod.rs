//! Musical key and note (`docs/ARCHITECTURE.md` §4, ADR-022, ADR-041).
//!
//! - **Key** ([`estimate_key`]): tonal chroma of the analysis window
//!   ([`chroma`]: sinusoidal peaks of the existing STFT, tuning-corrected)
//!   correlated with the 24 harmonic-expanded Temperley key profiles
//!   ([`profiles`]; Krumhansl–Schmuckler). Every non-silent file gets its
//!   best candidate; how tonal the material is (tonal ratio, present pitch
//!   classes, correlation) grades the confidence instead of gating the
//!   value ([`fuse`]), so drums and noise get a key at a confidence near 0.
//! - **Note** ([`note`]): YIN pitch of **one-shots** (loops have melodies,
//!   not a note). A one-shot with a note has the note's pitch class as its
//!   key candidate, without a mode (a single note does not imply major or
//!   minor), so the key filter finds bass/808 hits tuned to the song's key.
//! - **Filename prior** (optional; the eval hides names): fused with the
//!   audio candidate as evidence ([`fuse`]): agreement raises, disagreement
//!   lowers the confidence; [`KeySource`] records audio / name / both.
//!
//! [`key_and_note`] applies the loop/one-shot gating used by the `dsp`
//! job stage; silent audio gets no key at all (not even the name's).
//! Pure, deterministic, never panics, finite outputs.

pub mod chroma;
pub mod fuse;
pub mod note;
pub mod profiles;

#[cfg(test)]
mod tests;

use serde::Serialize;

pub use chroma::{tonal_chroma, TonalChroma};
pub use fuse::{
    present_classes, tonality, AUDIO_SUPPORT, CORRELATION_FLOOR, MARGIN_REF, MIN_CORRELATION,
    MIN_PITCH_CLASSES, MIN_TONAL_RATIO, NAME_BONUS, NAME_CONF_BARE, NAME_CONF_EXPLICIT,
    PRESENT_FRACTION, TONAL_RATIO_FLOOR,
};
pub use note::{Note, NoteTracker};

use crate::classify::{Kind, KindResult};
use crate::dsp::Features;
use crate::text::{Key, KeyPrior};
use crate::Result;

/// Version of the key/note definitions; bump when any changes so stored
/// keys and notes are recomputed. 2 (T10.1): more filename key priors.
/// 3 (T10.2): fused key with graded confidence, source `both`, note
/// confidence falls with |cents|.
pub const KEY_VERSION: u32 = 3;

/// Where a [`KeyEstimate`] came from (`analysis.key_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// The audio alone (no filename key, or the audio outweighed a
    /// disagreeing one).
    Detected,
    /// The filename key (no audio candidate, it outweighed a disagreeing
    /// one, or the agreeing audio is too weak to count as support).
    Filename,
    /// Audio and filename agree.
    Both,
}

impl KeySource {
    /// Value stored in `analysis.key_source`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Filename => "filename",
            Self::Both => "both",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "detected" => Some(Self::Detected),
            "filename" => Some(Self::Filename),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

/// Key of one file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct KeyEstimate {
    /// Tonic and mode (`mode: None` for a single note's pitch class or a
    /// mode-less filename key).
    pub key: Key,
    /// `0..=1` (graded: near 0 for drums and noise).
    pub confidence: f32,
    /// What decided the value.
    pub source: KeySource,
}

/// Key and note of one file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct KeyNote {
    /// Key, if any (`None` only for silence or no energy at all).
    pub key: Option<KeyEstimate>,
    /// Note (one-shots only).
    pub note: Option<Note>,
}

/// Fused key of `features`' analysis window (chroma candidate) and an
/// optional filename key. `None` only without audio energy in the chroma
/// range and without a prior.
pub fn estimate_key(features: &Features, prior: Option<KeyPrior>) -> Option<KeyEstimate> {
    let spec = &features.spectrogram;
    fuse::fuse(
        fuse::chroma_candidate(&tonal_chroma(spec), spec, prior),
        prior,
    )
}

/// Key and note of one file as the `dsp` stage stores them: one-shots get
/// a note when they have a stable pitch (and then the note's pitch class as
/// key candidate), everything else the chroma candidate; either is fused
/// with the filename key. Silent audio gets neither. `samples` are the
/// analysis window at the tracker's rate.
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
    if kind.scores.silent {
        return Ok(KeyNote::default());
    }
    let note = if kind.kind == Kind::OneShot {
        tracker.detect(samples)?
    } else {
        None
    };
    let key = match note {
        Some(n) => fuse::fuse(Some(note_candidate(&n)), prior),
        None => estimate_key(features, prior),
    };
    Ok(KeyNote { key, note })
}

/// The pitch class of a note as a mode-less key candidate.
fn note_candidate(note: &Note) -> (Key, f32) {
    let key = Key {
        root: note.pitch_class(),
        mode: None,
    };
    (key, note.confidence)
}

/// All three ADR-022 tonality conditions hold (the [`tonality`] factor is 1
/// for any best correlation ≥ [`MIN_CORRELATION`]).
pub fn is_tonal(tc: &TonalChroma) -> bool {
    tc.tonal_ratio >= MIN_TONAL_RATIO && present_classes(&tc.chroma) >= MIN_PITCH_CLASSES
}

//! Loop vs one-shot decision: a fixed logistic combination of the
//! [`KindScores`] evidence plus an optional filename hint.
//!
//! `z = W_PERIODICITY·(P_eff − P_CENTER) + duration_term + tail_term
//! (+ ±W_HINT)`, `p_loop = 1 / (1 + e^(−z))`, Loop iff `z > 0`, where
//! - `P_eff = periodicity · min(1, pulse_db / PULSE_REF_DB)`: rhythm is the
//!   primary cue and must be strong in absolute terms (steady noise has a
//!   faint, unstructured flux; a steady tone almost none);
//! - `duration_term = clamp(DUR_SLOPE·log2(duration / DUR_CENTER_S),
//!   DUR_MIN, DUR_MAX)`: sub-second files are almost never loops, long
//!   files lean loop but cannot outvote missing rhythm;
//! - `tail_term = clamp((tail_db − TAIL_CENTER_DB) / TAIL_SCALE_DB,
//!   TAIL_MIN, TAIL_MAX)`: a sound that has died out well before its end
//!   leans one-shot; a sustained end is no evidence either way
//!   (`TAIL_MAX` = 0: steady tones are sustained too); 0 when the tail is
//!   unknown (truncated window).
//!
//! Weights are hand-set from the fixture distributions (`docs/DECISIONS.md`
//! ADR-020), so `p_loop` is only *calibrated-ish*: ≈ 0.5 means the evidence
//! is balanced, ≥ 0.95 that every cue agrees. The filename hint (±2) is
//! worth less than clear rhythm (`P_eff ≥ 0.5` → rhythm term ≥ +3.6) or a
//! short decaying hit (≈ −4 or lower), so it only decides ambiguous
//! sounds such as a long rhythm-less pad loop.

use serde::Serialize;

use super::scores::{kind_scores, KindScores};
use crate::dsp::Features;

/// Weight of the effective periodicity.
pub const W_PERIODICITY: f32 = 12.0;
/// Effective periodicity at which rhythm alone is neutral.
pub const P_CENTER: f32 = 0.2;
/// Pulse strength (dB per band) at which periodicity counts fully.
pub const PULSE_REF_DB: f32 = 1.0;
/// Duration at which the duration term is 0 (s).
pub const DUR_CENTER_S: f32 = 2.0;
/// Duration term per octave of duration.
pub const DUR_SLOPE: f32 = 1.5;
/// Lower clamp of the duration term.
pub const DUR_MIN: f32 = -3.0;
/// Upper clamp of the duration term.
pub const DUR_MAX: f32 = 1.0;
/// Tail level (dB re peak) at which the tail term is 0.
pub const TAIL_CENTER_DB: f32 = -12.0;
/// dB of tail level per unit of the tail term.
pub const TAIL_SCALE_DB: f32 = 8.0;
/// Lower clamp of the tail term.
pub const TAIL_MIN: f32 = -1.0;
/// Upper clamp of the tail term.
pub const TAIL_MAX: f32 = 0.0;
/// Logit added (Loop) or subtracted (OneShot) by a filename hint.
pub const W_HINT: f32 = 2.0;

/// Loop or one-shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Rhythmic material meant to repeat in time with a tempo.
    Loop,
    /// A single event or sustained sound (hits, notes, tones, FX, noise).
    OneShot,
}

/// What decided a [`KindResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KindSource {
    /// The audio evidence (a hint, if any, agreed or was outweighed).
    Detected,
    /// The filename hint: it flipped an ambiguous audio decision, or the
    /// audio is silent.
    Filename,
    /// No evidence at all (silent/empty audio, no hint): `OneShot` with
    /// confidence 0.
    Fallback,
}

/// Loop/one-shot decision for one file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct KindResult {
    /// The decision.
    pub kind: Kind,
    /// Probability of `kind`: 0 for [`KindSource::Fallback`], otherwise
    /// in `0.5..=1`.
    pub confidence: f32,
    /// Probability of [`Kind::Loop`] (`0..=1`; 0.5 for a fallback).
    pub loop_probability: f32,
    /// What decided it.
    pub source: KindSource,
    /// The filename hint that was given.
    pub hint: Option<Kind>,
    /// Logit of the audio evidence alone (0 when silent).
    pub audio_logit: f32,
    /// The measured evidence.
    pub scores: KindScores,
}

/// Classifies the analysis window in `features` (from
/// [`crate::dsp::Analyzer::analyze`]) of a file whose **true** duration is
/// `duration_s` (the window may be capped at 30 s), with an optional
/// filename `hint` (see [`super::kind_hint`]). Never panics; every output
/// is finite. Silent or empty audio gives the hint's kind (source
/// `Filename`) or a `Fallback` one-shot with confidence 0.
pub fn classify_kind(features: &Features, duration_s: f64, hint: Option<Kind>) -> KindResult {
    let scores = kind_scores(features, duration_s);
    let hint_logit = match hint {
        Some(Kind::Loop) => W_HINT,
        Some(Kind::OneShot) => -W_HINT,
        None => 0.0,
    };
    if scores.silent {
        return match hint {
            Some(_) => decide(hint_logit, 0.0, hint, KindSource::Filename, scores),
            None => KindResult {
                kind: Kind::OneShot,
                confidence: 0.0,
                loop_probability: 0.5,
                source: KindSource::Fallback,
                hint,
                audio_logit: 0.0,
                scores,
            },
        };
    }
    let audio = audio_logit(&scores);
    let z = audio + hint_logit;
    let source = if hint.is_some() && (z > 0.0) != (audio > 0.0) {
        KindSource::Filename
    } else {
        KindSource::Detected
    };
    decide(z, audio, hint, source, scores)
}

/// Logit of the audio evidence (see the module docs).
pub fn audio_logit(s: &KindScores) -> f32 {
    let p_eff = s.periodicity * (s.pulse_db / PULSE_REF_DB).clamp(0.0, 1.0);
    let rhythm = W_PERIODICITY * (p_eff - P_CENTER);
    let dur = s.duration_s.max(1e-3) / DUR_CENTER_S;
    let duration = (DUR_SLOPE * dur.log2()).clamp(DUR_MIN, DUR_MAX);
    let tail = s.tail_db.map_or(0.0, |t| {
        ((t - TAIL_CENTER_DB) / TAIL_SCALE_DB).clamp(TAIL_MIN, TAIL_MAX)
    });
    let z = rhythm + duration + tail;
    if z.is_finite() {
        z
    } else {
        0.0
    }
}

fn decide(
    z: f32,
    audio_logit: f32,
    hint: Option<Kind>,
    source: KindSource,
    scores: KindScores,
) -> KindResult {
    let p = 1.0 / (1.0 + (-z).exp());
    let kind = if z > 0.0 { Kind::Loop } else { Kind::OneShot };
    let confidence = if kind == Kind::Loop { p } else { 1.0 - p };
    KindResult {
        kind,
        confidence,
        loop_probability: p,
        source,
        hint,
        audio_logit,
        scores,
    }
}

//! Loop vs one-shot decision: a fixed logistic combination of the
//! [`KindScores`] evidence plus an optional filename hint.
//!
//! `z = W_PERIODICITY·(P_eff − P_CENTER) + duration_term + tail_term
//! + long_term + events_term (+ ±W_HINT)`, `p_loop = 1 / (1 + e^(−z))`,
//! Loop iff `z > 0`, where
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
//!   unknown (truncated window). Exception (T10.7): a file of at least
//!   [`SUSTAIN_MIN_S`] (two 4/4 bars at 120 BPM) with
//!   [`PART_MIN_ONSETS`] onsets or more whose tail is within
//!   [`SUSTAIN_DB`] of its peak gets `+W_SUSTAIN` instead — a loop is cut
//!   on the bar grid while it sounds (it must join its own start), while
//!   one-shots that long (impacts, transitions, reverse FX) decay to their
//!   end (silver/golden: every labeled one-shot ≥ 4 s has a tail below
//!   −34 dB). The onset gate keeps a single held note or pad a one-shot;
//! - `long_term = W_LONG` when the file outlasts the 30 s analysis window
//!   and the window holds [`PART_MIN_ONSETS`] onsets or more (a single
//!   sustained sound — drone, pad — is not a part played in time), else
//!   0: one-shots that long are rare in sample libraries (none of the 195
//!   labeled one-shots of the silver pack exceeds 12 s), while
//!   construction-kit stems and full vocal takes — tempo-locked parts
//!   whose onsets (sung syllables, pads) give only weak periodicity — are
//!   minutes long (T10.7). With the duration term (+1) it outweighs the
//!   rhythm term of a window with no measurable pulse (−2.4), so such a
//!   long multi-event file leans loop;
//! - `events_term = W_EVENTS` when the window holds [`EVENTS_MIN_ONSETS`]
//!   onsets or more and a pulse of at least [`PULSE_REF_DB`], else 0:
//!   several strong, distinct attacks are a played phrase (a fill, a
//!   short loop) rather than one hit, even when the window is too short
//!   for two beat periods and periodicity is unmeasurable — the onset
//!   rate / event density of rhythm descriptor sets (Bello et al. 2005 for
//!   the onsets; e.g. the `OnsetRate` descriptor of Bogdanov et al.,
//!   "Essentia", ISMIR 2013 — concept only) (T10.7).
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
/// Logit added when the file outlasts the analysis window (see the module
/// docs).
pub const W_LONG: f32 = 1.5;
/// Onset peaks a window needs to count as a part rather than one sustained
/// sound (gates [`W_LONG`] and [`W_SUSTAIN`]).
pub const PART_MIN_ONSETS: u32 = 2;
/// Shortest file whose sustained end is loop evidence (s).
pub const SUSTAIN_MIN_S: f32 = 4.0;
/// A tail at least this loud (dB re peak) is sustained.
pub const SUSTAIN_DB: f32 = -6.0;
/// Tail term of a sustained end (replaces the clamped tail term).
pub const W_SUSTAIN: f32 = 1.5;
/// Onset peaks that make a window a phrase of several attacks …
pub const EVENTS_MIN_ONSETS: u32 = 4;
/// … and the logit they add (with a pulse of at least [`PULSE_REF_DB`]).
pub const W_EVENTS: f32 = 1.0;
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
    let part = s.onsets >= PART_MIN_ONSETS;
    let tail = s.tail_db.map_or(0.0, |t| {
        if part && s.duration_s >= SUSTAIN_MIN_S && t >= SUSTAIN_DB {
            W_SUSTAIN
        } else {
            ((t - TAIL_CENTER_DB) / TAIL_SCALE_DB).clamp(TAIL_MIN, TAIL_MAX)
        }
    });
    let long = if s.truncated && part { W_LONG } else { 0.0 };
    let events = if s.onsets >= EVENTS_MIN_ONSETS && s.pulse_db >= PULSE_REF_DB {
        W_EVENTS
    } else {
        0.0
    };
    let z = rhythm + duration + tail + long + events;
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

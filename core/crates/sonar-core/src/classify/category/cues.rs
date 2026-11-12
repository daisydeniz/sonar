//! The DSP evidence the category heuristic reads: a handful of named
//! scalars taken from the raw aspect vectors (`docs/ARCHITECTURE.md` §5,
//! ADR-019), the loop/one-shot decision and the true duration. Both the
//! `dsp` job stage (fresh vectors) and the `embed` stage (vectors read back
//! from the vector store, kind and duration from SQLite) build them the
//! same way, so both stages see the same heuristic.

use serde::Serialize;

use crate::classify::Kind;
use crate::dsp::aspects::{amplitude, pitch, spectrum, AspectVectors};

/// Named DSP cues of one file. Every field is finite.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct DspCues {
    /// Probability that the file is a loop (0.5 = unknown).
    pub p_loop: f32,
    /// `log2` of the true duration (s), clamped to `[-6, 6]`.
    pub log2_dur: f32,
    /// Spectral centroid, `log2(Hz / 1 kHz)`, mean over active frames.
    pub centroid: f32,
    /// Spectral centroid std over active frames (octaves).
    pub centroid_std: f32,
    /// 85 % roll-off, `log2(Hz / 1 kHz)`, mean.
    pub rolloff: f32,
    /// Spectral flatness (dB, `-60..=0`), mean.
    pub flatness: f32,
    /// Low-band level: mean of the 3 lowest band groups minus the average
    /// band level (dB).
    pub low: f32,
    /// High-band level: mean of the 4 highest band groups minus the average
    /// (dB).
    pub high: f32,
    /// Voiced fraction × mean YIN confidence (`0..=1`): how pitched.
    pub tonal: f32,
    /// Median f0, octaves from A4 (0 when unvoiced).
    pub f0: f32,
    /// `log10` attack time (s).
    pub attack: f32,
    /// `log10` decay time (s).
    pub decay: f32,
    /// `log10` effective duration (s).
    pub effective: f32,
    /// Crest factor (dB).
    pub crest: f32,
    /// Silent window: no audio evidence at all (CLAP is ignored too).
    pub silent: bool,
    /// No aspect vectors (DSP failed or not run): the DSP score is neutral,
    /// CLAP may still decide.
    pub missing: bool,
}

/// Low band groups averaged into [`DspCues::low`].
const LOW_GROUPS: std::ops::Range<usize> = 0..3;
/// High band groups averaged into [`DspCues::high`].
const HIGH_GROUPS: std::ops::Range<usize> = 12..16;
/// Gated loudness at or below this (dBFS) means the window is silent.
const SILENT_LOUDNESS: f32 = -99.0;

impl DspCues {
    /// Cues from raw aspect vectors, the loop/one-shot decision (`None` =
    /// unknown) with its confidence, and the true duration (s). Missing or
    /// short vectors give neutral values; never panics.
    pub fn new(
        aspects: &AspectVectors,
        kind: Option<Kind>,
        kind_conf: Option<f32>,
        duration_s: f64,
    ) -> Self {
        let at = |v: &[f32], i: usize| v.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0);
        let s = &aspects.spectrum;
        let p = &aspects.pitch;
        let a = &aspects.amplitude;
        let mean = |r: std::ops::Range<usize>| {
            let n = r.len() as f32;
            r.map(|i| at(s, i)).sum::<f32>() / n
        };
        let conf = kind_conf
            .filter(|c| c.is_finite())
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);
        let p_loop = match kind {
            Some(Kind::Loop) => conf,
            Some(Kind::OneShot) => 1.0 - conf,
            None => 0.5,
        };
        let dur = duration_s as f32;
        let log2_dur = if dur.is_finite() && dur > 0.0 {
            dur.log2().clamp(-6.0, 6.0)
        } else {
            0.0
        };
        let missing = s.len() < spectrum::SPECTRUM_DIM
            || p.len() < pitch::PITCH_DIM
            || a.len() < amplitude::AMPLITUDE_DIM;
        let silent = !missing && at(a, amplitude::LOUDNESS) <= SILENT_LOUDNESS;
        Self {
            p_loop,
            log2_dur,
            centroid: at(s, spectrum::CENTROID),
            centroid_std: at(s, spectrum::CENTROID + 1),
            rolloff: at(s, spectrum::ROLLOFF_IDX),
            flatness: at(s, spectrum::FLATNESS),
            low: mean(LOW_GROUPS),
            high: mean(HIGH_GROUPS),
            tonal: (at(p, pitch::VOICED) * at(p, pitch::CONFIDENCE)).clamp(0.0, 1.0),
            f0: at(p, pitch::F0),
            attack: at(a, amplitude::ATTACK),
            decay: at(a, amplitude::DECAY),
            effective: at(a, amplitude::EFFECTIVE),
            crest: at(a, amplitude::CREST),
            silent,
            missing,
        }
    }
}

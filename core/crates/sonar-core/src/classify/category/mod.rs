//! Category classifier (T5.3, ADR-028): which sound source a sample is
//! (`kick`, `hat`, `bass`, `fx`, ...; [`taxonomy`], `docs/categories.md`).
//!
//! Evidence, fused as logits (late fusion of independent scores, then one
//! softmax):
//! - **CLAP zero-shot** ([`clap`]): mean cosine of the file's audio
//!   embedding to each category's text prompts (Wu et al. 2023). Only when
//!   an embedding exists (the `embed` stage, ADR-027).
//! - **DSP** ([`dsp`]): a naive-Bayes score over named cues from the aspect
//!   vectors, the loop/one-shot decision and the duration ([`cues`]).
//! - **Name** ([`names`]): a category word in the file name or its nearest
//!   folders adds [`W_NAME`] to that category *after* calibration, so it
//!   decides ambiguous audio but cannot outvote clear audio evidence.
//!
//! `z = W_CLAP·clap + W_DSP·dsp` (or `W_DSP_ALONE·dsp` without CLAP);
//! `p = softmax(z / T + name)`; the category is `argmax p`, its confidence
//! `max p`. `T` ([`T_FUSED`], [`T_DSP`]) is fitted by temperature scaling
//! (Guo et al. 2017, [`calibrate`]) on held-out silver data, so the
//! confidence of the audio decision is calibrated there; the name bonus is
//! not (the eval hides names). Source `filename` when the name changed the
//! decision (or the audio is silent), else `detected`.
//!
//! Pure functions: deterministic, finite outputs, never panic.

pub mod calibrate;
pub mod clap;
pub mod cues;
pub mod dsp;
pub mod names;
pub mod taxonomy;

#[cfg(test)]
mod tests;

use serde::Serialize;

pub use clap::{ClapScorer, ClapScores, PromptBank};
pub use cues::DspCues;
pub use names::category_hint;
pub use taxonomy::{Category, CategoryInfo, N_CATEGORIES, TAXONOMY};

/// Version of the category definitions (taxonomy, prompts, profiles,
/// weights); bump with the `dsp` and `embed` stage versions so stored
/// categories are recomputed.
pub const CATEGORY_VERSION: u32 = 1;

/// Weight of the CLAP best-prompt cosine (ADR-028: grid search on the
/// silver training half, held-out top-1 flat within ±0.6 points for
/// `W_CLAP / W_DSP` in 33..50).
pub const W_CLAP: f32 = 10.0;
/// Weight of the DSP naive-Bayes score next to CLAP.
pub const W_DSP: f32 = 0.2;
/// Weight of the DSP score without CLAP (its temperature does the scaling).
pub const W_DSP_ALONE: f32 = 1.0;
/// Temperature of the fused (CLAP + DSP) logits, fitted by NLL on the
/// silver training half (held-out ECE 2.1 % → 1.5 %).
pub const T_FUSED: f32 = 0.96;
/// Temperature of the DSP-only logits, fitted likewise (held-out ECE
/// 14.0 % → 6.3 %).
pub const T_DSP: f32 = 2.69;
/// Calibrated-logit bonus of the category a file's name states: odds × e³
/// ≈ 20, so the name wins unless the audio favours another category by more
/// than 20 : 1.
pub const W_NAME: f32 = 3.0;

/// What decided a [`CategoryResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CategorySource {
    /// The audio (the name, if any, agreed or was outweighed).
    Detected,
    /// The name: it changed the audio's decision, or the audio is silent.
    Filename,
    /// No evidence (silent audio, no name): no category.
    Fallback,
}

impl CategorySource {
    /// Stored form (`analysis.category_source`), `None` for a fallback.
    pub fn as_db(self) -> Option<&'static str> {
        match self {
            Self::Detected => Some("detected"),
            Self::Filename => Some("filename"),
            Self::Fallback => None,
        }
    }
}

/// Category decision for one file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CategoryResult {
    /// The category (`None` only for [`CategorySource::Fallback`]).
    pub category: Option<Category>,
    /// Probability of `category` (0 for a fallback).
    pub confidence: f32,
    /// What decided it.
    pub source: CategorySource,
    /// Category stated by the name, if any.
    pub hint: Option<Category>,
    /// CLAP evidence was used.
    pub clap: bool,
    /// Best category of the audio alone (`None` when silent).
    pub audio: Option<Category>,
    /// Calibrated audio-only logits `z / T` (eval: calibration and
    /// diagnostics).
    #[serde(skip)]
    pub logits: [f32; N_CATEGORIES],
}

impl CategoryResult {
    /// The three most probable categories (with the name bonus).
    pub fn top3(&self) -> [(Category, f32); 3] {
        let mut p = [0.0f32; N_CATEGORIES];
        calibrate::softmax(&with_name(&self.logits, self.hint), 1.0, &mut p);
        let mut idx: Vec<usize> = (0..N_CATEGORIES).collect();
        idx.sort_by(|&a, &b| p[b].total_cmp(&p[a]).then(a.cmp(&b)));
        [0, 1, 2].map(|k| (Category::ALL[idx[k]], p[idx[k]]))
    }
}

/// Uncalibrated audio logits `z` and the temperature that calibrates them.
pub fn audio_logits(cues: &DspCues, clap: Option<&ClapScores>) -> ([f32; N_CATEGORIES], f32) {
    let d = dsp::dsp_scores(cues);
    let mut z = [0.0f32; N_CATEGORIES];
    match clap {
        Some(c) => {
            for i in 0..N_CATEGORIES {
                z[i] = W_CLAP * c[i] + W_DSP * d[i];
            }
            (z, T_FUSED)
        }
        None => {
            for i in 0..N_CATEGORIES {
                z[i] = W_DSP_ALONE * d[i];
            }
            (z, T_DSP)
        }
    }
}

fn with_name(logits: &[f32; N_CATEGORIES], hint: Option<Category>) -> [f32; N_CATEGORIES] {
    let mut z = *logits;
    if let Some(h) = hint {
        z[h.index()] += W_NAME;
    }
    z
}

fn argmax(z: &[f32]) -> usize {
    let mut best = 0;
    for (i, v) in z.iter().enumerate() {
        if *v > z[best] {
            best = i;
        }
    }
    best
}

/// Classifies one file from its DSP cues, optional CLAP scores and
/// optional name hint (see the module docs). A silent file gets the hint
/// (source `filename`) or no category (`Fallback`, confidence 0).
pub fn classify_category(
    cues: &DspCues,
    clap: Option<&ClapScores>,
    hint: Option<Category>,
) -> CategoryResult {
    // No audio evidence: silence, or neither DSP cues nor CLAP scores.
    let silent = cues.silent || (cues.missing && clap.is_none());
    let mut logits = [0.0f32; N_CATEGORIES];
    if !silent {
        let (z, t) = audio_logits(cues, clap);
        for (l, v) in logits.iter_mut().zip(z) {
            *l = if v.is_finite() { v / t } else { 0.0 };
        }
    }
    let audio = (!silent).then(|| Category::ALL[argmax(&logits)]);
    if silent && hint.is_none() {
        return CategoryResult {
            category: None,
            confidence: 0.0,
            source: CategorySource::Fallback,
            hint,
            clap: false,
            audio,
            logits,
        };
    }
    let z = with_name(&logits, hint);
    let mut p = [0.0f32; N_CATEGORIES];
    calibrate::softmax(&z, 1.0, &mut p);
    let best = argmax(&z);
    let category = Category::ALL[best];
    let source = if silent || audio != Some(category) {
        CategorySource::Filename
    } else {
        CategorySource::Detected
    };
    CategoryResult {
        category: Some(category),
        confidence: p[best],
        source,
        hint,
        clap: clap.is_some(),
        audio,
        logits,
    }
}

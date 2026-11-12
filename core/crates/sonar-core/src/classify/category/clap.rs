//! CLAP zero-shot category scores (Wu et al. 2023, arXiv:2211.06687, §4.3:
//! zero-shot audio classification by cosine similarity between the audio
//! embedding and text embeddings of class prompts).
//!
//! Each category has several prompts, one per typical sub-type
//! ([`super::TAXONOMY`]); a file's score for a category is its **best**
//! prompt's cosine. A heterogeneous class (`perc`: shakers, congas,
//! cowbells, ...) then matches any of its members instead of their blurred
//! average. On the silver set this beat averaging the prompt embeddings
//! (the CLIP ensembling of Radford et al. 2021, §3.1.4) by 2-3 points
//! held-out (ADR-028). A [`PromptBank`] is built once per job / eval run
//! (≈ 95 text-tower runs, ≈ 0.5 s), never per file.

use std::cell::RefCell;
use std::path::Path;

use crate::embed::{AudioPrep, EmbedOptions, EmbedPool, Embedding, TextEncoder};
use crate::{Error, Result};

use super::taxonomy::{Category, N_CATEGORIES, TAXONOMY};

thread_local! {
    static PREP: RefCell<AudioPrep> = RefCell::new(AudioPrep::new());
}

/// CLAP audio sessions plus the prompt bank: zero-shot scores of files
/// outside the job pipeline (`sonar classify`, `sonar eval`).
#[derive(Debug)]
pub struct ClapScorer {
    /// Audio sessions.
    pub pool: EmbedPool,
    /// Text embeddings of the taxonomy prompts.
    pub bank: PromptBank,
}

impl ClapScorer {
    /// Loads the audio pool and embeds the prompts.
    ///
    /// # Errors
    /// [`Error::ModelMissing`] / [`Error::Embed`]: no model, no CLAP
    /// support in this build, or a model fails to load.
    pub fn load(opts: &EmbedOptions) -> Result<Self> {
        let bank = PromptBank::from_encoder(&mut TextEncoder::new(opts)?)?;
        Ok(Self {
            pool: EmbedPool::new(opts)?,
            bank,
        })
    }

    /// Zero-shot scores of one file, decoded as the `embed` stage does
    /// (native rate → 48 kHz, first 10 s). Callable from many threads.
    ///
    /// # Errors
    /// Decode or model errors.
    pub fn scores(&self, path: &Path) -> Result<ClapScores> {
        Ok(self.scores_and_prompts(path, false)?.0)
    }

    /// [`ClapScorer::scores`] plus, if `prompts`, the cosine to every
    /// prompt ([`PromptBank::prompt_scores`] order).
    ///
    /// # Errors
    /// Decode or model errors.
    pub fn scores_and_prompts(
        &self,
        path: &Path,
        prompts: bool,
    ) -> Result<(ClapScores, Option<Vec<f32>>)> {
        let e = self.embed_file(path)?;
        Ok(self.scores_of(&e, prompts))
    }

    /// The CLAP audio embedding of one file, decoded as the `embed` stage
    /// does. Callable from many threads.
    ///
    /// # Errors
    /// Decode or model errors.
    pub fn embed_file(&self, path: &Path) -> Result<Embedding> {
        let prepared = PREP.with(|p| p.borrow_mut().prepare(path, None))?;
        self.pool.embed(prepared.features)
    }

    /// [`ClapScorer::scores_and_prompts`] of an embedding.
    pub fn scores_of(&self, e: &Embedding, prompts: bool) -> (ClapScores, Option<Vec<f32>>) {
        let all = prompts.then(|| self.bank.prompt_scores(e).iter().map(|p| p.2).collect());
        (self.bank.scores(e), all)
    }

    /// Drops this thread's decode state after a caught panic.
    pub fn reset_thread() {
        PREP.with(|p| p.borrow_mut().reset());
    }
}

/// Best prompt cosine of a file per category, in [`Category`] order.
pub type ClapScores = [f32; N_CATEGORIES];

/// Text embeddings of every taxonomy prompt.
#[derive(Clone)]
pub struct PromptBank {
    /// Every prompt with its category and embedding (taxonomy order).
    prompts: Vec<(Category, &'static str, Embedding)>,
}

impl std::fmt::Debug for PromptBank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptBank")
            .field("prompts", &self.prompts.len())
            .finish_non_exhaustive()
    }
}

impl PromptBank {
    /// Embeds every taxonomy prompt with `embed` (L2-normalised text
    /// embeddings).
    ///
    /// # Errors
    /// The first error of `embed`.
    pub fn build(mut embed: impl FnMut(&str) -> Result<Embedding>) -> Result<Self> {
        let mut prompts = Vec::new();
        for info in &TAXONOMY {
            for &p in info.prompts {
                prompts.push((info.category, p, embed(p)?));
            }
        }
        if prompts.is_empty() {
            return Err(Error::Embed {
                reason: "the category taxonomy has no prompts".into(),
            });
        }
        Ok(Self { prompts })
    }

    /// Builds the bank with a text tower.
    ///
    /// # Errors
    /// See [`PromptBank::build`].
    pub fn from_encoder(encoder: &mut TextEncoder) -> Result<Self> {
        Self::build(|p| encoder.embed(p))
    }

    /// Zero-shot scores of an L2-normalised audio embedding (best prompt
    /// cosine per category; −1 for a category without prompts). Non-finite
    /// cosines count as 0.
    pub fn scores(&self, audio: &Embedding) -> ClapScores {
        let mut out = [-1.0f32; N_CATEGORIES];
        for (c, _, e) in &self.prompts {
            let d: f32 = e.iter().zip(audio).map(|(a, b)| a * b).sum();
            let d = if d.is_finite() { d } else { 0.0 };
            let o = &mut out[c.index()];
            *o = o.max(d);
        }
        out
    }

    /// Cosine of `audio` to every single prompt, with the prompt's category
    /// and text (eval diagnostics).
    pub fn prompt_scores(&self, audio: &Embedding) -> Vec<(Category, &'static str, f32)> {
        self.prompts
            .iter()
            .map(|(c, p, e)| (*c, *p, e.iter().zip(audio).map(|(a, b)| a * b).sum()))
            .collect()
    }
}

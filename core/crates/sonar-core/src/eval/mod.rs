//! `sonar eval`: the measurements of `docs/EVAL.md` (T3.6).
//!
//! - [`dataset`] / [`golden`]: the three tiers (fixtures, silver, golden)
//!   and their labels;
//! - [`run`]: analysis with names hidden, the same function the `dsp` job
//!   stage stores kind / BPM / key / note with;
//! - [`metrics`]: loop/one-shot, BPM (+ octave errors), key (MIREX), note;
//! - [`category`]: category top-1, macro-F1, confusion, calibration (T5.3);
//! - [`similarity`]: P@10 / MRR of the aspect-weighted `similar` ranking
//!   (fixture families, golden `similar.jsonl`);
//! - [`text_search`] / [`text_queries`]: NDCG@10 / recall@50 of text,
//!   semantic and hybrid search (T5.4; fixture queries judged from truth,
//!   golden `text_queries.jsonl`);
//! - [`report`]: JSON + markdown with deltas;
//! - [`baseline`]: `eval/reports/BASELINE.md` and the regression rule.

pub mod baseline;
pub mod category;
mod csv;
pub mod dataset;
mod dump;
pub mod golden;
pub mod metrics;
pub mod report;
pub mod run;
pub mod similarity;
pub mod text_queries;
pub mod text_search;

use std::path::Path;

use crate::fixtures::{generate, FixtureOptions};
use crate::Result;

pub use baseline::{compare, Baseline, Comparison, Regression};
pub use dataset::{silver_dataset, Dataset, Tier};
pub use golden::{golden_dataset, init_template};
pub use report::EvalReport;
pub use run::{evaluate, evaluate_with, EvalOptions, TierReport};

/// Generates the fixture set (default seed; `include_long` adds the 310 s
/// file, which only costs generation time) into `dir` and returns its
/// dataset.
///
/// # Errors
/// Generation fails (disk full, unwritable `dir`).
pub fn fixtures_dataset(dir: &Path, include_long: bool) -> Result<Dataset> {
    let opts = FixtureOptions {
        include_long,
        ..FixtureOptions::default()
    };
    let manifest = generate(dir, &opts)?;
    Ok(dataset::fixtures_dataset(&manifest, dir))
}

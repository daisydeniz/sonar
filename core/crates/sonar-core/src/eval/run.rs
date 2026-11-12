//! Runs the analyzer over a [`Dataset`] and scores it.
//!
//! The analysis is [`analyze_window`], the function the `dsp` job stage
//! stores kind / BPM / key / note / category with, called with **no name
//! hint and no filename prior** and given only decoded samples: file and
//! folder names cannot influence a result. With [`EvalOptions::clap`] the
//! category is also fused with CLAP scores exactly as the `embed` stage does
//! ([`classify_category`]). Files run in parallel on the rayon pool, one
//! reused decoder/analyzer per thread.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::classify::category::audio_logits;
use crate::classify::category::clap::ClapScorer;
use crate::classify::{classify_category, Category, DspCues};
use crate::decode::DecodeOptions;
use crate::dsp::aspects::AspectVectors;
use crate::embed::{Embedding, TextEncoder};
use crate::jobs::work::{analyze_window, with_workers, NameHints};
use crate::quiet_panic;

use super::category::CategoryObs;
use super::dataset::{Dataset, Item, Tier};
use super::metrics::{Accumulator, Metrics, Observation};
use super::similarity::{self, Entry};
use super::text_queries::fixture_queries;
use super::text_search;

/// Result of one tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierReport {
    /// Which tier.
    pub tier: Tier,
    /// Where the files came from.
    pub source: String,
    /// Files analysed (those with at least one label).
    pub files: usize,
    /// Files seen without a usable label (skipped).
    pub unlabeled: usize,
    /// Decode/analysis errors on files that should decode.
    pub unexpected_errors: usize,
    /// Errors on files expected to fail (corrupt fixtures).
    pub expected_errors: usize,
    /// Labeled golden entries whose file is missing.
    pub missing_files: usize,
    /// Wall time of the analysis (s).
    pub wall_s: f64,
    /// The metrics.
    pub metrics: Metrics,
    /// First few problems (unexpected errors, missing files, bad cells).
    pub notes: Vec<String>,
}

/// Maximum number of lines kept in [`TierReport::notes`].
const MAX_NOTES: usize = 20;

/// One analysed file.
pub(crate) struct Analysed {
    pub obs: Observation,
    /// Raw aspect vectors (when similarity is measured).
    pub aspects: Option<AspectVectors>,
    /// Category cues.
    pub cues: DspCues,
    /// Cosine to every taxonomy prompt (only for `--category-dump`).
    pub prompts: Option<Vec<f32>>,
    /// CLAP audio embedding (with [`EvalOptions::clap`]).
    pub embedding: Option<Embedding>,
}

/// Analyses one file; `Err` = decode or analysis failure (also a panic).
/// Names are hidden: no hints, no priors.
fn analyze(item: &Item, with_aspects: bool, opts: &EvalOptions) -> Result<Analysed, String> {
    let run = quiet_panic::guarded(|| {
        let a = with_workers(|w| {
            let decoded = w
                .decoder
                .decode(&item.path, &DecodeOptions::default())
                .map_err(|e| e.to_string())?;
            analyze_window(w, &decoded, NameHints::default(), with_aspects)
                .map_err(|e| e.to_string())
        })?;
        let dump = opts.category_dump.is_some();
        // A file CLAP cannot read is scored without it (counted as a
        // CLAP-only miss).
        let embedding = opts
            .clap
            .as_ref()
            .and_then(|c| c.embed_file(&item.path).ok());
        let clap = opts
            .clap
            .as_ref()
            .zip(embedding.as_ref())
            .map(|(c, e)| c.scores_of(e, dump));
        let (dsp_logits, _) = audio_logits(&a.cues, None);
        let (fused, raw_logits) = match &clap {
            Some((s, _)) => (
                classify_category(&a.cues, Some(s), None),
                audio_logits(&a.cues, Some(s)).0,
            ),
            None => (a.category, dsp_logits),
        };
        let category = CategoryObs {
            fused: fused.category,
            confidence: fused.confidence,
            raw_logits,
            dsp: a.category.category,
            dsp_logits,
            clap: clap.as_ref().map(|(s, _)| {
                let best = (0..s.len()).fold(0, |b, i| if s[i] > s[b] { i } else { b });
                Category::ALL[best]
            }),
        };
        let obs = Observation {
            kind: a.kind.kind,
            bpm: a.tempo.map(|t| f64::from(t.bpm)),
            key: a.key_note.key.map(|k| k.key),
            note_midi: a.key_note.note.map(|n| n.midi),
            category: Some(category),
        };
        Ok(Analysed {
            obs,
            aspects: a.aspects,
            cues: a.cues,
            prompts: clap.and_then(|(_, p)| p),
            embedding,
        })
    });
    run.unwrap_or_else(|panic| {
        crate::jobs::work::reset_workers();
        ClapScorer::reset_thread();
        Err(format!("internal error: {panic}"))
    })
}

/// Options of [`evaluate_with`].
#[derive(Debug, Clone, Default)]
pub struct EvalOptions {
    /// Also run the (report-only) similarity weight / distance sweep.
    pub similar_sweep: bool,
    /// CLAP for the category metrics (`None` = DSP only).
    pub clap: Option<Arc<ClapScorer>>,
    /// Write per-file category evidence (JSON lines) here (diagnostics for
    /// tuning the classifier, ADR-028).
    pub category_dump: Option<PathBuf>,
    /// CLAP text tower for the semantic / hybrid text-search metrics
    /// (with [`EvalOptions::clap`]; `None` = BM25 only).
    pub text_encoder: Option<Arc<Mutex<TextEncoder>>>,
}

/// Analyses every item of `ds` and scores the results.
pub fn evaluate(ds: &Dataset) -> TierReport {
    evaluate_with(ds, EvalOptions::default())
}

/// [`evaluate`] with options.
pub fn evaluate_with(ds: &Dataset, opts: EvalOptions) -> TierReport {
    let start = Instant::now();
    let with_aspects = ds.has_similarity();
    let all: Vec<&Item> = ds.items.iter().chain(&ds.similar_extra).collect();
    let analysed: Vec<Result<Analysed, String>> = all
        .par_iter()
        .map(|item| analyze(item, with_aspects, &opts))
        .collect();
    let wall_s = start.elapsed().as_secs_f64();

    let mut acc = Accumulator::default();
    let (mut unexpected, mut expected) = (0, 0);
    let mut notes = Vec::new();
    if let Some(path) = &opts.category_dump {
        let bank = opts.clap.as_ref().map(|c| &c.bank);
        if let Err(e) = super::dump::write(path, &ds.items, &analysed, bank) {
            notes.push(format!("category dump not written: {e}"));
        }
    }
    for (item, result) in ds.items.iter().zip(&analysed) {
        match result {
            Ok(a) => {
                if item.labels.expect_error {
                    // `error_or_partial` fixtures may decode; not a failure.
                    continue;
                }
                acc.add(&item.id, &item.labels, &a.obs);
            }
            Err(_) if item.labels.expect_error => expected += 1,
            Err(e) => {
                unexpected += 1;
                notes.push(format!("{}: {e}", item.id));
            }
        }
    }
    let mut metrics = acc.finish();
    if with_aspects {
        // Golden queries: every analysed file is in the pool (the labeled
        // ones are distractors). Fixtures: files of a sound family.
        let by_family = ds.similar_queries.is_empty();
        let pool: Vec<Entry> = all
            .iter()
            .zip(&analysed)
            .filter_map(|(item, r)| {
                let vectors = r.as_ref().ok()?.aspects.clone()?;
                (!by_family || item.labels.family.is_some()).then(|| Entry {
                    path: item.path.clone(),
                    vectors,
                    family: item.labels.family.clone(),
                    note_midi: item.labels.note_midi,
                })
            })
            .collect();
        metrics.similar = similarity::compute(&pool, &ds.similar_queries, opts.similar_sweep);
        if metrics.similar.is_none() {
            notes.push("similarity: no seed with a relevant file in the pool".into());
        }
    } else if ds.tier == Tier::Silver {
        notes.push("similarity not measured: silver has no relevance judgments".into());
    } else if ds.tier == Tier::Golden {
        notes.push("similarity not measured: no similar.jsonl in the golden directory".into());
    }
    text_search_metrics(ds, &all, &analysed, &opts, &mut metrics, &mut notes);
    if metrics.category.is_some() && opts.clap.is_none() {
        notes.push("category: CLAP model not used, shipped classifier = DSP only".into());
    }
    notes.extend(ds.missing.iter().map(|m| format!("missing file: {m}")));
    notes.extend(ds.warnings.iter().cloned());
    notes.truncate(MAX_NOTES);
    TierReport {
        tier: ds.tier,
        source: ds.source.clone(),
        files: ds.items.len(),
        unlabeled: ds.unlabeled,
        unexpected_errors: unexpected,
        expected_errors: expected,
        missing_files: ds.missing.len(),
        wall_s,
        metrics,
        notes,
    }
}

/// Text / NL search metrics ([`text_search`]): fixture queries judged from
/// truth, golden queries from `text_queries.jsonl`; silver has none.
fn text_search_metrics(
    ds: &Dataset,
    all: &[&Item],
    analysed: &[Result<Analysed, String>],
    opts: &EvalOptions,
    metrics: &mut Metrics,
    notes: &mut Vec<String>,
) {
    let queries = match ds.tier {
        Tier::Fixtures => fixture_queries(&ds.items),
        Tier::Golden | Tier::Silver => ds.text_queries.clone(),
    };
    if queries.is_empty() {
        return;
    }
    // Fixture folders name the truth group; golden files keep their path.
    let names = ds.tier != Tier::Fixtures;
    let entries: Vec<text_search::Entry> = all
        .iter()
        .zip(analysed)
        .filter(|(item, _)| !item.labels.expect_error)
        .filter_map(|(item, r)| {
            let a = r.as_ref().ok()?;
            Some(text_search::Entry {
                path: item.path.clone(),
                rel_path: names.then(|| item.id.clone()),
                kind: Some(a.obs.kind),
                bpm: a.obs.bpm,
                key: a.obs.key,
                category: a.obs.category.as_ref().and_then(|c| c.dsp),
                embedding: a.embedding,
            })
        })
        .collect();
    let encoder = opts.text_encoder.as_deref().filter(|_| opts.clap.is_some());
    match text_search::compute(&entries, &queries, encoder) {
        Ok(m) => {
            if m.as_ref().is_some_and(|m| m.semantic.is_none()) {
                notes.push("text search: CLAP not used, BM25 only".into());
            }
            metrics.text = m;
        }
        Err(e) => notes.push(format!("text search not measured: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Kind;
    use crate::eval::dataset::Labels;

    #[test]
    fn missing_and_corrupt_files_are_errors_not_panics() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("bad.wav"), b"RIFF not audio").unwrap();
        let item = |name: &str, expect_error| Item {
            id: name.into(),
            path: tmp.path().join(name),
            labels: Labels {
                kind: Some(Kind::Loop),
                expect_error,
                ..Labels::default()
            },
        };
        let ds = Dataset {
            tier: Tier::Golden,
            source: "t".into(),
            items: vec![
                item("bad.wav", true),
                item("gone.wav", false),
                item("bad.wav", false),
            ],
            unlabeled: 0,
            missing: vec![],
            warnings: vec![],
            similar_queries: vec![],
            similar_extra: vec![],
            text_queries: vec![],
        };
        let r = evaluate(&ds);
        assert_eq!((r.files, r.expected_errors, r.unexpected_errors), (3, 1, 2));
        assert_eq!(r.metrics, Metrics::default());
        // Two errors plus the "no similar.jsonl" note of a golden tier.
        assert_eq!(r.notes.len(), 3);
        assert!(r.notes[2].contains("similar.jsonl"));
    }
}

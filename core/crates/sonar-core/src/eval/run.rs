//! Runs the analyzer over a [`Dataset`] and scores it.
//!
//! The analysis is [`analyze_window`], the function the `dsp` job stage
//! stores kind / BPM / key / note with, called with **no name hint and no
//! filename prior** and given only decoded samples: file and folder names
//! cannot influence a result. Files run in parallel on the rayon pool, one
//! reused decoder/analyzer per thread.

use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::decode::DecodeOptions;
use crate::dsp::aspects::AspectVectors;
use crate::jobs::work::{analyze_window, with_workers};
use crate::quiet_panic;
use crate::text::FilenamePrior;

use super::dataset::{Dataset, Item, Tier};
use super::metrics::{Accumulator, Metrics, Observation};
use super::similarity::{self, Entry};

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

/// Observation plus (when similarity is measured) raw aspect vectors.
type Analysed = (Observation, Option<AspectVectors>);

/// Analyses one file; `Err` = decode or analysis failure (also a panic).
fn analyze(item: &Item, with_aspects: bool) -> Result<Analysed, String> {
    let run = quiet_panic::guarded(|| {
        with_workers(|w| {
            let decoded = w
                .decoder
                .decode(&item.path, &DecodeOptions::default())
                .map_err(|e| e.to_string())?;
            let a = analyze_window(w, &decoded, None, FilenamePrior::default(), with_aspects)
                .map_err(|e| e.to_string())?;
            let obs = Observation {
                kind: a.kind.kind,
                bpm: a.tempo.map(|t| f64::from(t.bpm)),
                key: a.key_note.key.map(|k| k.key),
                note_midi: a.key_note.note.map(|n| n.midi),
            };
            Ok((obs, a.aspects))
        })
    });
    run.unwrap_or_else(|panic| {
        crate::jobs::work::reset_workers();
        Err(format!("internal error: {panic}"))
    })
}

/// Options of [`evaluate_with`].
#[derive(Debug, Clone, Copy, Default)]
pub struct EvalOptions {
    /// Also run the (report-only) similarity weight / distance sweep.
    pub similar_sweep: bool,
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
        .map(|item| analyze(item, with_aspects))
        .collect();
    let wall_s = start.elapsed().as_secs_f64();

    let mut acc = Accumulator::default();
    let (mut unexpected, mut expected) = (0, 0);
    let mut notes = Vec::new();
    for (item, result) in ds.items.iter().zip(&analysed) {
        match result {
            Ok((obs, _)) => {
                if item.labels.expect_error {
                    // `error_or_partial` fixtures may decode; not a failure.
                    continue;
                }
                acc.add(&item.labels, obs);
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
                let vectors = r.as_ref().ok()?.1.clone()?;
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
        notes.push("similarity not measured: silver names carry no category".into());
    } else if ds.tier == Tier::Golden {
        notes.push("similarity not measured: no similar.jsonl in the golden directory".into());
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
        };
        let r = evaluate(&ds);
        assert_eq!((r.files, r.expected_errors, r.unexpected_errors), (3, 1, 2));
        assert_eq!(r.metrics, Metrics::default());
        // Two errors plus the "no similar.jsonl" note of a golden tier.
        assert_eq!(r.notes.len(), 3);
        assert!(r.notes[2].contains("similar.jsonl"));
    }
}

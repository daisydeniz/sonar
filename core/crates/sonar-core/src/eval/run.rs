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
use crate::jobs::work::{analyze_window, with_workers};
use crate::quiet_panic;
use crate::text::FilenamePrior;

use super::dataset::{Dataset, Item, Tier};
use super::metrics::{Accumulator, Metrics, Observation};

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

/// Analyses one file; `Err` = decode or analysis failure (also a panic).
fn analyze(item: &Item) -> Result<Observation, String> {
    let run = quiet_panic::guarded(|| {
        with_workers(|w| {
            let decoded = w
                .decoder
                .decode(&item.path, &DecodeOptions::default())
                .map_err(|e| e.to_string())?;
            let a = analyze_window(w, &decoded, None, FilenamePrior::default(), false)
                .map_err(|e| e.to_string())?;
            Ok(Observation {
                kind: a.kind.kind,
                bpm: a.tempo.map(|t| f64::from(t.bpm)),
                key: a.key_note.key.map(|k| k.key),
                note_midi: a.key_note.note.map(|n| n.midi),
            })
        })
    });
    run.unwrap_or_else(|panic| {
        crate::jobs::work::reset_workers();
        Err(format!("internal error: {panic}"))
    })
}

/// Analyses every item of `ds` and scores the results.
pub fn evaluate(ds: &Dataset) -> TierReport {
    let start = Instant::now();
    let results: Vec<Result<Observation, String>> = ds.items.par_iter().map(analyze).collect();
    let wall_s = start.elapsed().as_secs_f64();

    let mut acc = Accumulator::default();
    let (mut unexpected, mut expected) = (0, 0);
    let mut notes = Vec::new();
    for (item, result) in ds.items.iter().zip(&results) {
        match result {
            Ok(obs) => {
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
        metrics: acc.finish(),
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
        };
        let r = evaluate(&ds);
        assert_eq!((r.files, r.expected_errors, r.unexpected_errors), (3, 1, 2));
        assert_eq!(r.metrics, Metrics::default());
        assert_eq!(r.notes.len(), 2);
    }
}

//! `sonar bench analyze`: the job pipeline over a library made of copies of
//! the (decodable) fixture files, in a throwaway store.

use std::fs;
use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use super::decode::generate_fixtures;
use super::{machine, Machine};
use crate::jobs::{self, JobOptions, Stage, StageSummary};
use crate::scan::scan_library;
use crate::{Error, Result, Store};

/// Options for [`bench_analyze`].
#[derive(Debug, Clone)]
pub struct AnalyzeBenchOptions {
    /// Files in the library (fixture files are replicated to reach it).
    pub files: usize,
    /// Stages to run.
    pub stages: Vec<Stage>,
    /// Worker threads; 0 = all cores.
    pub threads: usize,
    /// Files per committed batch.
    pub batch: usize,
}

/// Result of [`bench_analyze`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalyzeBench {
    /// Where it ran.
    pub machine: Machine,
    /// Files in the library.
    pub files: usize,
    /// Bytes of audio in the library.
    pub bytes: u64,
    /// Distinct fixture files that were replicated.
    pub distinct_sources: usize,
    /// Worker threads requested (0 = all cores).
    pub threads: usize,
    /// Batch size.
    pub batch: usize,
    /// Seconds to copy the files (untimed in the results below).
    pub generate_secs: f64,
    /// Seconds for the initial scan (untimed in the results below).
    pub scan_secs: f64,
    /// One entry per stage that ran.
    pub stages: Vec<StageSummary>,
    /// Wall-clock seconds of the whole job.
    pub job_secs: f64,
}

/// Builds a library of `opts.files` copies of the decodable fixture files
/// (without the > 5 min one) in `work/library`, scans it into
/// `work/bench.db` and runs the requested stages.
///
/// # Errors
/// [`Error::Io`], [`Error::Sqlite`], or the job's errors.
pub fn bench_analyze(work: &Path, opts: &AnalyzeBenchOptions) -> Result<AnalyzeBench> {
    let src = work.join("fixtures");
    let manifest = generate_fixtures(&src, false)?;
    let sources: Vec<_> = manifest
        .fixtures
        .iter()
        .filter(|f| f.truth.corrupt.is_none())
        .map(|f| src.join(&f.path))
        .collect();
    if sources.is_empty() {
        return Err(Error::Internal {
            reason: "no decodable fixtures to replicate".into(),
        });
    }

    let root = work.join("library");
    let t = Instant::now();
    let mut bytes = 0;
    for i in 0..opts.files {
        let from = &sources[i % sources.len()];
        let ext = from.extension().and_then(|e| e.to_str()).unwrap_or("wav");
        let dir = root.join(format!("pack{:03}", i / 200));
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let to = dir.join(format!("s{i:06}.{ext}"));
        bytes += fs::copy(from, &to).map_err(|e| Error::io(&to, e))?;
    }
    let generate_secs = t.elapsed().as_secs_f64();

    let db = work.join("bench.db");
    let t = Instant::now();
    {
        let mut store = Store::open(&db)?;
        let lib = store.add_library(&root)?;
        scan_library(&mut store, lib.id)?;
    }
    let scan_secs = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let summary = jobs::start(
        &db,
        JobOptions {
            stages: opts.stages.clone(),
            threads: opts.threads,
            batch_size: opts.batch.max(1),
            ..JobOptions::default()
        },
    )?
    .wait()?;
    Ok(AnalyzeBench {
        machine: machine(),
        files: opts.files,
        bytes,
        distinct_sources: sources.len(),
        threads: opts.threads,
        batch: opts.batch,
        generate_secs,
        scan_secs,
        stages: summary.stages,
        job_secs: t.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_both_stages_on_a_small_library() {
        let tmp = tempfile::tempdir().expect("tmp");
        let b = bench_analyze(
            tmp.path(),
            &AnalyzeBenchOptions {
                files: 60,
                stages: vec![Stage::Meta, Stage::Dsp],
                threads: 2,
                batch: 32,
            },
        )
        .expect("bench");
        assert_eq!(b.stages.len(), 2);
        for s in &b.stages {
            assert_eq!((s.total, s.done, s.errors), (60, 60, 0), "{s:?}");
        }
    }
}

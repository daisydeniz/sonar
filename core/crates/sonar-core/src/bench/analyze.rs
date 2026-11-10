//! `sonar bench analyze`: the job pipeline over a library made of copies of
//! the (decodable) fixture files, in a throwaway store.

use std::fs;
use std::io::Read;
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
    /// Read every file once (untimed) before the stages so they run on a
    /// warm page cache; false for `--cold`.
    pub warm: bool,
}

/// Result of [`bench_analyze`]. By default the numbers are for a **warm
/// page cache** (see [`AnalyzeBench::warm_cache`]).
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
    /// Worker threads used (resolved when 0 was requested).
    pub threads: usize,
    /// Batch size.
    pub batch: usize,
    /// Whether the files were read once before the stages ran, so the
    /// timings are CPU + database work on a warm page cache.
    pub warm_cache: bool,
    /// Seconds to write the files (untimed in the results below).
    pub generate_secs: f64,
    /// Seconds for the initial scan (untimed in the results below).
    pub scan_secs: f64,
    /// Seconds for the cache warm-up pass (untimed; 0 with `--cold`).
    pub warmup_secs: f64,
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

    // Independent files: the source bytes are read once and written out per
    // file. `fs::copy` would make APFS clones that share extents and start
    // with a cold page cache, which distorts the per-stage numbers.
    let mut contents = Vec::with_capacity(sources.len());
    for s in &sources {
        contents.push(fs::read(s).map_err(|e| Error::io(s, e))?);
    }
    let root = work.join("library");
    let t = Instant::now();
    let mut bytes = 0;
    for i in 0..opts.files {
        let data = &contents[i % sources.len()];
        let ext = sources[i % sources.len()]
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("wav");
        let dir = root.join(format!("pack{:03}", i / 200));
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let to = dir.join(format!("s{i:06}.{ext}"));
        fs::write(&to, data).map_err(|e| Error::io(&to, e))?;
        bytes += data.len() as u64;
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

    // Untimed warm-up: read every file in full so the stages measure CPU and
    // database work, not whichever stage happens to touch the disk first.
    let t = Instant::now();
    if opts.warm {
        warm_cache(&root)?;
    }
    let warmup_secs = t.elapsed().as_secs_f64();

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
        threads: match opts.threads {
            0 => std::thread::available_parallelism().map_or(1, usize::from),
            n => n,
        },
        batch: opts.batch,
        warm_cache: opts.warm,
        generate_secs,
        scan_secs,
        warmup_secs,
        stages: summary.stages,
        job_secs: t.elapsed().as_secs_f64(),
    })
}

/// Reads every file under `root` completely, discarding the data.
fn warm_cache(root: &Path) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    let mut buf = vec![0u8; 1 << 20];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))? {
            let path = entry.map_err(|e| Error::io(&dir, e))?.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let mut f = fs::File::open(&path).map_err(|e| Error::io(&path, e))?;
            while f.read(&mut buf).map_err(|e| Error::io(&path, e))? > 0 {}
        }
    }
    Ok(())
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
                warm: true,
            },
        )
        .expect("bench");
        assert_eq!(b.threads, 2);
        assert!(b.warm_cache);
        assert_eq!(b.stages.len(), 2);
        for s in &b.stages {
            assert_eq!((s.total, s.done, s.errors), (60, 60, 0), "{s:?}");
        }
    }
}

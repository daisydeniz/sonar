//! `sonar bench watch`: how the real watcher (ADR-014 settings) copes with a
//! continuous copy into a large library, alone and while the job pipeline
//! analyzes the same store.
//!
//! Phases (all in a throwaway store; the library generation and the initial
//! scan are untimed set-up):
//! 1. **Copy only**: a writer thread creates new audio files in new folders
//!    of the library, in small chunks like a Finder copy. Measured from the
//!    watcher's events: scans, their durations, duty cycle over the copy
//!    window, and the latency from the end of the copy until a scan that
//!    started after it (so it saw every file) had finished.
//! 2. **Baseline** (`with_analyze`): the pipeline alone over a copy of the
//!    store (no watcher, no copy), for the uncontended throughput.
//! 3. **Contention** (`with_analyze`): the pipeline runs on the library for
//!    the whole of a second copy (restarted over reset work whenever it
//!    finishes early) while the watcher rescans. The scans that overlap a
//!    pipeline run are counted.
//!
//! After every copy phase the index is compared with the disk: file count
//! and the summed size of the new files must match what was written.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::corpus::{self, rel_path};
use super::watch_pipeline::{self, PipelineRun};
use super::watch_stats::{duty_within, overlap, ScanStats, Span};
use super::{machine, Machine};
use crate::jobs::Stage;
use crate::scan::scan_library;
use crate::watch::{self, WatchEvent, WatchOptions};
use crate::{Error, Result, Store};

/// Options for [`bench_watch`].
#[derive(Debug, Clone)]
pub struct WatchBenchOptions {
    /// Files in the base library.
    pub files: usize,
    /// Files created per copy phase.
    pub copy_files: usize,
    /// Files per second the writer aims for; `None` = as fast as possible.
    pub copy_rate: Option<f64>,
    /// Also run the baseline and the contention phase.
    pub with_analyze: bool,
    /// Pipeline stages for those phases.
    pub stages: Vec<Stage>,
    /// Watcher settings (production defaults for the real measurement).
    pub watch: WatchOptions,
    /// How long to wait for the watcher to catch up after a copy ends.
    pub catch_up_timeout: Duration,
}

impl Default for WatchBenchOptions {
    fn default() -> Self {
        Self {
            files: 50_000,
            copy_files: 2_000,
            copy_rate: None,
            with_analyze: false,
            stages: vec![Stage::Meta, Stage::Dsp],
            watch: WatchOptions::default(),
            catch_up_timeout: Duration::from_secs(120),
        }
    }
}

/// Watcher behaviour during one copy phase.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WatchPhase {
    /// Files the writer created.
    pub copy_files: usize,
    /// Seconds the copy took.
    pub copy_secs: f64,
    /// The watcher's scans from the start of the copy until catch-up.
    pub scans: ScanStats,
    /// Share of the copy window (start to end of the copy) the watcher spent
    /// scanning; scans are clipped to the window.
    pub duty_cycle: f64,
    /// Seconds from the end of the copy until the first scan that started
    /// after it had finished (so it saw every file).
    pub catch_up_secs: f64,
    /// Such a scan finished before the timeout.
    pub caught_up: bool,
    /// Files expected in the index afterwards.
    pub expected_files: u64,
    /// Files in the index afterwards (not missing).
    pub db_files: u64,
    /// Bytes the writer wrote.
    pub bytes_written: u64,
    /// Growth of the summed `files.size` in the index.
    pub bytes_indexed: u64,
    /// File count and bytes both match.
    pub verified: bool,
    /// `WatchEvent::Error`s (e.g. SQLITE_BUSY) during the phase.
    pub watcher_errors: usize,
    /// The first few error messages.
    pub error_messages: Vec<String>,
}

/// The contention phase: copy + watcher + pipeline at once.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContendedPhase {
    /// Watcher behaviour.
    pub watch: WatchPhase,
    /// Pipeline behaviour (runs restarted to last for the whole copy).
    pub pipeline: PipelineRun,
    /// Watcher scans that overlapped a pipeline run.
    pub overlapping_scans: usize,
    /// Total milliseconds those scans overlapped a pipeline run.
    pub overlapping_scan_ms: u64,
}

/// Result of [`bench_watch`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WatchBench {
    /// Where it ran.
    pub machine: Machine,
    /// Files in the base library.
    pub files: usize,
    /// Target copy rate (files/s), `None` = unthrottled.
    pub copy_rate: Option<f64>,
    /// Seconds to generate the library (untimed set-up).
    pub generate_secs: f64,
    /// Seconds for the initial scan (untimed set-up).
    pub initial_scan_secs: f64,
    /// Milliseconds the watcher's own first scan took (no-op rescan).
    pub watcher_start_scan_ms: u64,
    /// Phase 1.
    pub copy_only: WatchPhase,
    /// Phase 2: pipeline alone.
    pub baseline: Option<PipelineRun>,
    /// Phase 3: pipeline during a copy.
    pub contended: Option<ContendedPhase>,
}

#[derive(Debug, Clone)]
enum Rec {
    /// A finished scan; `span` is `[end - duration, end]`.
    Scan {
        span: Span,
        duration_ms: u64,
    },
    Error(String),
}

/// A pipeline run with the count and milliseconds of watcher scans that
/// overlapped it.
type PipeResult = (PipelineRun, usize, u64);

type Log = Arc<Mutex<Vec<Rec>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Env<'a> {
    opts: &'a WatchBenchOptions,
    root: &'a Path,
    db: &'a Path,
    library_id: i64,
    log: Log,
}

/// Runs the benchmark in `work` (`library/`, `bench.db`, `baseline.db`).
///
/// # Errors
/// [`Error::Io`], [`Error::Sqlite`], or [`Error::Internal`] if the watcher
/// does not start. Slow or failed catch-up is reported in the result, not
/// as an error.
pub fn bench_watch(work: &Path, opts: &WatchBenchOptions) -> Result<WatchBench> {
    let root = work.join("library");
    let db = work.join("bench.db");
    let t = Instant::now();
    corpus::generate_tree(&root, opts.files)?;
    let generate_secs = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let library_id = {
        let mut store = Store::open(&db)?;
        let lib = store.add_library(&root)?;
        scan_library(&mut store, lib.id)?;
        lib.id
    };
    let initial_scan_secs = t.elapsed().as_secs_f64();

    let log: Log = Arc::default();
    let sink = Arc::clone(&log);
    let handle = watch::start(&db, opts.watch.clone(), move |e| {
        let rec = match e {
            WatchEvent::Scanned { summary, .. } => {
                let end = Instant::now();
                let len = Duration::from_millis(summary.duration_ms);
                Rec::Scan {
                    span: (end.checked_sub(len).unwrap_or(end), end),
                    duration_ms: summary.duration_ms,
                }
            }
            WatchEvent::Error { message, .. } => Rec::Error(message.clone()),
            _ => return,
        };
        lock(&sink).push(rec);
    })?;
    let env = Env {
        opts,
        root: &root,
        db: &db,
        library_id,
        log,
    };
    let watcher_start_scan_ms = wait_first_scan(&env)?;

    let m = opts.copy_files;
    let (copy_only, _) = run_phase(&env, opts.files, None)?;
    let (baseline, contended) = if opts.with_analyze {
        let base = watch_pipeline::baseline(work, &root, &opts.stages)?;
        let (watch_phase, pipe) = run_phase(&env, opts.files + m, Some(&opts.stages))?;
        let (pipeline, overlapping_scans, overlapping_scan_ms) =
            pipe.ok_or_else(|| Error::Internal {
                reason: "contention phase produced no pipeline result".into(),
            })?;
        (
            Some(base),
            Some(ContendedPhase {
                watch: watch_phase,
                pipeline,
                overlapping_scans,
                overlapping_scan_ms,
            }),
        )
    } else {
        (None, None)
    };
    handle.stop();
    Ok(WatchBench {
        machine: machine(),
        files: opts.files,
        copy_rate: opts.copy_rate,
        generate_secs,
        initial_scan_secs,
        watcher_start_scan_ms,
        copy_only,
        baseline,
        contended,
    })
}

/// Waits for the watcher's start-up scan; returns its duration.
fn wait_first_scan(env: &Env<'_>) -> Result<u64> {
    let deadline = Instant::now() + env.opts.catch_up_timeout;
    while Instant::now() < deadline {
        if let Some(ms) = lock(&env.log).iter().find_map(|r| match r {
            Rec::Scan { duration_ms, .. } => Some(*duration_ms),
            Rec::Error(_) => None,
        }) {
            return Ok(ms);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(Error::Internal {
        reason: "the watcher did not report its start-up scan".into(),
    })
}

/// Copies `opts.copy_files` files (corpus indices from `first`) while
/// optionally keeping the pipeline running. Returns the watcher's phase and,
/// with a pipeline, its run plus the scans overlapping it (count, ms).
fn run_phase(
    env: &Env<'_>,
    first: usize,
    stages: Option<&Vec<Stage>>,
) -> Result<(WatchPhase, Option<PipeResult>)> {
    let m = env.opts.copy_files;
    let (files_before, bytes_before) = index_totals(env)?;
    let mark = lock(&env.log).len();
    let start = Instant::now();
    let (root, rate_hz) = (env.root.to_path_buf(), env.opts.copy_rate);
    let writer = thread::spawn(move || write_files(&root, first, m, rate_hz));
    let pipe =
        stages.map(|stages| watch_pipeline::run_while(env.db, stages, || !writer.is_finished()));
    // Join the writer before surfacing a pipeline error, so it never keeps
    // writing into a work folder that is about to be deleted.
    let written = writer.join().map_err(|_| Error::Internal {
        reason: "copy thread panicked".into(),
    });
    let pipe = pipe.transpose()?;
    let (copy_end, bytes_written) = written??;

    // Catch-up: the first scan that started after the copy ended saw it all.
    let deadline = copy_end + env.opts.catch_up_timeout;
    let mut caught_at = None;
    while caught_at.is_none() && Instant::now() < deadline {
        caught_at = lock(&env.log).iter().skip(mark).find_map(|r| match r {
            Rec::Scan { span, .. } if span.0 >= copy_end => Some(span.1),
            _ => None,
        });
        if caught_at.is_none() {
            thread::sleep(Duration::from_millis(10));
        }
    }
    let end = caught_at.unwrap_or_else(Instant::now);
    let (mut spans, mut durations, mut errors) = (Vec::new(), Vec::new(), Vec::new());
    for r in lock(&env.log).iter().skip(mark) {
        match r {
            Rec::Scan { span, duration_ms } if span.1 >= start && span.0 <= end => {
                spans.push(*span);
                durations.push(*duration_ms);
            }
            Rec::Error(msg) => errors.push(msg.clone()),
            Rec::Scan { .. } => {}
        }
    }
    let (files_after, bytes_after) = index_totals(env)?;
    let expected = files_before + m as u64;
    let bytes_indexed = bytes_after.saturating_sub(bytes_before);
    let phase = WatchPhase {
        copy_files: m,
        copy_secs: copy_end.saturating_duration_since(start).as_secs_f64(),
        scans: ScanStats::of(&durations),
        duty_cycle: duty_within(&spans, (start, copy_end)),
        catch_up_secs: end.saturating_duration_since(copy_end).as_secs_f64(),
        caught_up: caught_at.is_some(),
        expected_files: expected,
        db_files: files_after,
        bytes_written,
        bytes_indexed,
        verified: files_after == expected && bytes_indexed == bytes_written,
        watcher_errors: errors.len(),
        error_messages: errors.into_iter().take(5).collect(),
    };
    let pipe = pipe.map(|o| {
        let (n, d) = overlap(&spans, &o.active);
        let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        (o.run, n, ms)
    });
    Ok((phase, pipe))
}

/// `(files not missing, summed size)` of the library in the index.
fn index_totals(env: &Env<'_>) -> Result<(u64, u64)> {
    let store = Store::open(env.db)?;
    let (n, bytes): (i64, i64) = store.conn().query_row(
        "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files \
         WHERE library_id = ?1 AND status <> 'missing'",
        [env.library_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((
        u64::try_from(n).unwrap_or(0),
        u64::try_from(bytes).unwrap_or(0),
    ))
}

/// Creates files `first..first + count` of the corpus layout (new folders
/// beyond the base library), written in 1 KB chunks, paced to `rate_hz`.
/// Returns when the last file is closed, and the bytes written.
fn write_files(
    root: &Path,
    first: usize,
    count: usize,
    rate_hz: Option<f64>,
) -> Result<(Instant, u64)> {
    let templates = corpus::templates();
    let start = Instant::now();
    let mut bytes = 0;
    for i in 0..count {
        if let Some(hz) = rate_hz.filter(|h| *h > 0.0) {
            let due = start + Duration::from_secs_f64(i as f64 / hz);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        let path = root.join(rel_path(first + i));
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        let data = corpus::content(&templates, first + i);
        let mut f = fs::File::create(&path).map_err(|e| Error::io(&path, e))?;
        for chunk in data.chunks(1024) {
            f.write_all(chunk).map_err(|e| Error::io(&path, e))?;
            thread::yield_now();
        }
        bytes += data.len() as u64;
    }
    Ok((Instant::now(), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny(with_analyze: bool) -> WatchBenchOptions {
        WatchBenchOptions {
            files: 150,
            copy_files: 60,
            copy_rate: Some(400.0),
            with_analyze,
            stages: vec![Stage::Meta],
            watch: WatchOptions {
                debounce: Duration::from_millis(100),
                max_delay: Duration::from_millis(500),
                ..WatchOptions::default()
            },
            catch_up_timeout: Duration::from_secs(60),
        }
    }

    #[test]
    fn copy_phase_is_caught_up_and_verified() {
        let tmp = tempfile::tempdir().expect("tmp");
        let b = bench_watch(tmp.path(), &tiny(false)).expect("bench");
        let p = &b.copy_only;
        assert!(p.caught_up && p.verified, "{p:?}");
        assert_eq!((p.expected_files, p.db_files), (210, 210));
        assert_eq!(p.bytes_written, p.bytes_indexed);
        assert_eq!(p.watcher_errors, 0, "{p:?}");
        assert!(p.scans.count >= 1);
        assert!(b.baseline.is_none() && b.contended.is_none());
    }

    #[test]
    fn contention_phase_runs_pipeline_and_verifies() {
        let tmp = tempfile::tempdir().expect("tmp");
        let b = bench_watch(tmp.path(), &tiny(true)).expect("bench");
        let base = b.baseline.expect("baseline");
        assert_eq!((base.error.as_deref(), base.runs), (None, 1));
        assert_eq!(base.files_done, 210);
        let c = b.contended.expect("contended");
        assert_eq!(c.pipeline.error, None, "{c:?}");
        assert!(c.pipeline.runs >= 1 && c.pipeline.files_done >= 210);
        assert!(c.watch.caught_up && c.watch.verified, "{c:?}");
        assert_eq!(c.watch.expected_files, 270);
        assert_eq!(c.watch.watcher_errors, 0, "{c:?}");
    }
}

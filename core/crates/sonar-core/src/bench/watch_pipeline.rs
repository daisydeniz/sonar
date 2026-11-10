//! The job-pipeline side of `sonar bench watch`: a pipeline that keeps
//! committing for as long as a copy runs (restarted over reset work when it
//! finishes early), and the uncontended baseline.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use serde::Serialize;

use super::rate;
use super::watch_stats::max_gap;
use crate::jobs::{self, JobEvent, JobOptions, Stage, StageSummary};
use crate::scan::scan_library;
use crate::{Result, Store};

/// One or more back-to-back runs of the job pipeline.
///
/// Note: `files_done` and the stage `done` counts can exceed the stage
/// `total` of a run, because the watcher keeps adding files to the store
/// while the run is in progress (the claim loop picks them up).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipelineRun {
    /// Number of job runs (more than one when the copy outlasted a run and
    /// the work was reset and restarted).
    pub runs: usize,
    /// Per-stage totals of every run, in order.
    pub stages: Vec<StageSummary>,
    /// Seconds the pipeline was actually running (sum over runs).
    pub active_secs: f64,
    /// Files handled over all stages and runs.
    pub files_done: u64,
    /// `files_done / active_secs`.
    pub files_per_sec: f64,
    /// Longest pause between progress events (a stalled commit shows here).
    pub max_progress_gap_ms: u64,
    /// The job's error, if one failed (the loop then stops).
    pub error: Option<String>,
}

/// A [`PipelineRun`] plus the wall-clock intervals it was active.
pub(super) struct Outcome {
    pub run: PipelineRun,
    pub active: Vec<(Instant, Instant)>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs the pipeline over the store at `db`; after each run, `again()`
/// decides whether to reset the stage results and run once more.
pub(super) fn run_while(db: &Path, stages: &[Stage], again: impl Fn() -> bool) -> Result<Outcome> {
    let mut run = PipelineRun {
        runs: 0,
        stages: Vec::new(),
        active_secs: 0.0,
        files_done: 0,
        files_per_sec: 0.0,
        max_progress_gap_ms: 0,
        error: None,
    };
    let mut active = Vec::new();
    let mut gap = std::time::Duration::ZERO;
    loop {
        let progress: Arc<Mutex<Vec<Instant>>> = Arc::default();
        let sink = Arc::clone(&progress);
        let start = Instant::now();
        let result = jobs::start(
            db,
            JobOptions {
                stages: stages.to_vec(),
                on_event: Some(Arc::new(move |e: &JobEvent| {
                    if matches!(e, JobEvent::Progress { .. }) {
                        lock(&sink).push(Instant::now());
                    }
                })),
                ..JobOptions::default()
            },
        )?
        .wait();
        let end = Instant::now();
        run.runs += 1;
        run.active_secs += end.duration_since(start).as_secs_f64();
        active.push((start, end));
        gap = gap.max(max_gap(start, &lock(&progress)));
        match result {
            Ok(s) => {
                run.files_done += s.stages.iter().map(|x| x.done).sum::<u64>();
                run.stages.extend(s.stages);
            }
            Err(e) => {
                run.error = Some(e.to_string());
                break;
            }
        }
        if !again() {
            break;
        }
        // Start over: every file is pending again (bench-owned store). This
        // reset is one large write transaction the real app never issues, so
        // it adds contention: a "0 BUSY" result here is conservative.
        Store::open(db)?
            .conn()
            .execute("DELETE FROM file_stages", [])?;
    }
    run.files_per_sec = rate(run.files_done, run.active_secs);
    run.max_progress_gap_ms = u64::try_from(gap.as_millis()).unwrap_or(u64::MAX);
    Ok(Outcome { run, active })
}

/// The pipeline once, alone, over the library at `root` in its own store
/// `work/baseline.db` (no watcher, no copy).
pub(super) fn baseline(work: &Path, root: &Path, stages: &[Stage]) -> Result<PipelineRun> {
    let db = work.join("baseline.db");
    {
        let mut store = Store::open(&db)?;
        let lib = store.add_library(root)?;
        scan_library(&mut store, lib.id)?;
    }
    Ok(run_while(&db, stages, || false)?.run)
}

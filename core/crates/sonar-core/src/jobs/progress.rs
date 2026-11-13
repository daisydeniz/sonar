//! Progress reporting and pause/cancel control shared between the job
//! thread, its workers and the [`super::JobHandle`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use serde::Serialize;

use super::stage::Stage;
use crate::quiet_panic;

/// Event delivered to [`super::JobOptions::on_event`], from the job thread
/// (or, for `Paused`/`Resumed`, the thread that called the handle). The
/// callback must be quick and must not call back into the handle's `wait`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum JobEvent {
    /// A stage begins; `total` files were pending when it started (files
    /// added by a concurrent scan may push `done` past it).
    StageStarted {
        /// The stage.
        stage: Stage,
        /// Pending files at stage start.
        total: u64,
    },
    /// A batch was committed (throttled to one event per batch).
    Progress {
        /// The stage.
        stage: Stage,
        /// Files handled so far (ok + partial + errors + skipped).
        done: u64,
        /// Pending files at stage start.
        total: u64,
        /// Files recorded as errors so far.
        errors: u64,
        /// Files left pending (I/O error, changed meanwhile) so far.
        skipped: u64,
    },
    /// A batch was committed: the files whose rows (and, for `Dsp`/`Embed`,
    /// vectors) changed. Sent before the matching `Progress`. Hosts use it
    /// to refresh derived data such as the text index (T5.5a).
    Committed {
        /// The stage.
        stage: Stage,
        /// `files.id` of every file in the batch.
        file_ids: Vec<i64>,
    },
    /// A requested stage cannot run in this build or on this machine (the
    /// `Embed` stage without model files) and was skipped; no file was
    /// marked.
    StageSkipped {
        /// The stage.
        stage: Stage,
        /// Why, for the user.
        reason: String,
    },
    /// Workers stop taking new files.
    Paused,
    /// Workers continue.
    Resumed,
    /// A stage ended (completed or cut short by cancel).
    StageFinished(StageSummary),
    /// Cancel was requested; in-flight files were committed.
    Cancelled,
    /// All requested stages ran to completion.
    Finished,
}

/// Totals of one stage in one job run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StageSummary {
    /// The stage.
    pub stage: Stage,
    /// Pending files when the stage started.
    pub total: u64,
    /// Files handled: `ok + partial + errors + skipped`.
    pub done: u64,
    /// Completed cleanly.
    pub ok: u64,
    /// Completed on a damaged/truncated file.
    pub partial: u64,
    /// Recorded as errors (`files.status = 'error'`).
    pub errors: u64,
    /// Left pending: I/O error, or the file changed between claim and commit.
    pub skipped: u64,
    /// Wall-clock seconds.
    pub seconds: f64,
    /// Where the wall-clock time went (pipeline instrumentation).
    pub timing: StageTiming,
}

/// Where a stage's wall-clock time went, to judge how much of the worker
/// pool sat idle (ADR-013 addendum). Seconds unless noted.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StageTiming {
    /// Worker threads in the pool.
    pub threads: usize,
    /// Batches committed.
    pub batches: u64,
    /// Coordinator time reading batches from the store.
    pub claim_secs: f64,
    /// Coordinator time blocked waiting for the next batch's workers (the
    /// pool was the bottleneck; with look-ahead it overlaps other batches).
    pub wait_secs: f64,
    /// Coordinator time committing batches (incl. BUSY retries).
    pub commit_secs: f64,
    /// Sum of per-file worker time (decode + measure).
    pub busy_secs: f64,
    /// Sum over batches of the slowest file's time.
    pub max_file_secs: f64,
}

impl StageTiming {
    /// Share of `threads x wall` the workers spent on files (0..1).
    pub fn utilization(&self, wall: f64) -> f64 {
        let cap = self.threads as f64 * wall;
        if cap > 0.0 {
            self.busy_secs / cap
        } else {
            0.0
        }
    }
}

impl StageSummary {
    pub(crate) fn new(stage: Stage, total: u64) -> Self {
        Self {
            stage,
            total,
            done: 0,
            ok: 0,
            partial: 0,
            errors: 0,
            skipped: 0,
            seconds: 0.0,
            timing: StageTiming::default(),
        }
    }

    /// Throughput of the stage (files handled per second).
    pub fn files_per_sec(&self) -> f64 {
        if self.seconds > 0.0 {
            self.done as f64 / self.seconds
        } else {
            0.0
        }
    }
}

/// A requested stage that did not run, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkippedStage {
    /// The stage.
    pub stage: Stage,
    /// Why (missing model files, build without the feature, ...).
    pub reason: String,
}

/// Result of a whole job run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobSummary {
    /// One entry per stage that started, in run order.
    pub stages: Vec<StageSummary>,
    /// Requested stages that could not run.
    pub skipped: Vec<SkippedStage>,
    /// The job was cancelled before finishing every stage.
    pub cancelled: bool,
    /// Wall-clock seconds.
    pub seconds: f64,
}

/// Point-in-time view of a running job, for polling UIs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProgressSnapshot {
    /// Stage currently running (the last one after the job ended).
    pub stage: Option<Stage>,
    /// Pending files when that stage started.
    pub total: u64,
    /// Files handled in that stage so far.
    pub done: u64,
    /// Of those, recorded as errors.
    pub errors: u64,
    /// Of those, left pending.
    pub skipped: u64,
    /// Pause requested.
    pub paused: bool,
    /// Cancel requested.
    pub cancelled: bool,
    /// The job thread has finished.
    pub finished: bool,
}

#[derive(Default)]
struct ControlState {
    paused: bool,
    cancelled: bool,
}

type Listener = Arc<dyn Fn(&JobEvent) + Send + Sync>;

/// State shared by the job thread, workers and handle.
pub(crate) struct Shared {
    control: Mutex<ControlState>,
    wake: Condvar,
    progress: Mutex<ProgressSnapshot>,
    listener: Option<Listener>,
    /// Mirrors `cancelled`; handed to decoders so cancel interrupts a long
    /// decode (the file then stays pending).
    cancel_flag: Arc<AtomicBool>,
    /// Lock-free mirror of "neither paused nor cancelled" for the per-file
    /// check; written only while holding `control`.
    runnable: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub(crate) fn new(listener: Option<Listener>) -> Arc<Self> {
        Arc::new(Self {
            control: Mutex::new(ControlState::default()),
            wake: Condvar::new(),
            progress: Mutex::new(ProgressSnapshot {
                stage: None,
                total: 0,
                done: 0,
                errors: 0,
                skipped: 0,
                paused: false,
                cancelled: false,
                finished: false,
            }),
            listener,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            runnable: AtomicBool::new(true),
        })
    }

    /// Delivers `event`; a panicking listener is contained.
    pub(crate) fn emit(&self, event: JobEvent) {
        if let Some(f) = &self.listener {
            if quiet_panic::guarded(|| f(&event)).is_err() {
                tracing::warn!("job event listener panicked on {event:?}");
            }
        }
    }

    pub(crate) fn pause(&self) {
        let changed = {
            let mut c = lock(&self.control);
            self.runnable.store(false, Ordering::Relaxed);
            !std::mem::replace(&mut c.paused, true) && !c.cancelled
        };
        if changed {
            self.emit(JobEvent::Paused);
        }
    }

    pub(crate) fn resume(&self) {
        let changed = {
            let mut c = lock(&self.control);
            self.runnable.store(!c.cancelled, Ordering::Relaxed);
            std::mem::replace(&mut c.paused, false)
        };
        self.wake.notify_all();
        if changed {
            self.emit(JobEvent::Resumed);
        }
    }

    pub(crate) fn cancel(&self) {
        {
            let mut c = lock(&self.control);
            c.cancelled = true;
            self.runnable.store(false, Ordering::Relaxed);
        }
        self.cancel_flag.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    /// Stops workers and interrupts decoders after an internal failure,
    /// without marking the job as cancelled by the user.
    pub(crate) fn abort(&self) {
        {
            let _c = lock(&self.control);
            self.runnable.store(false, Ordering::Relaxed);
        }
        self.cancel_flag.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel_flag)
    }

    pub(crate) fn is_paused(&self) -> bool {
        lock(&self.control).paused
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        lock(&self.control).cancelled
    }

    /// True while neither paused nor cancelled (non-blocking).
    pub(crate) fn should_run(&self) -> bool {
        self.runnable.load(Ordering::Relaxed)
    }

    /// Blocks (no busy loop) while paused. Returns `false` once the job is
    /// cancelled: the caller must not start new work.
    pub(crate) fn wait_runnable(&self) -> bool {
        let mut c = lock(&self.control);
        while c.paused && !c.cancelled {
            c = self.wake.wait(c).unwrap_or_else(PoisonError::into_inner);
        }
        !c.cancelled
    }

    pub(crate) fn begin_stage(&self, stage: Stage, total: u64) {
        let mut p = lock(&self.progress);
        p.stage = Some(stage);
        p.total = total;
        p.done = 0;
        p.errors = 0;
        p.skipped = 0;
    }

    pub(crate) fn update(&self, s: &StageSummary) {
        let mut p = lock(&self.progress);
        p.done = s.done;
        p.errors = s.errors;
        p.skipped = s.skipped;
    }

    pub(crate) fn mark_finished(&self) {
        lock(&self.progress).finished = true;
    }

    pub(crate) fn snapshot(&self) -> ProgressSnapshot {
        let mut p = lock(&self.progress).clone();
        let c = lock(&self.control);
        p.paused = c.paused;
        p.cancelled = c.cancelled;
        p
    }
}

//! Job pipeline (T1.5, ADR-013): runs the analysis stages over the store in
//! the background, with progress, pause/resume/cancel, and resumability.
//!
//! # Model
//! - Stages ([`Stage`]): `Meta` (format + duration, fast), `Dsp` (analysis
//!   decode, peak, loudness), `Embed` (declared, not implemented yet, so
//!   always skipped). Each has a version; per-file progress lives in
//!   `file_stages`, and a file is *pending* for a stage when it has no row or
//!   an older version. A scan that sees changed content deletes the rows.
//! - **Priorities**: strictly staged across the whole store: every pending
//!   `Meta` file is done before the first `Dsp` file starts. Within a stage
//!   files go by id. Only files of online libraries with status
//!   `new`/`meta`/`analyzed` are eligible.
//! - **Execution**: [`start`] opens its own [`Store`] connection and spawns
//!   the job thread, so the caller never blocks. That thread claims batches,
//!   decodes them in parallel on a rayon pool (one `Decoder` per worker),
//!   and commits each batch in a single transaction. `kill -9` therefore
//!   leaves whole batches; restarting resumes from what is pending.
//! - **Errors**: decode/format errors are recorded (`error` row,
//!   `files.status = 'error'`); damaged-but-readable files are `partial`; an
//!   `Error::Io` records nothing and the file stays pending. A result is
//!   dropped if the file's size/mtime changed since it was claimed.
//! - **Control**: [`JobHandle::pause`] blocks workers between files (no busy
//!   loop), [`JobHandle::cancel`] stops claiming, lets in-flight files
//!   finish and commits them. Progress can be polled
//!   ([`JobHandle::progress`]) or pushed ([`JobOptions::on_event`]).

mod progress;
pub(crate) mod queue;
mod runner;
mod stage;
mod work;

#[cfg(test)]
mod tests;

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::{Error, Result, Store};

pub use progress::{JobEvent, JobSummary, ProgressSnapshot, StageSummary};
pub use stage::{Outcome, Stage, DSP_VERSION, EMBED_VERSION, META_VERSION};

use progress::Shared;

/// Callback receiving [`JobEvent`]s.
pub type EventCallback = Arc<dyn Fn(&JobEvent) + Send + Sync>;

/// Test-only hook, see [`JobOptions::debug_before_file`].
#[doc(hidden)]
pub type BeforeFileHook = Arc<dyn Fn(Stage, &Path) + Send + Sync>;

/// Options for [`start`].
#[derive(Clone)]
pub struct JobOptions {
    /// Stages to run (any order; they always run in priority order, and
    /// `Embed` is skipped until it is implemented). Default: `Meta`, `Dsp`.
    pub stages: Vec<Stage>,
    /// Worker threads for decoding; 0 = available cores.
    pub threads: usize,
    /// Files claimed and committed together (one transaction). Default 128.
    pub batch_size: usize,
    /// Receives progress events; runs on the job thread.
    pub on_event: Option<EventCallback>,
    /// Test only: sleep this long before each file (so a kill test can
    /// interrupt a run reliably).
    #[doc(hidden)]
    pub debug_file_delay: Duration,
    /// Test only: called before each file inside the panic guard.
    #[doc(hidden)]
    pub debug_before_file: Option<BeforeFileHook>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            stages: vec![Stage::Meta, Stage::Dsp],
            threads: 0,
            batch_size: 128,
            on_event: None,
            debug_file_delay: Duration::ZERO,
            debug_before_file: None,
        }
    }
}

impl fmt::Debug for JobOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobOptions")
            .field("stages", &self.stages)
            .field("threads", &self.threads)
            .field("batch_size", &self.batch_size)
            .field("on_event", &self.on_event.is_some())
            .finish_non_exhaustive()
    }
}

/// Handle of a running job. Dropping it without [`JobHandle::wait`] leaves
/// the job running to completion in the background.
pub struct JobHandle {
    shared: Arc<Shared>,
    thread: JoinHandle<Result<JobSummary>>,
}

impl fmt::Debug for JobHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobHandle")
            .field("progress", &self.progress())
            .finish_non_exhaustive()
    }
}

impl JobHandle {
    /// Workers stop taking new files (in-flight files finish and are
    /// committed with their batch).
    pub fn pause(&self) {
        self.shared.pause();
    }

    /// Continues after [`JobHandle::pause`].
    pub fn resume(&self) {
        self.shared.resume();
    }

    /// Stops claiming files; in-flight files finish and are committed.
    /// [`JobHandle::wait`] then returns a summary with `cancelled = true`.
    pub fn cancel(&self) {
        self.shared.cancel();
    }

    /// Whether the job is paused.
    pub fn is_paused(&self) -> bool {
        self.shared.is_paused()
    }

    /// Current progress (cheap, for polling).
    pub fn progress(&self) -> ProgressSnapshot {
        self.shared.snapshot()
    }

    /// True once the job thread has ended.
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Waits for the job to end and returns its summary.
    ///
    /// # Errors
    /// The store/job error that stopped it ([`Error::Sqlite`], ...), or
    /// [`Error::Internal`] if the job thread panicked.
    pub fn wait(self) -> Result<JobSummary> {
        self.thread.join().unwrap_or_else(|_| {
            Err(Error::Internal {
                reason: "job thread panicked".into(),
            })
        })
    }
}

/// Starts a job on a new thread over the store file at `store_path` (opened
/// here, with its own connection; in-memory stores cannot be shared).
///
/// # Errors
/// Whatever [`Store::open`] returns, or [`Error::Internal`] if the thread
/// cannot be spawned. Failures during the run surface from
/// [`JobHandle::wait`].
pub fn start(store_path: &Path, opts: JobOptions) -> Result<JobHandle> {
    let store = Store::open(store_path)?;
    let shared = Shared::new(opts.on_event.clone());
    let for_thread = Arc::clone(&shared);
    let thread = std::thread::Builder::new()
        .name("sonar-jobs".into())
        .spawn(move || {
            // Plain catch_unwind: a coordinator panic is a real bug and keeps
            // the default hook's message and backtrace.
            let out = std::panic::catch_unwind(AssertUnwindSafe(|| {
                runner::run(store, &opts, &for_thread)
            }))
            .unwrap_or_else(|_| {
                Err(Error::Internal {
                    reason: "job thread panicked".into(),
                })
            });
            for_thread.mark_finished();
            out
        })
        .map_err(|e| Error::Internal {
            reason: format!("cannot spawn job thread: {e}"),
        })?;
    Ok(JobHandle { shared, thread })
}

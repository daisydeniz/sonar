//! Indexing control of the engine: start / pause / resume / cancel, polled
//! progress, and the auto-queue of work the watcher finds.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use super::events::EngineEvent;
use super::worker::Msg;
use super::{lock, wait_for, Engine, Inner};
use crate::jobs::{self, JobEvent, JobHandle, JobOptions, JobSummary, ProgressSnapshot, Stage};
use crate::Result;

/// A job that is running (or being joined, `handle` taken).
pub(super) struct RunningJob {
    handle: Option<JobHandle>,
    gen: u64,
}

/// Indexing state behind `Inner::indexing`.
#[derive(Default)]
pub(super) struct IndexState {
    /// Set by `start_indexing`, cleared by `cancel_indexing`: new files
    /// found by scans start a job.
    enabled: bool,
    /// New work arrived while a job ran: rerun when it ends.
    dirty: bool,
    /// Generation of the newest job (stale end notices are ignored).
    gen: u64,
    job: Option<RunningJob>,
    /// Options of the last `start_indexing`, reused by automatic jobs.
    options: JobOptions,
    /// Result of the last finished job, taken by `wait_indexing`.
    last_result: Option<Result<JobSummary>>,
}

/// Point-in-time indexing state, for polling UIs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IndexingStatus {
    /// Indexing is enabled: new files found by scans are analyzed
    /// automatically ([`Engine::start_indexing`] .. [`Engine::cancel_indexing`]).
    pub enabled: bool,
    /// A job is running (possibly paused).
    pub running: bool,
    /// The running job is paused.
    pub paused: bool,
    /// Progress of the running job's current stage.
    pub progress: Option<ProgressSnapshot>,
}

impl Inner {
    /// Starts a job with the stored options; the caller holds the lock and
    /// pushes `IndexingStarted` after releasing it.
    fn begin_job(&self, st: &mut IndexState) -> Result<()> {
        st.gen += 1;
        let gen = st.gen;
        let opts = self.job_options(&st.options, gen);
        let handle = jobs::start(&self.db_path, opts)?;
        st.dirty = false;
        st.last_result = None;
        st.job = Some(RunningJob {
            handle: Some(handle),
            gen,
        });
        Ok(())
    }

    /// `base` with the engine's hook in front of the caller's callback:
    /// committed batches go to the index thread, the end of the job wakes
    /// it. Only the channel is captured, never `Inner`.
    fn job_options(&self, base: &JobOptions, gen: u64) -> JobOptions {
        let mut opts = base.clone();
        let user = opts.on_event.take();
        let tx = self.tx.clone();
        let stale = Arc::clone(&self.semantic.stale);
        opts.on_event = Some(Arc::new(move |event: &JobEvent| {
            if let Some(user) = &user {
                user(event);
            }
            match event {
                JobEvent::Committed { stage, file_ids } => {
                    if *stage == Stage::Embed {
                        stale.store(true, Ordering::SeqCst);
                    }
                    let _ = tx.send(Msg::Refresh(file_ids.clone()));
                }
                JobEvent::Finished | JobEvent::Cancelled => {
                    stale.store(true, Ordering::SeqCst);
                    let _ = tx.send(Msg::JobEnded(gen));
                }
                _ => {}
            }
        }));
        opts
    }

    /// New rows need analysis (watcher or manual scan found files): starts a
    /// job when indexing is enabled and idle, else asks for a rerun.
    pub(super) fn queue_new_work(&self) {
        let started = {
            let mut st = lock(&self.indexing);
            if !st.enabled {
                return;
            }
            if st.job.is_some() {
                st.dirty = true;
                return;
            }
            self.begin_job(&mut st)
        };
        self.report_start(started);
    }

    fn report_start(&self, started: Result<()>) {
        match started {
            Ok(()) => {
                self.events.push(EngineEvent::IndexingStarted);
                let _ = self.tx.try_send(Msg::Wake);
            }
            Err(e) => self.events.push(EngineEvent::Error {
                message: format!("cannot start indexing: {e}"),
            }),
        }
    }

    pub(super) fn job_running(&self) -> bool {
        lock(&self.indexing).job.is_some()
    }

    /// Whether the running job's thread has ended. The worker reads this
    /// *before* draining its channel and passes it to [`Inner::reap_job`],
    /// so every message the job sent is handled before it is collected.
    pub(super) fn job_finished(&self) -> bool {
        lock(&self.indexing)
            .job
            .as_ref()
            .and_then(|j| j.handle.as_ref())
            .is_some_and(JobHandle::is_finished)
    }

    /// Collects a job that ended: `ended` is the generation named by an end
    /// notice (join it, the thread is about to return); `finished` says the
    /// thread was seen finished (a job that failed emits no end notice).
    /// Then reruns if work arrived meanwhile.
    pub(super) fn reap_job(&self, ended: Option<u64>, finished: bool) {
        let handle = {
            let mut st = lock(&self.indexing);
            let Some(job) = st.job.as_mut() else { return };
            let due = Some(job.gen) == ended || finished;
            if !due {
                return;
            }
            match job.handle.take() {
                Some(h) => h,
                None => return,
            }
        };
        let result = handle.wait();
        let (cancelled, error) = match &result {
            Ok(summary) => (summary.cancelled, None),
            Err(e) => (false, Some(e.to_string())),
        };
        let restarted = {
            let mut st = lock(&self.indexing);
            st.job = None;
            st.last_result = Some(result);
            if st.enabled && std::mem::take(&mut st.dirty) {
                Some(self.begin_job(&mut st))
            } else {
                None
            }
        };
        self.indexing_idle.notify_all();
        self.events
            .push(EngineEvent::IndexingFinished { cancelled, error });
        if let Some(started) = restarted {
            self.report_start(started);
        }
    }

    /// Engine shutdown: waits for the job to end while draining the channel
    /// (the job thread may be blocked sending on it).
    pub(super) fn finish_job(&self, rx: &std::sync::mpsc::Receiver<Msg>) {
        let handle = lock(&self.indexing)
            .job
            .as_mut()
            .and_then(|j| j.handle.take());
        if let Some(handle) = handle {
            handle.cancel();
            while !handle.is_finished() {
                while rx.try_recv().is_ok() {}
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = handle.wait();
        }
        lock(&self.indexing).job = None;
        self.indexing_idle.notify_all();
    }
}

impl Engine {
    /// Enables indexing and starts a job (stages `opts.stages`, default
    /// meta + dsp; add `Embed` for CLAP) over everything pending, on
    /// core-owned threads. Returns `false` if a job was already running
    /// (the options then apply to jobs started later).
    ///
    /// While enabled, files added or changed by [`Engine::scan_library`] or
    /// the watcher start another job automatically (see the module docs);
    /// `opts.on_event` also receives those jobs' events.
    ///
    /// # Errors
    /// [`Error::Sqlite`] etc. if the job cannot start.
    pub fn start_indexing(&self, opts: JobOptions) -> Result<bool> {
        let started = {
            let mut st = lock(&self.inner.indexing);
            st.options = opts;
            if st.job.is_some() {
                st.enabled = true;
                false
            } else {
                self.inner.begin_job(&mut st)?;
                st.enabled = true;
                true
            }
        };
        if started {
            self.inner.events.push(EngineEvent::IndexingStarted);
        }
        let _ = self.inner.tx.try_send(Msg::Wake);
        Ok(started)
    }

    /// Pauses the running job (files in flight finish and are committed).
    pub fn pause_indexing(&self) {
        if let Some(h) = lock(&self.inner.indexing)
            .job
            .as_ref()
            .and_then(|j| j.handle.as_ref())
        {
            h.pause();
        }
    }

    /// Continues a paused job.
    pub fn resume_indexing(&self) {
        if let Some(h) = lock(&self.inner.indexing)
            .job
            .as_ref()
            .and_then(|j| j.handle.as_ref())
        {
            h.resume();
        }
    }

    /// Disables indexing and cancels the running job, if any (in-flight
    /// files are committed). [`Engine::start_indexing`] re-enables it.
    pub fn cancel_indexing(&self) {
        let mut st = lock(&self.inner.indexing);
        st.enabled = false;
        st.dirty = false;
        if let Some(h) = st.job.as_ref().and_then(|j| j.handle.as_ref()) {
            h.cancel();
        }
    }

    /// Current indexing state (cheap, for polling).
    pub fn indexing_status(&self) -> IndexingStatus {
        let st = lock(&self.inner.indexing);
        let handle = st.job.as_ref().and_then(|j| j.handle.as_ref());
        IndexingStatus {
            enabled: st.enabled,
            running: st.job.is_some(),
            paused: handle.is_some_and(JobHandle::is_paused),
            progress: handle.map(JobHandle::progress),
        }
    }

    /// Waits until no job is running (at most `timeout`; `None` waits as
    /// long as it takes) and returns the summary of the job that ended
    /// last. On return every batch that job committed is in the text index.
    /// `Ok(None)`: still running at the timeout, or no job has run.
    ///
    /// # Errors
    /// The store/job error that stopped the job.
    pub fn wait_indexing(&self, timeout: Option<Duration>) -> Result<Option<JobSummary>> {
        let idle = wait_for(
            &self.inner.indexing,
            &self.inner.indexing_idle,
            timeout,
            |st| st.job.is_none(),
        );
        if !idle {
            return Ok(None);
        }
        match lock(&self.inner.indexing).last_result.take() {
            Some(Ok(summary)) => Ok(Some(summary)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }
}

//! The `sonar-engine` thread: the single place where the text index is
//! refreshed from background activity and where finished jobs are
//! collected, so those steps never race each other.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use super::events::EngineEvent;
use super::Inner;
use crate::quiet_panic;
use crate::watch::WatchEvent;
use crate::Store;

/// How often a running job is checked for having ended without an end
/// notice (a job that failed sends none).
const JOB_POLL: Duration = Duration::from_millis(250);

/// Messages to the engine thread. Senders block when the channel is full.
pub(super) enum Msg {
    /// Re-read these files into the text index (committed job batch).
    Refresh(Vec<i64>),
    /// A watcher event (its scan's touched ids are refreshed first).
    Watched(WatchEvent),
    /// The job of this generation emitted its final event.
    JobEnded(u64),
    /// State changed; re-evaluate (a job started).
    Wake,
    /// Stop after handling what is queued.
    Shutdown,
}

/// Thread body.
pub(super) fn run(inner: &Arc<Inner>, rx: &Receiver<Msg>) {
    let store = match Store::open(&inner.db_path) {
        Ok(s) => Some(s),
        Err(e) => {
            inner.events.push(EngineEvent::Error {
                message: format!("index sync cannot open the store: {e}"),
            });
            None
        }
    };
    let mut shutdown = false;
    // Ids whose refresh failed, retried next round (see `Retry`).
    let mut retry = Retry::default();
    while !shutdown {
        let first = if inner.job_running() || !retry.ids.is_empty() {
            match rx.recv_timeout(JOB_POLL) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            }
        };
        // A panic in one round must not kill the engine thread: log it and
        // carry on with the next round.
        let round = quiet_panic::guarded(|| {
            handle_round(inner, rx, store.as_ref(), first, &mut retry, &mut shutdown);
        });
        if let Err(message) = round {
            tracing::error!("engine worker round panicked: {message}");
        }
    }
    inner.finish_job(rx);
}

/// One loop iteration: drain the queue, refresh the index, forward watcher
/// events, collect a finished job.
fn handle_round(
    inner: &Arc<Inner>,
    rx: &Receiver<Msg>,
    store: Option<&Store>,
    first: Option<Msg>,
    retry: &mut Retry,
    shutdown: &mut bool,
) {
    // Seen before draining: everything the job sent is then in hand.
    let finished = inner.job_finished();
    let mut ids: Vec<i64> = std::mem::take(&mut retry.ids);
    let mut watched = Vec::new();
    let mut ended = None;
    for msg in first.into_iter().chain(rx.try_iter()) {
        match msg {
            Msg::Refresh(v) => ids.extend(v),
            Msg::Watched(mut event) => {
                // The ids are consumed here; queued events do not carry them.
                if let WatchEvent::Scanned { touched, .. } = &mut event {
                    ids.extend(std::mem::take(touched));
                }
                watched.push(event);
            }
            Msg::JobEnded(gen) => ended = Some(gen),
            Msg::Wake => {}
            Msg::Shutdown => *shutdown = true,
        }
    }
    retry.record(refresh(inner, store, ids));
    for event in watched {
        // `Online`: files whose read failed while the drive was away are
        // still pending and the queue skips offline libraries.
        let new_work = match &event {
            WatchEvent::Scanned { summary, .. } => {
                summary.added + summary.changed + summary.restored > 0
            }
            WatchEvent::Online { .. } => true,
            _ => false,
        };
        inner.events.push(EngineEvent::Watch(event));
        if new_work {
            inner.queue_new_work();
        }
    }
    inner.reap_job(ended, finished);
}

/// Rounds a failing refresh is retried before its ids are dropped (the
/// next scan or analysis of those files refreshes them again anyway).
const MAX_RETRIES: u32 = 5;

/// Ids a refresh could not write (e.g. another process holds the Tantivy
/// writer lock), kept for the next round.
#[derive(Default)]
struct Retry {
    ids: Vec<i64>,
    failures: u32,
}

impl Retry {
    fn record(&mut self, failed: Vec<i64>) {
        if failed.is_empty() {
            self.failures = 0;
            return;
        }
        self.failures += 1;
        if self.failures >= MAX_RETRIES {
            tracing::error!(files = failed.len(), "text index refresh given up");
            self.failures = 0;
        } else {
            self.ids = failed;
        }
    }
}

/// Brings the text index up to date for `ids` (de-duplicated). Returns the
/// ids that could not be written.
fn refresh(inner: &Inner, store: Option<&Store>, mut ids: Vec<i64>) -> Vec<i64> {
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() || !inner.index_ready.load(std::sync::atomic::Ordering::SeqCst) {
        return Vec::new();
    }
    let Some(store) = store else {
        return Vec::new();
    };
    match inner.sync_index(store, &ids) {
        Ok(()) => {
            inner
                .events
                .push(EngineEvent::IndexRefreshed { files: ids.len() });
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("text index refresh failed: {e}");
            inner.events.push(EngineEvent::Error {
                message: format!("text index refresh failed: {e}"),
            });
            ids
        }
    }
}

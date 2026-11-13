//! The `sonar-engine` thread: the single place where the text index is
//! refreshed from background activity and where finished jobs are
//! collected, so those steps never race each other.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use super::events::EngineEvent;
use super::Inner;
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
    while !shutdown {
        let first = if inner.job_running() {
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
        // Seen before draining: everything the job sent is then in hand.
        let finished = inner.job_finished();
        let mut ids: Vec<i64> = Vec::new();
        let mut watched = Vec::new();
        let mut ended = None;
        for msg in first.into_iter().chain(rx.try_iter()) {
            match msg {
                Msg::Refresh(v) => ids.extend(v),
                Msg::Watched(event) => {
                    if let WatchEvent::Scanned { touched, .. } = &event {
                        ids.extend(touched.iter().copied());
                    }
                    watched.push(event);
                }
                Msg::JobEnded(gen) => ended = Some(gen),
                Msg::Wake => {}
                Msg::Shutdown => shutdown = true,
            }
        }
        refresh(inner, store.as_ref(), ids);
        for event in watched {
            let new_work = matches!(
                &event,
                WatchEvent::Scanned { summary, .. }
                    if summary.added + summary.changed + summary.restored > 0
            );
            inner.events.push(EngineEvent::Watch(event));
            if new_work {
                inner.queue_new_work();
            }
        }
        inner.reap_job(ended, finished);
    }
    inner.finish_job(rx);
}

/// Brings the text index up to date for `ids` (de-duplicated).
fn refresh(inner: &Inner, store: Option<&Store>, mut ids: Vec<i64>) {
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() || !inner.index_ready.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let Some(store) = store else { return };
    match inner.index.refresh_files(store, &ids) {
        Ok(()) => inner
            .events
            .push(EngineEvent::IndexRefreshed { files: ids.len() }),
        Err(e) => inner.events.push(EngineEvent::Error {
            message: format!("text index refresh failed: {e}"),
        }),
    }
}

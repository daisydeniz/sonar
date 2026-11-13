//! Engine events: a bounded queue the host polls, or a listener it
//! registers at open.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use crate::watch::WatchEvent;

/// Most events kept for [`super::Engine::poll_events`]; the oldest are
/// dropped beyond it (a UI that never polls cannot grow memory).
pub const EVENT_QUEUE_CAPACITY: usize = 1024;

/// Something that happened in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EngineEvent {
    /// The watcher rescanned a library, or a volume went offline / online.
    /// For `Scanned` the text index already reflects the scan.
    Watch(WatchEvent),
    /// An indexing job (meta, dsp, embed) started, by request or because
    /// the watcher found new files.
    IndexingStarted,
    /// The job ended. `error` is set if the job itself failed (file-level
    /// errors are data, not failures).
    IndexingFinished {
        /// Stopped by [`super::Engine::cancel_indexing`].
        cancelled: bool,
        /// Why the job failed, if it did.
        error: Option<String>,
    },
    /// The text index was brought up to date for this many files.
    IndexRefreshed {
        /// Files re-read from the store (after de-duplication).
        files: usize,
    },
    /// A background step failed (index refresh, job start); the engine
    /// keeps running.
    Error {
        /// Human-readable cause.
        message: String,
    },
}

/// Callback receiving [`EngineEvent`]s on an engine thread; it must be
/// quick and must not call blocking engine methods
/// ([`super::Engine::wait_indexing`], dropping the engine).
pub type EventListener = Arc<dyn Fn(&EngineEvent) + Send + Sync>;

/// Bounded FIFO of events; with a listener, events go to it instead.
pub(super) struct EventQueue {
    queue: Mutex<VecDeque<EngineEvent>>,
    listener: Option<EventListener>,
}

impl EventQueue {
    pub(super) fn new(listener: Option<EventListener>) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            listener,
        }
    }

    pub(super) fn push(&self, event: EngineEvent) {
        if let Some(listener) = &self.listener {
            if crate::quiet_panic::guarded(|| listener(&event)).is_err() {
                tracing::warn!("engine event listener panicked on {event:?}");
            }
            return;
        }
        let mut q = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if q.len() >= EVENT_QUEUE_CAPACITY {
            q.pop_front();
        }
        q.push_back(event);
    }

    pub(super) fn drain(&self) -> Vec<EngineEvent> {
        let mut q = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        q.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_bounded_and_drains_in_order() {
        let q = EventQueue::new(None);
        for i in 0..EVENT_QUEUE_CAPACITY + 5 {
            q.push(EngineEvent::IndexRefreshed { files: i });
        }
        let all = q.drain();
        assert_eq!(all.len(), EVENT_QUEUE_CAPACITY);
        assert_eq!(all[0], EngineEvent::IndexRefreshed { files: 5 });
        assert!(q.drain().is_empty());
    }

    #[test]
    fn listener_replaces_the_queue() {
        let seen = Arc::new(Mutex::new(0usize));
        let s = Arc::clone(&seen);
        let listener: EventListener = Arc::new(move |_| {
            *s.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        });
        let q = EventQueue::new(Some(listener));
        q.push(EngineEvent::IndexingStarted);
        assert_eq!(*seen.lock().unwrap_or_else(PoisonError::into_inner), 1);
        assert!(q.drain().is_empty());
    }
}

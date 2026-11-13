//! File watching (T1.6, ADR-014): keeps the index in line with the disk
//! while the app or `sonar watch` runs.
//!
//! # Strategy
//! Events are only *triggers*. FSEvents (through `notify`) tells us that
//! something changed under a library root; the watcher then runs the
//! incremental [`scan_library_with`] for that library, whose size+mtime fast
//! path makes a no-op rescan cheap and whose library-wide view is what move
//! detection needs (a renamed folder moves every file below it). There are
//! no per-event database edits.
//!
//! - **Filter** ([`filter`]): events only on dot-files or non-audio files
//!   are dropped; directory events, renames and unknown kinds always count.
//!   A `need_rescan`/overflow flag or a watcher error also triggers a scan.
//! - **Debounce** ([`debounce`]): events of one library are coalesced; the
//!   scan runs after [`WatchOptions::debounce`] of silence (default 400 ms)
//!   but at most [`WatchOptions::max_delay`] (5 s) after the first event of
//!   a burst, so a long copy is still scanned periodically.
//! - **Serialization**: the watcher thread runs scans one at a time, so two
//!   scans of one library never overlap (concurrent scans may fail, T1.2).
//!   Events arriving during a scan are queued and re-debounced afterwards.
//! - **Offline volumes**: every [`WatchOptions::root_poll`] (2 s) each
//!   root's existence is checked. A root that vanished drops its notify
//!   watch (notify cannot watch a missing path) and triggers a scan, which
//!   marks the library `online = 0` and keeps all rows. A root that is back
//!   gets a fresh notify watch and an immediate scan (`online = 1`). While a
//!   library is offline but its root exists, the scan is retried each poll.
//!   The same poll picks up libraries added to the store after start.
//! - **Start**: every library is scanned once at start, which catches what
//!   changed while nothing was watching and settles the `online` flag.
//! - **No analysis here**: the caller (app, CLI) runs [`crate::jobs`] when it
//!   wants new files analyzed.
//!
//! # Concurrency with the job pipeline
//! The watcher and a running job use separate connections to the same WAL
//! database. The scan walks and hashes outside its write transaction; jobs
//! commit short batches. A writer that finds the database locked waits up to
//! the store's 5 s busy timeout. If a scan still fails (`SQLITE_BUSY`),
//! the watcher reports [`WatchEvent::Error`] and retries after
//! [`WatchOptions::error_retry`]. The job pipeline drops a result whose
//! file was reset by a scan in between (stale-claim check).
//!
//! # Robustness
//! The callback and the scan run under [`crate::quiet_panic::guarded`]; a
//! panic becomes a [`WatchEvent::Error`] and the watcher keeps running. A
//! panic anywhere else ends the thread; the host then gets a last
//! `WatchEvent::Error { library_id: None, .. }` ("watcher stopped ...") and
//! [`WatchHandle::is_running`] turns false, so hosts may also poll that.
//!
//! # Spacing of rescans
//! After a scan of a library, event-driven rescans of it wait at least
//! `2 x scan duration` (besides the quiet period), so a long copy into a
//! large library is not walked back to back. In a sustained copy this
//! bounds the duty cycle to about 1/3 (measured: 3-12 % at 250 files/s on
//! 50k files). The bound is per sustained stream: the first scan of a burst
//! is not delayed, so a short burst of e.g. 20,000 files followed by a quiet
//! period spends 31-43 % of burst plus catch-up time in that one scan (still
//! under 2 s on 50k files, see ADR-014). Small libraries react within the
//! debounce. Root appearance/disappearance, manual rescans and retries
//! bypass this spacing. A root that exists but is
//! offline in the index (unlistable, stale mount point) is retried every
//! [`WatchOptions::error_retry`].

mod debounce;
mod filter;
mod runner;

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;

use crate::scan::ScanReport;
use crate::{Error, Result, Store};

pub use debounce::Debouncer;
pub use filter::{is_relevant, probe_fs, Probe};

/// Tunables of the watcher. Defaults suit the app; tests shrink them.
#[derive(Debug, Clone)]
pub struct WatchOptions {
    /// Silence required after the last event before a library is rescanned.
    pub debounce: Duration,
    /// Longest a continuous burst of events can postpone the rescan.
    pub max_delay: Duration,
    /// How often root existence and the library list are checked.
    pub root_poll: Duration,
    /// Wait before retrying a scan that failed.
    pub error_retry: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(400),
            max_delay: Duration::from_secs(5),
            root_poll: Duration::from_secs(2),
            error_retry: Duration::from_secs(5),
        }
    }
}

/// Outcome counts of one watcher-triggered scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScanSummary {
    /// New files.
    pub added: usize,
    /// Files whose content changed.
    pub changed: usize,
    /// Files that moved or were renamed (row kept).
    pub moved: usize,
    /// Files newly marked missing.
    pub removed: usize,
    /// Missing files that came back.
    pub restored: usize,
    /// Unreadable files.
    pub errors: usize,
    /// Non-fatal problems met.
    pub issues: usize,
    /// Scan wall-clock time in milliseconds.
    pub duration_ms: u64,
}

impl ScanSummary {
    fn of(report: &ScanReport) -> Self {
        let c = &report.counts;
        Self {
            added: c.added,
            changed: c.changed,
            moved: c.moved,
            removed: c.removed,
            restored: c.restored,
            errors: c.errors,
            issues: report.issues.len(),
            duration_ms: u64::try_from(report.duration.as_millis()).unwrap_or(u64::MAX),
        }
    }

    /// True if the scan changed any row.
    pub fn changed_anything(&self) -> bool {
        self.added + self.changed + self.moved + self.removed + self.restored + self.errors > 0
    }
}

/// What the watcher reports. Delivered on the watcher thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum WatchEvent {
    /// A library was rescanned (online). Also sent when nothing changed.
    Scanned {
        /// Library scanned.
        library_id: i64,
        /// What the scan did.
        summary: ScanSummary,
        /// Ids of the rows the scan touched (for index refresh); not part
        /// of the JSON form.
        #[serde(skip)]
        touched: Vec<i64>,
    },
    /// The library root became unavailable; rows are kept.
    Offline {
        /// Library concerned.
        library_id: i64,
    },
    /// The library root is available again (sent before its `Scanned`).
    Online {
        /// Library concerned.
        library_id: i64,
    },
    /// Something failed; the watcher keeps running and retries.
    Error {
        /// Library concerned, if any.
        library_id: Option<i64>,
        /// Human-readable cause.
        message: String,
    },
}

/// Callback receiving [`WatchEvent`]s on the watcher thread.
pub type EventCallback = Box<dyn Fn(&WatchEvent) + Send + 'static>;

/// Messages to the watcher thread.
pub(crate) enum Msg {
    /// A relevant filesystem event (or watcher error) for a library.
    Fs { library_id: i64 },
    /// Re-read the library list and root states now.
    Refresh,
    /// Rescan every library now.
    RescanAll,
    /// Stop.
    Stop,
}

/// Handle of a running watcher. Dropping it stops the watcher.
pub struct WatchHandle {
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for WatchHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatchHandle")
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

impl WatchHandle {
    /// Rescans every library as soon as possible.
    pub fn rescan_now(&self) {
        let _ = self.tx.send(Msg::RescanAll);
    }

    /// Re-reads the library list now (instead of at the next poll), so a
    /// library just added to the store is watched and scanned.
    pub fn refresh_libraries(&self) {
        let _ = self.tx.send(Msg::Refresh);
    }

    /// True while the watcher thread is alive.
    pub fn is_running(&self) -> bool {
        self.thread
            .lock()
            .ok()
            .and_then(|t| t.as_ref().map(|j| !j.is_finished()))
            .unwrap_or(false)
    }

    /// Stops the watcher and joins its thread. A scan in progress is
    /// cancelled (it writes nothing). Idempotent.
    ///
    /// Called from the watcher thread itself (inside the event callback)
    /// it only requests the stop and returns without joining, since a
    /// thread cannot join itself; the thread then ends after the callback
    /// returns.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
        let joined = self.thread.lock().ok().and_then(|mut t| {
            let on_watcher_thread = t
                .as_ref()
                .is_some_and(|j| j.thread().id() == std::thread::current().id());
            if on_watcher_thread {
                None
            } else {
                t.take()
            }
        });
        if let Some(j) = joined {
            let _ = j.join();
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts watching every library of the store file at `store_path`
/// (opened here with its own connection) on a new thread. Libraries added
/// later are picked up at the next poll or [`WatchHandle::refresh_libraries`].
///
/// # Errors
/// Whatever [`Store::open`] returns, or [`Error::Internal`] if the thread
/// cannot be spawned. Problems during the run arrive as
/// [`WatchEvent::Error`].
pub fn start(
    store_path: &Path,
    options: WatchOptions,
    on_event: impl Fn(&WatchEvent) + Send + 'static,
) -> Result<WatchHandle> {
    let store = Store::open(store_path)?;
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let mut ctx = runner::Context {
        store,
        options,
        tx: tx.clone(),
        rx,
        stop: Arc::clone(&stop),
        on_event: Box::new(on_event),
    };
    let thread = std::thread::Builder::new()
        .name("sonar-watch".into())
        .spawn(move || {
            if let Err(msg) = crate::quiet_panic::guarded(|| runner::run(&mut ctx)) {
                tracing::error!("watcher thread panicked: {msg}");
                // Last word to the host: the watcher is no longer running.
                let event = WatchEvent::Error {
                    library_id: None,
                    message: format!("watcher stopped after an internal panic: {msg}"),
                };
                let _ = crate::quiet_panic::guarded(|| (ctx.on_event)(&event));
            }
        })
        .map_err(|e| Error::Internal {
            reason: format!("cannot spawn watcher thread: {e}"),
        })?;
    Ok(WatchHandle {
        tx,
        stop,
        thread: Mutex::new(Some(thread)),
    })
}

//! The watcher thread: owns the store connection, the per-library notify
//! watchers and the debouncer, and runs the scans (see the module docs of
//! [`super`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Instant;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::debounce::Debouncer;
use super::filter::{is_relevant, probe_fs};
use super::{EventCallback, Msg, ScanSummary, WatchEvent, WatchOptions};
use crate::scan::{scan_library_with, ScanOptions};
use crate::{quiet_panic, Error, Store};

/// Everything the thread needs, moved in at spawn.
pub(super) struct Context {
    pub store: Store,
    pub options: WatchOptions,
    pub tx: Sender<Msg>,
    pub rx: Receiver<Msg>,
    pub stop: Arc<AtomicBool>,
    pub on_event: EventCallback,
}

/// What we know about one watched library.
struct LibState {
    root: PathBuf,
    /// Active notify watch; `None` while the root is missing or unwatchable.
    watcher: Option<RecommendedWatcher>,
    /// The root existed at the last poll.
    present: bool,
    /// `libraries.online` as of the last scan.
    online: bool,
    /// A watch failure was already reported (report once per streak).
    watch_error_reported: bool,
    /// When the root was last rescanned because it is present but the index
    /// says offline (unlistable, stale mount point); throttles that retry
    /// to [`WatchOptions::error_retry`].
    last_offline_retry: Option<Instant>,
}

struct Runner<'a> {
    ctx: &'a mut Context,
    libs: BTreeMap<i64, LibState>,
    deb: Debouncer,
}

/// Runs until stopped. Takes the context by reference so the caller can
/// still report to the callback if this panics.
pub(super) fn run(ctx: &mut Context) {
    let deb = Debouncer::new(ctx.options.debounce, ctx.options.max_delay);
    let mut r = Runner {
        ctx,
        libs: BTreeMap::new(),
        deb,
    };
    r.main_loop();
}

fn root_present(root: &Path) -> bool {
    std::fs::metadata(root).is_ok_and(|m| m.is_dir())
}

impl Runner<'_> {
    fn stopped(&self) -> bool {
        self.ctx.stop.load(Ordering::SeqCst)
    }

    fn emit(&self, event: &WatchEvent) {
        // A panicking callback must not take the watcher down.
        let _ = quiet_panic::guarded(|| (self.ctx.on_event)(event));
    }

    fn error(&self, library_id: Option<i64>, message: String) {
        tracing::warn!(?library_id, "watch: {message}");
        self.emit(&WatchEvent::Error {
            library_id,
            message,
        });
    }

    fn main_loop(&mut self) {
        let mut next_poll = Instant::now();
        while !self.stopped() {
            let now = Instant::now();
            if now >= next_poll {
                self.poll(now);
                next_poll = now + self.ctx.options.root_poll;
            }
            for id in self.deb.take_due(Instant::now()) {
                if self.stopped() {
                    return;
                }
                self.scan(id);
            }
            let now = Instant::now();
            let until_poll = next_poll.saturating_duration_since(now);
            let wait = self
                .deb
                .next_wait(now)
                .map_or(until_poll, |w| w.min(until_poll));
            match self.ctx.rx.recv_timeout(wait) {
                Ok(msg) => {
                    if !self.handle(msg, &mut next_poll) {
                        return;
                    }
                    while let Ok(msg) = self.ctx.rx.try_recv() {
                        if !self.handle(msg, &mut next_poll) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Returns false to stop.
    fn handle(&mut self, msg: Msg, next_poll: &mut Instant) -> bool {
        let now = Instant::now();
        match msg {
            Msg::Fs { library_id } => {
                if self.libs.contains_key(&library_id) {
                    self.deb.note(library_id, now);
                }
            }
            Msg::Refresh => *next_poll = now,
            Msg::RescanAll => {
                for id in self.libs.keys() {
                    self.deb.urgent(*id, now);
                }
            }
            Msg::Stop => return false,
        }
        true
    }

    /// Syncs the library list and root states; schedules scans on changes.
    fn poll(&mut self, now: Instant) {
        let listed = match self.ctx.store.libraries() {
            Ok(l) => l,
            Err(e) => return self.error(None, format!("cannot list libraries: {e}")),
        };
        let gone: Vec<i64> = self
            .libs
            .keys()
            .copied()
            .filter(|id| !listed.iter().any(|l| l.id == *id))
            .collect();
        for id in gone {
            self.libs.remove(&id);
            self.deb.forget(id);
        }
        for lib in listed {
            let present = root_present(&lib.root_path);
            let is_new = !self.libs.contains_key(&lib.id);
            let st = self.libs.entry(lib.id).or_insert_with(|| LibState {
                root: lib.root_path.clone(),
                watcher: None,
                present: false,
                online: lib.online,
                watch_error_reported: false,
                last_offline_retry: None,
            });
            if st.root != lib.root_path {
                // Root changed in the store: drop the old watch, start over.
                st.root.clone_from(&lib.root_path);
                st.watcher = None;
                st.present = false;
                st.watch_error_reported = false;
            }
            let changed = present != st.present;
            let mut urgent = is_new || changed;
            st.present = present;
            if !present {
                st.watcher = None; // notify cannot watch a missing path
                st.watch_error_reported = false;
            } else if st.watcher.is_none() {
                // Fresh root or an earlier watch failure: (re)establish.
                urgent |= self.try_watch(lib.id);
            }
            // Offline in the index but present on disk (unlistable, stale
            // mount point): retry, but no faster than `error_retry`.
            let retry = self.ctx.options.error_retry;
            if let Some(st) = self.libs.get_mut(&lib.id) {
                if present && !st.online {
                    if urgent {
                        st.last_offline_retry = Some(now);
                    } else if st
                        .last_offline_retry
                        .is_none_or(|t| now.duration_since(t) >= retry)
                    {
                        st.last_offline_retry = Some(now);
                        urgent = true;
                    }
                }
            }
            if urgent {
                self.deb.urgent(lib.id, now);
            }
        }
    }

    /// Creates the notify watch for `id`. Returns true if it was
    /// established (the caller then scans, so nothing between is missed).
    fn try_watch(&mut self, id: i64) -> bool {
        let Some(st) = self.libs.get(&id) else {
            return false;
        };
        let root = st.root.clone();
        let tx = self.ctx.tx.clone();
        let cb_root = root.clone();
        let handler = move |res: notify::Result<notify::Event>| {
            let hit = match res {
                Ok(ev) => ev.need_rescan() || is_relevant(&ev.kind, &ev.paths, &cb_root, probe_fs),
                Err(e) => {
                    tracing::warn!("notify error for {}: {e}", cb_root.display());
                    true
                }
            };
            if hit {
                let _ = tx.send(Msg::Fs { library_id: id });
            }
        };
        let result = notify::recommended_watcher(handler).and_then(|mut w| {
            w.watch(&root, RecursiveMode::Recursive)?;
            Ok(w)
        });
        match result {
            Ok(w) => {
                if let Some(st) = self.libs.get_mut(&id) {
                    st.watcher = Some(w);
                    st.watch_error_reported = false;
                }
                true
            }
            Err(e) => {
                let first = self
                    .libs
                    .get_mut(&id)
                    .is_some_and(|st| !std::mem::replace(&mut st.watch_error_reported, true));
                if first {
                    self.error(Some(id), format!("cannot watch {}: {e}", root.display()));
                }
                false
            }
        }
    }

    /// Scans one library and reports the outcome.
    fn scan(&mut self, id: i64) {
        if !self.libs.contains_key(&id) {
            return;
        }
        let opts = ScanOptions {
            cancel: Some(Arc::clone(&self.ctx.stop)),
        };
        let store = &mut self.ctx.store;
        let result = quiet_panic::guarded(|| scan_library_with(store, id, &opts));
        let failure = match result {
            Ok(Ok(report)) => {
                // Event-driven rescans of this library are spaced out, so a
                // long copy into a large library cannot cause back-to-back
                // full walks.
                self.deb.scan_finished(id, Instant::now(), report.duration);
                self.report(id, &report);
                return;
            }
            Ok(Err(Error::Cancelled)) => return,
            Ok(Err(Error::LibraryNotFound { .. })) => {
                self.libs.remove(&id); // removed from the store meanwhile
                return;
            }
            Ok(Err(e)) => e.to_string(),
            Err(panic) => format!("scan panicked: {panic}"),
        };
        self.error(Some(id), format!("scan failed: {failure}"));
        let now = Instant::now();
        self.deb
            .retry_at(id, now, now + self.ctx.options.error_retry);
    }

    fn report(&mut self, id: i64, report: &crate::scan::ScanReport) {
        let was_online = self.libs.get(&id).is_none_or(|st| st.online);
        if let Some(st) = self.libs.get_mut(&id) {
            st.online = !report.offline;
        }
        if report.offline {
            if was_online {
                self.emit(&WatchEvent::Offline { library_id: id });
            }
            return;
        }
        if !was_online {
            self.emit(&WatchEvent::Online { library_id: id });
        }
        self.emit(&WatchEvent::Scanned {
            library_id: id,
            summary: ScanSummary::of(report),
            touched: report.touched.clone(),
        });
    }
}

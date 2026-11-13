//! The engine facade (T5.5a, ADR-030): one long-lived object that owns
//! everything the app (through the FFI) and the CLI need, so neither wires
//! store, index, watcher and jobs by hand.
//!
//! # Ownership
//! An [`Engine`] owns, for one data directory (`home`, see [`crate::paths`]):
//! the SQLite [`Store`] (one connection for quick queries), the Tantivy
//! [`TextIndex`], the [`Synonyms`], an optional watcher, at most one running
//! indexing job, a lazily loaded CLAP query side ([`SemanticSearch`], text
//! search only when the model or embeddings are missing) and one background
//! thread (`sonar-engine`) that keeps the text index in step with the store.
//! Scans, the watcher and jobs open their own connections.
//!
//! # Threading
//! `Engine` is `Send + Sync`; every method takes `&self` and may be called
//! from any thread (the FFI calls it from Swift background threads). Shared
//! state sits behind short mutexes that are never held across a blocking
//! operation; the only blocking methods are [`Engine::add_library`] /
//! [`Engine::scan_library`] (a scan), [`Engine::wait_indexing`] and
//! [`Engine::search`]. Dropping the engine stops the watcher, cancels a
//! running job (in-flight files are committed) and joins the thread.
//!
//! # Index sync (no full rebuilds)
//! The text index is rebuilt in full only when it is missing, unreadable or
//! of another version at open ([`EngineOptions::rebuild_index`]). Afterwards
//! it is refreshed with [`TextIndex::refresh_files`] for exactly the touched
//! ids: a scan run by [`Engine::scan_library`] refreshes before it returns;
//! the watcher's scans (`WatchEvent::Scanned::touched`) and every committed
//! job batch ([`JobEvent::Committed`]) go through a bounded channel to the
//! `sonar-engine` thread, which coalesces what is queued into one refresh.
//! Consequences: when a `Scanned` [`EngineEvent`] is visible the index
//! reflects that scan, and when [`Engine::wait_indexing`] returns every
//! committed batch is searchable (kind, BPM, key and category filters).
//!
//! # Indexing and the watcher
//! [`Engine::start_indexing`] *enables* indexing and starts a job over
//! everything pending. While enabled and idle ("idle-but-enabled"), a
//! watcher or manual scan that adds, changes or restores files queues
//! another job automatically; if a job is running at that moment a rerun
//! follows when it ends (so a file added after the job passed its stage
//! cursor is never missed). [`Engine::cancel_indexing`] disables it (a
//! later [`Engine::start_indexing`] re-enables). Without `start_indexing`
//! the watcher only keeps rows and index fresh, as `sonar watch` does.
//!
//! # Search
//! [`Engine::search`] is cancellable with latest-query-wins semantics; see
//! `search.rs`.

mod events;
mod indexing;
mod libraries;
mod search;
mod worker;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use events::{EngineEvent, EventListener, EVENT_QUEUE_CAPACITY};
pub use indexing::IndexingStatus;
pub use search::SearchRequest;

use crate::embed::EmbedOptions;
use crate::index::{BuildReport, OpenState, RebuildReason, TextIndex, INDEX_SUBDIR};
use crate::search::{Synonyms, SYNONYMS_FILE};
use crate::watch::{self, WatchHandle, WatchOptions};
use crate::{paths, Error, Result, Store};

use events::EventQueue;
use worker::Msg;

/// Messages the background thread can have queued before senders block
/// (back-pressure on the job and watcher threads).
const CHANNEL_CAPACITY: usize = 256;

/// Settings of [`Engine::open_with`].
#[derive(Clone)]
pub struct EngineOptions {
    /// Receives [`EngineEvent`]s on an engine thread. `None`: events queue
    /// up for [`Engine::poll_events`].
    pub on_event: Option<EventListener>,
    /// Build the text index at open if it is missing, unreadable or of
    /// another version (default). With `false` (CLI commands that only
    /// maintain an index that already exists) a missing index is left
    /// alone, not synced, and built on the first search.
    pub rebuild_index: bool,
    /// Synonyms file instead of `<home>/synonyms.tsv` / built-in groups.
    pub synonyms_file: Option<PathBuf>,
    /// CLAP models for the semantic leg of [`Engine::search`].
    pub embed: EmbedOptions,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            on_event: None,
            rebuild_index: true,
            synonyms_file: None,
            embed: EmbedOptions::default(),
        }
    }
}

impl std::fmt::Debug for EngineOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineOptions")
            .field("on_event", &self.on_event.is_some())
            .field("rebuild_index", &self.rebuild_index)
            .field("synonyms_file", &self.synonyms_file)
            .finish_non_exhaustive()
    }
}

/// State shared between the [`Engine`], its thread and the callbacks it
/// hands to the watcher and jobs (they hold only the channel, not this).
pub(crate) struct Inner {
    home: PathBuf,
    db_path: PathBuf,
    options: EngineOptions,
    /// Quick queries (libraries, paths); scans and jobs have their own.
    store: Mutex<Store>,
    index: TextIndex,
    /// False when the index was left unbuilt ([`EngineOptions::rebuild_index`]).
    index_ready: AtomicBool,
    synonyms: Synonyms,
    events: EventQueue,
    tx: SyncSender<Msg>,
    indexing: Mutex<indexing::IndexState>,
    indexing_idle: std::sync::Condvar,
    watcher: Mutex<Option<WatchHandle>>,
    semantic: search::SemanticCache,
    /// Cancel flag of the newest search.
    search_cancel: Mutex<Arc<AtomicBool>>,
}

/// A refresh of at least this many files, or of more than a quarter of the
/// index, is done as a full rebuild: one pass over the store beats one
/// query per id (a first scan of a big library keeps the rebuild cost).
const REBUILD_MIN_IDS: usize = 1000;
/// Refreshes above this many ids always rebuild.
const REBUILD_ALWAYS_IDS: usize = 50_000;

impl Inner {
    /// Whether refreshing `n` files is better done as a full rebuild.
    fn prefers_rebuild(&self, n: usize) -> bool {
        n > REBUILD_ALWAYS_IDS
            || (n >= REBUILD_MIN_IDS && (n as u64).saturating_mul(4) > self.index.num_docs())
    }

    /// Brings the index up to date for `ids`: [`TextIndex::refresh_files`],
    /// or a full rebuild when the set is large (see [`REBUILD_MIN_IDS`]).
    pub(crate) fn sync_index(&self, store: &Store, ids: &[i64]) -> Result<()> {
        if self.prefers_rebuild(ids.len()) {
            self.index.rebuild(store).map(|_| ())
        } else {
            self.index.refresh_files(store, ids)
        }
    }
}

/// Locks a mutex, treating poison as recoverable (a panic elsewhere must
/// not take the whole engine down; the guarded data stays consistent).
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The engine facade, see the [module docs](self).
pub struct Engine {
    inner: Arc<Inner>,
    thread: Mutex<Option<JoinHandle<()>>>,
    startup_rebuild: Option<(RebuildReason, BuildReport)>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("home", &self.inner.home)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Opens the engine over the default data directory (`SONAR_HOME` or
    /// the macOS Application Support directory).
    ///
    /// # Errors
    /// [`Error::NoDataDir`] plus everything [`Engine::open`] returns.
    pub fn open_default() -> Result<Self> {
        Self::open(paths::data_dir()?)
    }

    /// Opens the engine over the data directory `home` with default options.
    ///
    /// # Errors
    /// See [`Engine::open_with`].
    pub fn open(home: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(home, EngineOptions::default())
    }

    /// Opens (creating if needed) `<home>/sonar.db`, `<home>/index/text`
    /// and loads the synonyms. A missing or unusable text index is rebuilt
    /// from the store (see [`Engine::startup_rebuild`]).
    ///
    /// # Errors
    /// Whatever [`Store::open`] / [`TextIndex::open`] return, plus
    /// [`Error::Io`] for an unreadable explicit synonyms file and
    /// [`Error::Internal`] if the thread cannot be spawned.
    pub fn open_with(home: impl AsRef<Path>, options: EngineOptions) -> Result<Self> {
        let home = std::path::absolute(home.as_ref()).map_err(|e| Error::io(home.as_ref(), e))?;
        let db_path = home.join(paths::DB_FILE_NAME);
        let store = Store::open(&db_path)?;
        let mut index_dir = home.clone();
        index_dir.extend(INDEX_SUBDIR);
        let (index, state) = TextIndex::open(index_dir)?;
        let mut startup_rebuild = None;
        let mut ready = true;
        if let OpenState::NeedsRebuild(reason) = state {
            if options.rebuild_index {
                startup_rebuild = Some((reason, index.rebuild(&store)?));
            } else {
                ready = false;
            }
        }
        let synonyms = match &options.synonyms_file {
            Some(path) => Synonyms::load(path)?,
            None => Synonyms::load_or_builtin(home.join(SYNONYMS_FILE))?,
        };
        let (tx, rx) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let inner = Arc::new(Inner {
            home,
            db_path,
            store: Mutex::new(store),
            index,
            index_ready: AtomicBool::new(ready),
            synonyms,
            events: EventQueue::new(options.on_event.clone()),
            tx,
            indexing: Mutex::new(indexing::IndexState::default()),
            indexing_idle: std::sync::Condvar::new(),
            watcher: Mutex::new(None),
            semantic: search::SemanticCache::default(),
            search_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            options,
        });
        let for_thread = Arc::clone(&inner);
        let thread = std::thread::Builder::new()
            .name("sonar-engine".into())
            .spawn(move || worker::run(&for_thread, &rx))
            .map_err(|e| Error::Internal {
                reason: format!("cannot spawn engine thread: {e}"),
            })?;
        Ok(Self {
            inner,
            thread: Mutex::new(Some(thread)),
            startup_rebuild,
        })
    }

    /// The data directory this engine works in.
    pub fn home(&self) -> &Path {
        &self.inner.home
    }

    /// Why and how fast the text index was rebuilt at open, if it was.
    pub fn startup_rebuild(&self) -> Option<&(RebuildReason, BuildReport)> {
        self.startup_rebuild.as_ref()
    }

    /// Number of documents in the text index.
    pub fn text_index_docs(&self) -> u64 {
        self.inner.index.num_docs()
    }

    /// Whether the text index is built and kept in sync (false only with
    /// [`EngineOptions::rebuild_index`] off and no index on disk).
    pub fn text_index_ready(&self) -> bool {
        self.inner
            .index_ready
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Takes the queued events (empty if a listener was set at open).
    pub fn poll_events(&self) -> Vec<EngineEvent> {
        self.inner.events.drain()
    }

    /// Starts watching every library: the watcher scans each once, then
    /// again after filesystem changes, and the index follows (see the
    /// module docs). No-op if already watching.
    ///
    /// # Errors
    /// What [`watch::start`] returns.
    pub fn start_watching(&self, options: WatchOptions) -> Result<()> {
        let mut slot = lock(&self.inner.watcher);
        if slot.as_ref().is_some_and(WatchHandle::is_running) {
            return Ok(());
        }
        let tx = self.inner.tx.clone();
        let handle = watch::start(&self.inner.db_path, options, move |event| {
            let _ = tx.send(Msg::Watched(event.clone()));
        })?;
        *slot = Some(handle);
        Ok(())
    }

    /// Stops the watcher (waits for a scan in progress to abort). No-op if
    /// not watching.
    pub fn stop_watching(&self) {
        let handle = lock(&self.inner.watcher).take();
        drop(handle);
    }

    /// Whether the watcher thread is alive.
    pub fn is_watching(&self) -> bool {
        lock(&self.inner.watcher)
            .as_ref()
            .is_some_and(WatchHandle::is_running)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Watcher first (the thread keeps draining while it winds down),
        // then the job, then the thread itself.
        self.stop_watching();
        self.cancel_indexing();
        let _ = self.inner.tx.send(Msg::Shutdown);
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }
}

/// Waits on `cv` until `done(&mut state)` or `timeout` passes.
/// Returns whether `done` held.
pub(crate) fn wait_for<T>(
    mutex: &Mutex<T>,
    cv: &std::sync::Condvar,
    timeout: Option<Duration>,
    mut done: impl FnMut(&mut T) -> bool,
) -> bool {
    let deadline = timeout.and_then(|t| Instant::now().checked_add(t));
    let mut guard = lock(mutex);
    loop {
        if done(&mut guard) {
            return true;
        }
        let wait = match deadline {
            Some(d) => match d.checked_duration_since(Instant::now()) {
                Some(left) if !left.is_zero() => left,
                _ => return false,
            },
            None => Duration::from_secs(3600),
        };
        guard = cv
            .wait_timeout(guard, wait)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Engine>();
};

//! Libraries: add (and scan), rescan, remove, list.

use std::path::Path;
use std::sync::atomic::Ordering;

use super::{lock, Engine};
use crate::scan::{self, ScanReport};
use crate::{Library, Result, Store};

impl Engine {
    /// All libraries, ordered by id.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn libraries(&self) -> Result<Vec<Library>> {
        lock(&self.inner.store).libraries()
    }

    /// Absolute path of file `id`, `None` if unknown.
    ///
    /// # Errors
    /// [`Error::Sqlite`](crate::Error::Sqlite).
    pub fn file_path(&self, id: i64) -> Result<Option<std::path::PathBuf>> {
        lock(&self.inner.store).file_path(id)
    }

    /// Id of the file at `path` (inside a registered library), `None` if
    /// unknown.
    ///
    /// # Errors
    /// [`Error::Sqlite`](crate::Error::Sqlite).
    pub fn file_id_by_path(&self, path: &Path) -> Result<Option<i64>> {
        lock(&self.inner.store).file_id_by_path(path)
    }

    /// Registers `root` as a library (idempotent) and scans it. When
    /// indexing is enabled the new files are analyzed automatically.
    ///
    /// A root that cannot be opened but is already registered (an unmounted
    /// drive) is rescanned instead, which yields its `offline` report.
    ///
    /// # Errors
    /// [`Error::Io`] if `root` is not a directory and not registered, plus
    /// the errors of [`Engine::scan_library`].
    pub fn add_library(&self, root: impl AsRef<Path>) -> Result<(Library, ScanReport)> {
        let root = root.as_ref();
        let library = {
            let store = lock(&self.inner.store);
            match store.add_library(root) {
                Ok(lib) => lib,
                Err(add_err) => match store.library_by_root(root)? {
                    Some(lib) => lib,
                    None => return Err(add_err),
                },
            }
        };
        let report = self.scan_library(library.id)?;
        // Re-read: the scan may have set the online flag.
        let library = lock(&self.inner.store)
            .library(library.id)?
            .unwrap_or(library);
        Ok((library, report))
    }

    /// Scans one library (incremental) on the calling thread and refreshes
    /// the text index for exactly the files the scan touched before it
    /// returns. Runs on its own connection, so other calls stay responsive;
    /// a concurrent scan of the same library (e.g. by the watcher) waits for
    /// the other one to finish (`scan::lock`).
    ///
    /// # Errors
    /// [`Error::LibraryNotFound`], [`Error::Sqlite`], [`Error::Index`].
    pub fn scan_library(&self, library_id: i64) -> Result<ScanReport> {
        let mut store = Store::open(&self.inner.db_path)?;
        let was_offline = store.library(library_id)?.is_some_and(|l| !l.online);
        let report = scan::scan_library(&mut store, library_id)?;
        if !report.touched.is_empty() && self.inner.index_ready.load(Ordering::SeqCst) {
            self.inner.sync_index(&store, &report.touched)?;
        }
        let c = &report.counts;
        // A library back from offline has pending files the queue skipped.
        let came_online = was_offline && !report.offline;
        if c.added + c.changed + c.restored > 0 || came_online {
            self.inner.queue_new_work();
        }
        Ok(report)
    }

    /// Forgets a library: its file rows, analysis and user data go (see
    /// [`Store::remove_library`]), its files leave the text index, nothing
    /// on disk is touched. Returns `false` if there is no such library.
    ///
    /// # Errors
    /// [`Error::Sqlite`], [`Error::Index`].
    pub fn remove_library(&self, library_id: i64) -> Result<bool> {
        let removed = lock(&self.inner.store).remove_library(library_id)?;
        let Some(file_ids) = removed else {
            return Ok(false);
        };
        if self.inner.index_ready.load(Ordering::SeqCst) {
            self.inner.index.delete(&file_ids)?;
        }
        if let Some(w) = lock(&self.inner.watcher).as_ref() {
            w.refresh_libraries();
        }
        Ok(true)
    }
}

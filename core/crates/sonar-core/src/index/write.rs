//! Writes to the text index: full rebuild, upsert/delete by file id, and
//! refresh of a set of ids from the store.

use std::fs;
use std::io;
use std::sync::MutexGuard;
use std::time::{Duration, Instant};

use tantivy::{IndexWriter, TantivyDocument, Term};

use super::source::{self, IndexDoc};
use super::{index_version, TextIndex, VERSION_FILE};
use crate::{Error, Result, Store};

/// Writer heap per indexing thread (Tantivy's minimum is 15 MB).
const HEAP_PER_THREAD: usize = 40_000_000;

/// Indexing threads for a full rebuild (diminishing returns beyond this
/// for short documents).
const MAX_REBUILD_THREADS: usize = 4;

/// Result of [`TextIndex::rebuild`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildReport {
    /// Documents in the index after the build.
    pub docs: u64,
    /// Wall-clock time of the build, including reading the store.
    pub elapsed: Duration,
}

impl TextIndex {
    /// Replaces the whole index with the indexable files of `store`
    /// (atomic for readers: they see the old index until the commit). The
    /// version file is removed first and written after the commit, so an
    /// interrupted build is detected and redone on the next open.
    ///
    /// # Errors
    /// [`Error::Sqlite`], [`Error::Index`] (e.g. another process holds the
    /// writer lock), [`Error::Io`] (version file).
    pub fn rebuild(&self, store: &Store) -> Result<BuildReport> {
        let started = Instant::now();
        let _guard = self.lock();
        if let Some(dir) = &self.dir {
            let path = dir.join(VERSION_FILE);
            match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(Error::io(path, e)),
                _ => {}
            }
        }
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, MAX_REBUILD_THREADS);
        let w = self.writer(threads)?;
        w.delete_all_documents()?;
        source::for_each_doc(store, |doc| self.add(&w, &doc))?;
        self.finish(w)?;
        if let Some(dir) = &self.dir {
            let path = dir.join(VERSION_FILE);
            fs::write(&path, index_version()).map_err(|e| Error::io(path, e))?;
        }
        Ok(BuildReport {
            docs: self.num_docs(),
            elapsed: started.elapsed(),
        })
    }

    /// Adds or replaces the documents of `docs` (by file id).
    ///
    /// # Errors
    /// [`Error::Index`].
    pub fn upsert(&self, docs: &[IndexDoc]) -> Result<()> {
        let _guard = self.lock();
        let w = self.writer(1)?;
        for doc in docs {
            w.delete_term(self.id_term(doc.file_id));
            self.add(&w, doc)?;
        }
        self.finish(w)
    }

    /// Removes the documents of `file_ids` (unknown ids are ignored).
    ///
    /// # Errors
    /// [`Error::Index`].
    pub fn delete(&self, file_ids: &[i64]) -> Result<()> {
        let _guard = self.lock();
        let w = self.writer(1)?;
        for &id in file_ids {
            w.delete_term(self.id_term(id));
        }
        self.finish(w)
    }

    /// Re-reads `file_ids` from `store` and updates their documents: files
    /// that are gone or missing are removed, the others replaced. This is
    /// how scan/watch/analysis keep the index fresh (pass the ids a batch
    /// touched, after its transaction committed).
    ///
    /// # Errors
    /// [`Error::Sqlite`], [`Error::Index`].
    pub fn refresh_files(&self, store: &Store, file_ids: &[i64]) -> Result<()> {
        let _guard = self.lock();
        let w = self.writer(1)?;
        for &id in file_ids {
            w.delete_term(self.id_term(id));
            if let Some(doc) = source::doc(store, id)? {
                self.add(&w, &doc)?;
            }
        }
        self.finish(w)
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        // A panic while writing leaves nothing to repair here: Tantivy
        // rolls back uncommitted work when the writer drops.
        self.write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn writer(&self, threads: usize) -> Result<IndexWriter<TantivyDocument>> {
        Ok(self
            .index
            .writer_with_num_threads(threads, threads * HEAP_PER_THREAD)?)
    }

    fn id_term(&self, file_id: i64) -> Term {
        // Negative ids never exist; map them to a term that matches nothing.
        Term::from_field_u64(
            self.fields.file_id,
            u64::try_from(file_id).unwrap_or(u64::MAX),
        )
    }

    fn add(&self, w: &IndexWriter<TantivyDocument>, doc: &IndexDoc) -> Result<()> {
        if let Some(d) = doc.to_document(&self.fields) {
            w.add_document(d)?;
        }
        Ok(())
    }

    /// Commits, waits for merges (so segments do not pile up across many
    /// small writes) and makes the result visible to searches.
    fn finish(&self, mut w: IndexWriter<TantivyDocument>) -> Result<()> {
        w.commit()?;
        w.wait_merging_threads()?;
        self.reader.reload()?;
        Ok(())
    }
}

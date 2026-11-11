//! Writer operations: append, tombstone, compaction (ADR-023).
//!
//! After an error from any of these the in-memory state may be ahead of
//! the files: drop the store and open it again (the files themselves stay
//! consistent, see the module docs of [`super`]).

use std::collections::HashSet;
use std::time::Instant;

use serde::Serialize;

use super::file::RowFile;
use super::generation::{self, Generation};
use super::{VectorStore, TOMBSTONE};
use crate::dsp::aspects::{AspectVectors, LibraryNorm};
use crate::{Error, Result};

/// Rows copied per write during compaction.
const COPY_ROWS: usize = 8192;

/// Result of [`VectorStore::compact`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CompactReport {
    /// Rows before.
    pub rows_before: u64,
    /// Rows after (= live rows).
    pub rows_after: u64,
    /// Wall time.
    pub seconds: f64,
}

impl VectorStore {
    /// Appends one row per `(file_id, raw vectors)`; a file id that already
    /// has a row (or appears twice) keeps only its last row. The stats are
    /// merged with the batch's. Nothing is written if any id is not
    /// positive or any vector has the wrong dimension.
    ///
    /// # Errors
    /// [`Error::VectorStore`] (read-only view, bad id), [`Error::Dsp`]
    /// (dimension), [`Error::Io`].
    pub fn append(&mut self, batch: &[(i64, &AspectVectors)]) -> Result<()> {
        self.writable()?;
        let mut batch_norm = LibraryNorm::new();
        for (id, v) in batch {
            if *id <= TOMBSTONE {
                return Err(self.error(format!("file id {id} cannot be stored")));
            }
            batch_norm.push(v)?;
        }
        let start = self.gen.rows;
        let n = batch.len() as u64;
        if n == 0 {
            return Ok(());
        }
        if start + n > u64::from(u32::MAX) {
            return Err(self.error("row limit reached".into()));
        }
        let g = &mut self.gen;
        let ids: Vec<u8> = batch.iter().flat_map(|(id, _)| id.to_le_bytes()).collect();
        g.ids.write_rows(start, &ids)?;
        for (k, f) in g.aspects.iter().enumerate() {
            let bytes: Vec<u8> = batch
                .iter()
                .flat_map(|(_, v)| v.parts()[k].1.iter().flat_map(|x| x.to_le_bytes()))
                .collect();
            f.write_rows(start, &bytes)?;
        }
        // Commit point: the header counts (the smallest one wins on open).
        for f in g.files() {
            f.set_rows(start + n)?;
        }
        g.rows = start + n;
        for (i, (id, _)) in batch.iter().enumerate() {
            if let Some(old) = g.by_file.insert(*id, (start + i as u64) as u32) {
                tombstone(&g.ids, u64::from(old))?;
            }
        }
        g.norm.merge(&batch_norm)?;
        g.save_norm()?;
        for f in g.files_mut() {
            f.remap()?;
        }
        Ok(())
    }

    /// Tombstones the rows of `file_ids`; returns how many had a row.
    ///
    /// # Errors
    /// [`Error::VectorStore`] (read-only view), [`Error::Io`].
    pub fn remove(&mut self, file_ids: impl IntoIterator<Item = i64>) -> Result<u64> {
        self.writable()?;
        let mut removed = 0;
        for id in file_ids {
            if let Some(row) = self.gen.by_file.remove(&id) {
                tombstone(&self.gen.ids, u64::from(row))?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Tombstones every live row whose file id is not in `keep`; returns
    /// how many.
    ///
    /// # Errors
    /// See [`Self::remove`].
    pub fn retain(&mut self, keep: &HashSet<i64>) -> Result<u64> {
        let drop: Vec<i64> = self.file_ids().filter(|id| !keep.contains(id)).collect();
        self.remove(drop)
    }

    /// Rewrites the live rows (in row order) into a new generation with
    /// exactly recomputed stats, then swaps it in atomically.
    ///
    /// # Errors
    /// [`Error::VectorStore`] (read-only view), [`Error::Io`].
    pub fn compact(&mut self) -> Result<CompactReport> {
        self.writable()?;
        let t = Instant::now();
        let rows_before = self.gen.rows;
        let mut live: Vec<u32> = self.gen.by_file.values().copied().collect();
        live.sort_unstable();
        let next = self.generation + 1;
        let path = generation::create(&self.dir, next)?;
        {
            let empty = generation::open(&path, true)?;
            copy_rows(&self.gen, &empty, &live)?;
            for f in empty.files() {
                f.set_rows(live.len() as u64)?;
                f.sync()?;
            }
        }
        // Re-open: maps the rows, rebuilds the id map and extends the empty
        // stats over every row, i.e. recomputes them from the live rows.
        let gen = generation::open(&path, true)?;
        generation::sync_dir(&path)?;
        generation::write_current(&self.dir, next)?;
        self.gen = gen;
        self.generation = next;
        generation::remove_stale(&self.dir, Some(next))?;
        Ok(CompactReport {
            rows_before,
            rows_after: self.gen.rows,
            seconds: t.elapsed().as_secs_f64(),
        })
    }

    fn error(&self, reason: String) -> Error {
        Error::VectorStore {
            path: self.dir.clone(),
            reason,
        }
    }
}

fn tombstone(ids: &RowFile, row: u64) -> Result<()> {
    ids.write_rows(row, &TOMBSTONE.to_le_bytes())
}

/// Copies rows `live` of every file of `from` to the start of `to`.
fn copy_rows(from: &Generation, to: &Generation, live: &[u32]) -> Result<()> {
    for (src, dst) in from.files().zip(to.files()) {
        let rb = src.layout().row_bytes() as usize;
        let data = src.data(from.rows);
        let mut buf = Vec::with_capacity(COPY_ROWS.min(live.len()) * rb);
        for (i, chunk) in live.chunks(COPY_ROWS).enumerate() {
            buf.clear();
            for &r in chunk {
                let r = r as usize;
                let row = data
                    .get(r * rb..(r + 1) * rb)
                    .ok_or_else(|| Error::Internal {
                        reason: "vector row out of range".into(),
                    })?;
                buf.extend_from_slice(row);
            }
            dst.write_rows((i * COPY_ROWS) as u64, &buf)?;
        }
    }
    Ok(())
}

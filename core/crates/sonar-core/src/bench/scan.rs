//! `sonar bench scan`: first scan, no-op rescan and incremental rescan of a
//! generated library (budget: > 2,000 files/s initial metadata scan).

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use super::corpus::{self, Mutations};
use super::{machine, rate, Machine};
use crate::scan::{scan_library, ScanReport};
use crate::{Result, Store};

/// One timed scan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScanPhase {
    /// Wall-clock seconds.
    pub secs: f64,
    /// Files present on disk that the scan looked at (all outcomes but
    /// `removed`) per second.
    pub files_per_sec: f64,
    /// New rows.
    pub added: usize,
    /// Rows whose content changed.
    pub changed: usize,
    /// Rows that followed a rename.
    pub moved: usize,
    /// Rows newly missing.
    pub removed: usize,
    /// Rows restored.
    pub restored: usize,
    /// Rows left as they were.
    pub unchanged: usize,
    /// Unreadable files.
    pub errors: usize,
}

impl ScanPhase {
    fn new(r: &ScanReport, secs: f64) -> Self {
        let c = r.counts;
        let seen = c.added + c.changed + c.moved + c.restored + c.unchanged + c.errors;
        Self {
            secs,
            files_per_sec: rate(seen as u64, secs),
            added: c.added,
            changed: c.changed,
            moved: c.moved,
            removed: c.removed,
            restored: c.restored,
            unchanged: c.unchanged,
            errors: c.errors,
        }
    }
}

/// Result of [`bench_scan`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScanBench {
    /// Where it ran.
    pub machine: Machine,
    /// Audio files generated.
    pub files: usize,
    /// Folders generated.
    pub dirs: usize,
    /// Bytes of audio generated.
    pub bytes: u64,
    /// Seconds spent generating (not part of any timing below).
    pub generate_secs: f64,
    /// First scan: every file is new. The page cache is warm from
    /// generation, so this measures the scanner, not the disk. The first
    /// scan reads up to 128 KiB per file for `quick_hash`, so on a cold
    /// library on a slow disk it is slower than this number.
    pub first: ScanPhase,
    /// Rescan with nothing changed.
    pub noop: ScanPhase,
    /// What was edited before the incremental rescan.
    pub edits: EditCounts,
    /// Rescan after those edits.
    pub incremental: ScanPhase,
    /// Database size after the last scan (bytes, without WAL).
    pub db_bytes: u64,
}

/// Serializable copy of [`Mutations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EditCounts {
    /// Files with changed content.
    pub changed: usize,
    /// Files renamed.
    pub renamed: usize,
    /// Files deleted.
    pub deleted: usize,
    /// Files added.
    pub added: usize,
}

impl From<Mutations> for EditCounts {
    fn from(m: Mutations) -> Self {
        Self {
            changed: m.changed,
            renamed: m.renamed,
            deleted: m.deleted,
            added: m.added,
        }
    }
}

/// Generates `files` files under `work/library`, then scans them into a
/// fresh store `work/bench.db`: first scan, no-op rescan, and a rescan after
/// [`corpus::mutate`].
///
/// # Errors
/// [`crate::Error::Io`] and [`crate::Error::Sqlite`].
pub fn bench_scan(work: &Path, files: usize) -> Result<ScanBench> {
    let root = work.join("library");
    let t = Instant::now();
    let corpus = corpus::generate_tree(&root, files)?;
    let generate_secs = t.elapsed().as_secs_f64();

    let db = work.join("bench.db");
    let mut store = Store::open(&db)?;
    let lib = store.add_library(&root)?;
    let run = |store: &mut Store| -> Result<ScanPhase> {
        let t = Instant::now();
        let report = scan_library(store, lib.id)?;
        Ok(ScanPhase::new(&report, t.elapsed().as_secs_f64()))
    };
    let first = run(&mut store)?;
    let noop = run(&mut store)?;
    let edits = corpus::mutate(&root, files)?;
    let incremental = run(&mut store)?;
    let db_bytes = std::fs::metadata(&db).map_or(0, |m| m.len());
    Ok(ScanBench {
        machine: machine(),
        files,
        dirs: corpus.dirs,
        bytes: corpus.bytes,
        generate_secs,
        first,
        noop,
        edits: edits.into(),
        incremental,
        db_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_bench_sees_every_edit() {
        let tmp = tempfile::tempdir().expect("tmp");
        let b = bench_scan(tmp.path(), 1_000).expect("bench");
        assert_eq!(b.first.added, 1_000);
        assert_eq!(b.noop.unchanged, 1_000);
        assert_eq!((b.noop.added, b.noop.changed, b.noop.moved), (0, 0, 0));
        let e = b.edits;
        assert_eq!(b.incremental.changed, e.changed);
        assert_eq!(b.incremental.moved, e.renamed);
        assert_eq!(b.incremental.removed, e.deleted);
        assert_eq!(b.incremental.added, e.added);
        assert_eq!(b.incremental.errors, 0);
        assert!(b.first.files_per_sec > 0.0 && b.db_bytes > 0);
    }
}

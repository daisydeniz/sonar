//! Library scanner: the "discover → identify" stage of the indexing
//! pipeline (docs/ARCHITECTURE.md §4). One call scans one library and
//! brings its `files` rows in line with the disk.
//!
//! # Walk
//! Recursive from the library root. **Symlinks are never followed** (files
//! or directories): it rules out cycles and double-indexing of the same
//! sample through two paths, and a library that should include another
//! folder can add it as its own library. Names starting with `.` are
//! skipped (dot-files, dot-dirs, macOS AppleDouble `._*` resource forks).
//! Only [`AUDIO_EXTENSIONS`] are indexed (ASCII case-insensitive).
//! `rel_path` is relative to the root with `/` separators; names that are
//! not UTF-8 are reported as issues and skipped. Names are stored as the
//! filesystem returns them (no Unicode normalization).
//!
//! # Identity
//! A row at the same path with the same size and mtime (ns) is unchanged
//! and the file is not opened. Otherwise the file gets a [`quick_hash`]
//! (XXH3-64 of size + first/last 64 KiB), computed in parallel with rayon.
//!
//! # Reconcile (one transaction per scan)
//! - **changed** — same path, different size or mtime: new identity, status
//!   `new`, `analysis_version` 0, `error`/`content_hash` cleared,
//!   `audio_meta`/`analysis`/`file_stages` rows deleted (stages re-run);
//!   user data kept.
//! - **moved** — a row whose path vanished and a new path with the same
//!   `(size, quick_hash)`: the row's `rel_path`/`mtime` are updated so its
//!   id and user data follow the file. Several candidates pair one-to-one:
//!   present rows before `missing` ones, then by path; new paths by path.
//!   `missing` rows from earlier scans can be revived this way too.
//! - **removed** — vanished, unmatched rows become `missing` (kept, with
//!   user data and stage rows, so a returning file loses nothing).
//! - **restored** — a `missing` row whose path reappears with the same size
//!   and mtime: status re-derived from its surviving stage rows
//!   (`analysis` or a finished `dsp` stage → `analyzed`, `audio_meta` →
//!   `meta`, else `new`); `error` stage rows are dropped so they are retried.
//!   Moves keep stage rows (same content). If the content differs it counts
//!   as **changed** instead.
//! - **added** — remaining new paths: new rows, status `new`.
//! - **errors** — a file that can be listed but not read (e.g. mode 000):
//!   row status `error`, `error` text, `quick_hash` NULL; retried on every
//!   scan; never used for move matching. Recorded as a [`ScanIssue`].
//! - An unlistable directory (or an entry that cannot be stat'ed) is an
//!   issue, and rows under it are left untouched: we could not look.
//! - Root missing or unlistable (e.g. unmounted drive): the report is
//!   flagged `offline`, `libraries.online` set to 0, no file row changes.
//!   A successful scan sets `online` back to 1.
//! - Stale mount point: a root that exists but is a completely **empty**
//!   directory while the library has present (non-`missing`) rows is treated
//!   as offline too (an unmounted volume often leaves its empty mount-point
//!   directory behind). Without this guard every row would be marked
//!   `missing`. A library the user really emptied therefore stays
//!   "offline" until at least one entry appears; no rows are lost.
//!
//! Same-path content swaps (`a`↔`b` renamed into each other) are seen as
//! two **changed** files: user data stays with the path.
//!
//! The walk and hash phases run before the write transaction, so the
//! write lock is held only while applying. A concurrent writer to the same
//! library's rows is not expected (the job pipeline, T1.5, serializes
//! scans per library).

mod apply;
mod identity;
mod reconcile;
mod walk;

#[cfg(test)]
mod tests;

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{Error, Result, Store};

pub use identity::{
    is_audio_extension, is_audio_name, quick_hash, quick_hash_with, AUDIO_EXTENSIONS,
    QUICK_HASH_CHUNK,
};
pub use reconcile::ScanCounts;

/// Options for [`scan_library_with`].
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Set to `true` from another thread to stop the scan; it then returns
    /// [`Error::Cancelled`] without writing anything.
    pub cancel: Option<Arc<AtomicBool>>,
}

/// What kind of problem a [`ScanIssue`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IssueKind {
    /// A directory could not be listed; rows under it were left alone.
    UnreadableDir,
    /// A file could not be stat'ed or read.
    UnreadableFile,
    /// A file or directory name is not valid UTF-8; skipped.
    NonUtf8Name,
}

impl fmt::Display for IssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnreadableDir => "unreadable directory",
            Self::UnreadableFile => "unreadable file",
            Self::NonUtf8Name => "non-UTF-8 name",
        })
    }
}

/// A non-fatal problem met during a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanIssue {
    /// Absolute path concerned.
    pub path: PathBuf,
    /// Problem category.
    pub kind: IssueKind,
    /// OS error or explanation.
    pub message: String,
}

impl fmt::Display for ScanIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.kind,
            self.path.display(),
            self.message
        )
    }
}

/// Result of scanning one library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    /// Library scanned.
    pub library_id: i64,
    /// Its root directory.
    pub root: PathBuf,
    /// The root was missing or unreadable; nothing but `online` was written.
    pub offline: bool,
    /// Per-outcome file counts (all zero when offline).
    pub counts: ScanCounts,
    /// Non-fatal problems, in discovery order.
    pub issues: Vec<ScanIssue>,
    /// Ids of every row the scan added, changed, moved, restored or marked
    /// missing: what the text index must refresh (empty when offline).
    pub touched: Vec<i64>,
    /// Wall-clock time of the scan.
    pub duration: Duration,
}

/// Scans library `library_id` (see the module docs for the rules).
///
/// # Errors
/// [`Error::LibraryNotFound`], [`Error::Sqlite`]. Filesystem problems are
/// not errors: they become issues, error rows, or an `offline` report.
pub fn scan_library(store: &mut Store, library_id: i64) -> Result<ScanReport> {
    scan_library_with(store, library_id, &ScanOptions::default())
}

/// [`scan_library`] with options (cancellation).
///
/// # Errors
/// As [`scan_library`], plus [`Error::Cancelled`].
pub fn scan_library_with(
    store: &mut Store,
    library_id: i64,
    options: &ScanOptions,
) -> Result<ScanReport> {
    let start = Instant::now();
    let library = store
        .library(library_id)?
        .ok_or(Error::LibraryNotFound { id: library_id })?;
    let root = library.root_path;
    let never = AtomicBool::new(false);
    let cancel = options.cancel.as_deref().unwrap_or(&never);

    let mut report = ScanReport {
        library_id,
        root: root.clone(),
        offline: false,
        counts: ScanCounts::default(),
        issues: Vec::new(),
        touched: Vec::new(),
        duration: Duration::ZERO,
    };
    let walked = match walk::walk(&root, cancel) {
        Ok(w) => w,
        Err(walk::WalkError::Cancelled) => return Err(Error::Cancelled),
        Err(walk::WalkError::Offline(e)) => {
            let why = format!("library root unavailable: {e}");
            return offline_report(store, report, why, start);
        }
    };
    let rows = apply::load_files(store.conn(), library_id)?;
    if walked.files.is_empty()
        && rows.iter().any(|r| r.status != reconcile::Status::Missing)
        && is_empty_dir(&root)
    {
        let why = "library root is an empty directory (stale mount point?)".to_owned();
        return offline_report(store, report, why, start);
    }
    report.issues = walked.issues;

    let plan = reconcile::plan(
        rows,
        walked.files,
        &walked.unknown,
        &root,
        cancel,
        |rel, size, buf| quick_hash_with(&root.join(rel), size, buf),
    )
    .map_err(|reconcile::Cancelled| Error::Cancelled)?;
    report.touched = apply::apply(store.conn_mut(), library_id, &plan.ops)?;

    report.counts = plan.counts;
    report.issues.extend(plan.issues);
    report.duration = start.elapsed();
    Ok(report)
}

/// True if `dir` can be listed and has no entries at all.
fn is_empty_dir(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_none())
}

/// Marks the library offline (no row changes) and finishes the report.
fn offline_report(
    store: &mut Store,
    mut report: ScanReport,
    message: String,
    start: Instant,
) -> Result<ScanReport> {
    store.set_library_online(report.library_id, false)?;
    report.offline = true;
    report.issues.push(ScanIssue {
        path: report.root.clone(),
        kind: IssueKind::UnreadableDir,
        message,
    });
    report.duration = start.elapsed();
    Ok(report)
}

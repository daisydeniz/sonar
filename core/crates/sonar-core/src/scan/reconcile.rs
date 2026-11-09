//! Reconcile: compare the store's rows for one library with a fresh walk
//! and produce the list of row operations ([`Op`]) plus report counts.
//!
//! Pure apart from the injected hash function, so the matching rules are
//! unit-tested without a filesystem (see `reconcile_tests.rs`). Only files
//! that are new or whose size/mtime changed are hashed, in parallel (rayon).

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};

use super::identity::{hash_to_db, size_to_db};
use super::walk::{DiskFile, Unknown};
use super::{IssueKind, ScanIssue};

/// `files.status` values (schema CHECK constraint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    New,
    Meta,
    Analyzed,
    Error,
    Missing,
}

impl FromSql for Status {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "new" => Ok(Self::New),
            "meta" => Ok(Self::Meta),
            "analyzed" => Ok(Self::Analyzed),
            "error" => Ok(Self::Error),
            "missing" => Ok(Self::Missing),
            other => Err(FromSqlError::Other(
                format!("unknown file status '{other}'").into(),
            )),
        }
    }
}

/// The scan-relevant columns of a `files` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DbFile {
    pub id: i64,
    pub rel_path: String,
    pub size: i64,
    pub mtime: i64,
    pub quick_hash: Option<i64>,
    pub status: Status,
}

impl DbFile {
    /// Row recorded by a scan that could not read the file: retried every
    /// scan. (An `error` row *with* a hash is an analysis failure and
    /// follows the normal unchanged/changed rules.)
    fn is_scan_error(&self) -> bool {
        self.status == Status::Error && self.quick_hash.is_none()
    }
}

/// Outcome of hashing a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Identity {
    /// `quick_hash`, bit-cast for the store.
    Hash(i64),
    /// The file could not be read; message for `files.error`.
    Error(String),
}

/// One row operation; applied in order inside one transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    /// New row (status `new`, or `error` if unreadable).
    Insert {
        rel_path: String,
        size: i64,
        mtime: i64,
        identity: Identity,
    },
    /// Content at a known path changed (or a scan error is retried): new
    /// identity, status reset, `audio_meta`/`analysis` dropped, user data kept.
    Update {
        id: i64,
        size: i64,
        mtime: i64,
        identity: Identity,
    },
    /// A `missing` row's file is back unchanged: status re-derived.
    Restore { id: i64 },
    /// Same content at a new path: row keeps its id and all its data.
    Move {
        id: i64,
        rel_path: String,
        mtime: i64,
        /// The row was `missing`; re-derive its status too.
        restore: bool,
    },
    /// Gone from disk: status `missing`, row and user data kept.
    Missing { id: i64 },
}

/// Per-outcome file counts of one scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanCounts {
    /// New rows for new files.
    pub added: usize,
    /// Known paths whose content changed (or that became readable again).
    pub changed: usize,
    /// Rows that followed their file to a new path.
    pub moved: usize,
    /// Rows newly marked `missing`.
    pub removed: usize,
    /// `missing` rows whose file reappeared unchanged at the same path.
    pub restored: usize,
    /// Present, unchanged files (no read needed).
    pub unchanged: usize,
    /// Files that could not be read (row status `error`).
    pub errors: usize,
}

/// Everything a scan will write, plus counts and per-file issues.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub ops: Vec<Op>,
    pub counts: ScanCounts,
    pub issues: Vec<ScanIssue>,
}

/// A file that must be hashed, and what it is.
struct Job {
    disk: DiskFile,
    /// Existing row at the same path, if any.
    row: Option<DbFile>,
}

/// The cancel flag was set while hashing.
#[derive(Debug)]
pub(crate) struct Cancelled;

/// Builds the plan. `hash(rel_path, size, buf)` computes a quick hash,
/// reading through `buf` (one per rayon work split, reused across files);
/// `root` is only used to report absolute paths in issues.
pub(crate) fn plan<H>(
    db: Vec<DbFile>,
    disk: Vec<DiskFile>,
    unknown: &Unknown,
    root: &Path,
    cancel: &AtomicBool,
    hash: H,
) -> Result<Plan, Cancelled>
where
    H: Fn(&str, u64, &mut Vec<u8>) -> io::Result<u64> + Sync,
{
    let mut plan = Plan::default();
    let mut by_path: HashMap<String, DbFile> =
        db.into_iter().map(|r| (r.rel_path.clone(), r)).collect();

    // 1. Files on disk: unchanged (fast path, no read), restore, or hash.
    let mut jobs = Vec::new();
    for d in disk {
        let Some(row) = by_path.remove(&d.rel_path) else {
            jobs.push(Job { disk: d, row: None });
            continue;
        };
        let same = row.size == size_to_db(d.size) && row.mtime == d.mtime_ns;
        match row.status {
            Status::Missing if same && row.quick_hash.is_some() => {
                plan.ops.push(Op::Restore { id: row.id });
                plan.counts.restored += 1;
            }
            Status::Missing => jobs.push(Job {
                disk: d,
                row: Some(row),
            }),
            _ if row.is_scan_error() || !same => jobs.push(Job {
                disk: d,
                row: Some(row),
            }),
            _ => plan.counts.unchanged += 1,
        }
    }

    // 2. Rows not seen on disk (and not under an unlistable directory) are
    //    move sources; `missing` rows from earlier scans can be too.
    let mut sources: HashMap<(i64, i64), Vec<DbFile>> = HashMap::new();
    let mut gone = Vec::new();
    for row in by_path.into_values() {
        if unknown.contains(&row.rel_path) {
            continue;
        }
        match (row.status, row.quick_hash) {
            (Status::Missing, None) => {}
            (_, Some(h)) => sources.entry((row.size, h)).or_default().push(row),
            (_, None) => gone.push(row.id),
        }
    }

    // 3. Hash new and changed files in parallel.
    let hashed: Vec<Option<io::Result<u64>>> = jobs
        .par_iter()
        .map_init(Vec::new, |buf, j| {
            (!cancel.load(Ordering::Relaxed)).then(|| hash(&j.disk.rel_path, j.disk.size, buf))
        })
        .collect();
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled);
    }

    // 4. Changed rows get their new identity; new files become move targets.
    let mut targets: BTreeMap<(i64, i64), Vec<DiskFile>> = BTreeMap::new();
    for (job, result) in jobs.into_iter().zip(hashed) {
        let identity = match result {
            Some(Ok(h)) => Identity::Hash(hash_to_db(h)),
            Some(Err(e)) => {
                plan.issues.push(ScanIssue {
                    path: root.join(&job.disk.rel_path),
                    kind: IssueKind::UnreadableFile,
                    message: e.to_string(),
                });
                Identity::Error(e.to_string())
            }
            None => return Err(Cancelled),
        };
        let (size, mtime) = (size_to_db(job.disk.size), job.disk.mtime_ns);
        let failed = matches!(identity, Identity::Error(_));
        if failed {
            plan.counts.errors += 1;
        }
        match (job.row, identity) {
            (Some(row), identity) => {
                if !failed {
                    plan.counts.changed += 1;
                }
                plan.ops.push(Op::Update {
                    id: row.id,
                    size,
                    mtime,
                    identity,
                });
            }
            (None, Identity::Hash(h)) => targets.entry((size, h)).or_default().push(job.disk),
            // Unreadable files are never move targets.
            (None, identity) => plan.ops.push(Op::Insert {
                rel_path: job.disk.rel_path,
                size,
                mtime,
                identity,
            }),
        }
    }

    // 5. Pair sources and targets with equal (size, hash) one-to-one, in a
    //    deterministic order: present rows before `missing` ones, then by
    //    path; targets by path (the walk sorted them).
    for (key, files) in targets {
        let mut srcs = sources.remove(&key).unwrap_or_default();
        srcs.sort_by(|a, b| {
            let rank = |r: &DbFile| r.status == Status::Missing;
            (rank(a), &a.rel_path).cmp(&(rank(b), &b.rel_path))
        });
        let mut srcs = srcs.into_iter();
        for f in files {
            match srcs.next() {
                Some(src) => {
                    plan.counts.moved += 1;
                    plan.ops.push(Op::Move {
                        id: src.id,
                        rel_path: f.rel_path,
                        mtime: f.mtime_ns,
                        restore: src.status == Status::Missing,
                    });
                }
                None => {
                    plan.counts.added += 1;
                    plan.ops.push(Op::Insert {
                        rel_path: f.rel_path,
                        size: key.0,
                        mtime: f.mtime_ns,
                        identity: Identity::Hash(key.1),
                    });
                }
            }
        }
        gone.extend(srcs.filter(|r| r.status != Status::Missing).map(|r| r.id));
    }

    // 6. Unmatched sources that were present are now missing.
    gone.extend(
        sources
            .into_values()
            .flatten()
            .filter(|r| r.status != Status::Missing)
            .map(|r| r.id),
    );
    gone.sort_unstable();
    plan.counts.removed = gone.len();
    plan.ops
        .extend(gone.into_iter().map(|id| Op::Missing { id }));
    Ok(plan)
}

#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod tests;

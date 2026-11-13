//! Duplicate finder (T6.9a, ADR-036): groups of files that are the same
//! bytes (**exact**) or the same sound in another file (**near**).
//!
//! - **Which files.** Every file of an online library whose status is not
//!   `missing`. Hidden files are included (the UI shows their flag via
//!   `FileRow::hidden`), so hiding one copy does not make the other look
//!   unique.
//! - **Exact** ([`exact`]): same size and quick hash, confirmed by a full
//!   XXH3-128 content hash cached in `files.content_hash`.
//! - **Near** ([`near`]): aspect vectors compared within a duration
//!   window, linked when every aspect's distance is at most
//!   [`DupesOptions::threshold`] ([`DEFAULT_NEAR_THRESHOLD`], calibrated on
//!   the fixtures: ADR-036), grouped by connected components.
//! - **Exact before near.** Byte-identical files have identical vectors,
//!   so in the near pass each exact group is represented by its lowest file
//!   id that has vectors; its other members appear only in the exact group.
//!   A near group therefore never holds two copies of the same bytes, and a
//!   file is in at most one exact and at most one near group.
//! - **Order.** Groups by wasted bytes (all but the largest member)
//!   descending, then exact before near, then lowest file id; members by
//!   file id. Deterministic for the same store and vectors.
//! - **Cancel.** [`DupesOptions::cancel`] is checked per hashed buffer and
//!   per chunk of the near scan ([`Error::Cancelled`]).

mod distance;
mod exact;
mod near;

#[cfg(test)]
mod calibrate_tests;
#[cfg(test)]
mod test_lib;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

pub use distance::AMPLITUDE_SKIP;
pub use exact::CONTENT_HASH_LEN;
pub use near::{ALL_PAIRS_MAX, WINDOW_MS, WINDOW_PCT};

use crate::index::vectors::VectorStore;
use crate::{Error, Result, Store};

/// Default near-duplicate threshold: largest per-aspect RMS z-difference
/// (ADR-036: same-sound fixture pairs ≤ 0.124, nearest different sounds
/// within the duration window ≥ 0.194).
pub const DEFAULT_NEAR_THRESHOLD: f32 = 0.15;
/// Largest threshold accepted (beyond it groups stop meaning "the same
/// sound" and the prefilter stops pruning).
pub const MAX_NEAR_THRESHOLD: f32 = 0.5;

/// What [`find_duplicates`] looks for.
#[derive(Debug, Clone)]
pub struct DupesOptions {
    /// Find byte-identical files.
    pub exact: bool,
    /// Find the same sound in other files.
    pub near: bool,
    /// Near threshold, `0..=`[`MAX_NEAR_THRESHOLD`].
    pub threshold: f32,
    /// Set to stop early ([`Error::Cancelled`]).
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for DupesOptions {
    fn default() -> Self {
        Self {
            exact: true,
            near: true,
            threshold: DEFAULT_NEAR_THRESHOLD,
            cancel: None,
        }
    }
}

/// Kind of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DupeKind {
    /// Same bytes.
    Exact,
    /// Same sound, other bytes.
    Near,
}

/// One group of duplicates.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DupeGroup {
    /// Exact or near.
    pub kind: DupeKind,
    /// Members (≥ 2), ascending.
    pub file_ids: Vec<i64>,
    /// File size of each member (bytes), same order.
    pub sizes: Vec<u64>,
    /// Bytes freed by keeping only the largest member.
    pub wasted_bytes: u64,
    /// Largest pair distance in the group (0 for exact groups).
    pub max_distance: f32,
    /// Mean pair distance (0 for exact groups).
    pub mean_distance: f32,
}

/// Phase timings of one run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct DupesTimings {
    /// Reading the file list.
    pub load_ms: f64,
    /// Exact pass (hashing included).
    pub exact_ms: f64,
    /// Near pass.
    pub near_ms: f64,
}

/// Result of [`find_duplicates`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DupesReport {
    /// Groups, see the module docs for the order.
    pub groups: Vec<DupeGroup>,
    /// Files considered (online, not missing).
    pub files: usize,
    /// Files sharing size and quick hash with another file.
    pub exact_candidates: usize,
    /// Content hashes computed now (the others were cached).
    pub hashed: usize,
    /// Candidates not hashed: unreadable (offline, permissions) or changed
    /// since the last scan.
    pub skipped: usize,
    /// Files compared in the near pass.
    pub near_files: usize,
    /// Pairs within the duration window.
    pub near_pairs: u64,
    /// Near threshold used.
    pub threshold: f32,
    /// Where the time went.
    pub timings: DupesTimings,
}

/// One file as the passes see it.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    id: i64,
    size: u64,
    mtime: i64,
    quick_hash: Option<i64>,
    content_hash: Option<[u8; CONTENT_HASH_LEN]>,
    duration_ms: Option<i64>,
}

/// Finds duplicate groups (see the module docs). `vs = None` (nothing
/// analyzed yet) finds exact groups only. Hashing reads files and caches
/// hashes through `store`; call it off the UI thread.
///
/// # Errors
/// [`Error::InvalidQuery`] for a bad threshold, [`Error::Cancelled`],
/// [`Error::Sqlite`]; unreadable files are skipped, not errors.
pub fn find_duplicates(
    store: &Store,
    vs: Option<&VectorStore>,
    opts: &DupesOptions,
) -> Result<DupesReport> {
    let t = opts.threshold;
    if !(t.is_finite() && (0.0..=MAX_NEAR_THRESHOLD).contains(&t)) {
        return Err(Error::InvalidQuery {
            reason: format!("near threshold {t} is not within 0..={MAX_NEAR_THRESHOLD}"),
        });
    }
    let cancel = opts.cancel.as_deref();
    let clock = Instant::now();
    let entries = load(store)?;
    let mut timings = DupesTimings {
        load_ms: ms(clock),
        ..DupesTimings::default()
    };
    let clock = Instant::now();
    let exact = if opts.exact {
        exact::find(store, &entries, cancel)?
    } else {
        exact::Exact::default()
    };
    timings.exact_ms = ms(clock);
    let clock = Instant::now();
    let near = match (opts.near, vs) {
        (true, Some(vs)) => {
            let mut skip = vec![false; entries.len()];
            for g in &exact.groups {
                let keep = g.iter().position(|&i| vs.row_of(entries[i].id).is_some());
                for (n, &i) in g.iter().enumerate() {
                    skip[i] = Some(n) != keep;
                }
            }
            near::find(vs, &entries, &skip, t, cancel)?
        }
        _ => near::Near::default(),
    };
    timings.near_ms = ms(clock);
    if is_cancelled(cancel) {
        return Err(Error::Cancelled);
    }
    let mut groups: Vec<DupeGroup> = exact
        .groups
        .iter()
        .map(|g| group(DupeKind::Exact, &entries, g, 0.0, 0.0))
        .chain(near.groups.iter().map(|g| {
            group(
                DupeKind::Near,
                &entries,
                &g.members,
                g.max_distance,
                g.mean_distance,
            )
        }))
        .collect();
    groups.sort_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then(a.kind.cmp(&b.kind))
            .then(a.file_ids.cmp(&b.file_ids))
    });
    Ok(DupesReport {
        groups,
        files: entries.len(),
        exact_candidates: exact.candidates,
        hashed: exact.hashed,
        skipped: exact.skipped,
        near_files: near.files,
        near_pairs: near.pairs,
        threshold: t,
        timings,
    })
}

fn group(kind: DupeKind, entries: &[Entry], members: &[usize], max: f32, mean: f32) -> DupeGroup {
    let sizes: Vec<u64> = members.iter().map(|&i| entries[i].size).collect();
    let largest = sizes.iter().copied().max().unwrap_or(0);
    DupeGroup {
        kind,
        file_ids: members.iter().map(|&i| entries[i].id).collect(),
        wasted_bytes: sizes.iter().sum::<u64>().saturating_sub(largest),
        sizes,
        max_distance: max,
        mean_distance: mean,
    }
}

/// Every file of an online library that is not missing, by file id.
fn load(store: &Store) -> Result<Vec<Entry>> {
    let mut stmt = store.conn().prepare(
        "SELECT f.id, f.size, f.mtime, f.quick_hash, f.content_hash, m.duration_ms \
         FROM files f JOIN libraries l ON l.id = f.library_id \
         LEFT JOIN audio_meta m ON m.file_id = f.id \
         WHERE l.online = 1 AND f.status <> 'missing' ORDER BY f.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let hash: Option<Vec<u8>> = r.get(4)?;
        Ok(Entry {
            id: r.get(0)?,
            size: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
            mtime: r.get(2)?,
            quick_hash: r.get(3)?,
            content_hash: hash.and_then(|h| h.try_into().ok()),
            duration_ms: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn is_cancelled(flag: Option<&AtomicBool>) -> bool {
    flag.is_some_and(|f| f.load(Ordering::Relaxed))
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1e3
}

//! FFI records of the duplicate finder (ADR-036, T6.9a) and their
//! conversions. No product logic: grouping, threshold and order live in
//! `sonar_core::dupes`.

use sonar_core::dupes::{DupeGroup, DupeKind, DupesOptions, DupesReport, DEFAULT_NEAR_THRESHOLD};

use crate::convert::count;

/// What a duplicate group shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DuplicateKind {
    /// Byte-identical files.
    Exact,
    /// The same sound in other files (other format, rate, bit depth,
    /// channels or gain).
    Near,
}

/// What `duplicates` looks for. `DuplicatesQuery()` = everything with the
/// default threshold.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct DuplicatesQuery {
    /// Find byte-identical files.
    #[uniffi(default = true)]
    pub exact: bool,
    /// Find the same sound in other files (needs analysis).
    #[uniffi(default = true)]
    pub near: bool,
    /// Near threshold, 0...0.5 (`nil` = 0.15, calibrated); larger groups
    /// looser matches. Out of range: `FfiError.InvalidInput`.
    #[uniffi(default = None)]
    pub threshold: Option<f32>,
}

/// One group of duplicates. Show the files with `rows(fileIds:)`
/// (`FileRow.hidden` tells which are hidden: hidden files are included).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DuplicateGroup {
    /// Exact or near.
    pub kind: DuplicateKind,
    /// Members (>= 2), ascending id. A file is in at most one exact and at
    /// most one near group; a near group holds one file per exact group.
    pub file_ids: Vec<i64>,
    /// File size in bytes per member, same order as `file_ids`.
    pub sizes: Vec<u64>,
    /// Bytes freed by keeping only the largest member.
    pub wasted_bytes: u64,
    /// Largest distance between two members (0 for exact groups; at most
    /// about the threshold for near groups).
    pub max_distance: f32,
    /// Mean distance between members (0 for exact groups).
    pub mean_distance: f32,
}

/// Result of `duplicates`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DuplicatesResult {
    /// Largest waste first, then exact before near.
    pub groups: Vec<DuplicateGroup>,
    /// Files looked at (online libraries, not missing).
    pub files: u64,
    /// Files hashed in this run (the next run reuses the hashes).
    pub hashed: u64,
    /// Files that could not be read or changed since the last scan.
    pub skipped: u64,
    /// Files compared for near duplicates (0 before analysis).
    pub near_files: u64,
    /// Near threshold used.
    pub threshold: f32,
}

/// Core options of a query (the cancel flag is the engine's).
pub fn options(q: DuplicatesQuery) -> DupesOptions {
    DupesOptions {
        exact: q.exact,
        near: q.near,
        threshold: q.threshold.unwrap_or(DEFAULT_NEAR_THRESHOLD),
        cancel: None,
    }
}

impl From<DupeKind> for DuplicateKind {
    fn from(k: DupeKind) -> Self {
        match k {
            DupeKind::Exact => Self::Exact,
            DupeKind::Near => Self::Near,
        }
    }
}

impl From<DupeGroup> for DuplicateGroup {
    fn from(g: DupeGroup) -> Self {
        Self {
            kind: g.kind.into(),
            file_ids: g.file_ids,
            sizes: g.sizes,
            wasted_bytes: g.wasted_bytes,
            max_distance: g.max_distance,
            mean_distance: g.mean_distance,
        }
    }
}

impl From<DupesReport> for DuplicatesResult {
    fn from(r: DupesReport) -> Self {
        Self {
            groups: r.groups.into_iter().map(Into::into).collect(),
            files: count(r.files),
            hashed: count(r.hashed),
            skipped: count(r.skipped),
            near_files: count(r.near_files),
            threshold: r.threshold,
        }
    }
}

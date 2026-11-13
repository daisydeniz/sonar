//! Scanner tests on real temp directories.

mod basic;
mod changes;
mod concurrent;
mod errors;
mod moves;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::{params, OptionalExtension};
use tempfile::TempDir;

use super::*;

/// A temp library registered in an in-memory store.
pub(super) struct Lib {
    _tmp: TempDir,
    /// Canonical library root.
    pub root: PathBuf,
    pub store: Store,
    pub id: i64,
}

impl Lib {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("lib");
        fs::create_dir(&root).expect("mkdir root");
        let store = Store::open_in_memory().expect("store");
        let lib = store.add_library(&root).expect("add library");
        Self {
            root: lib.root_path,
            _tmp: tmp,
            store,
            id: lib.id,
        }
    }

    /// Directory next to the root (outside the library).
    pub fn outside(&self) -> PathBuf {
        self.root.parent().expect("root has parent").to_path_buf()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Writes `data` at `rel`, creating parent directories.
    pub fn write(&self, rel: &str, data: &[u8]) {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir -p");
        fs::write(&p, data).expect("write");
    }

    pub fn rename(&self, from: &str, to: &str) {
        let to = self.path(to);
        fs::create_dir_all(to.parent().expect("parent")).expect("mkdir -p");
        fs::rename(self.path(from), to).expect("rename");
    }

    /// Sets the mtime of `rel` to a fixed offset from the epoch.
    pub fn set_mtime(&self, rel: &str, secs: u64) {
        let f = fs::File::options()
            .write(true)
            .open(self.path(rel))
            .expect("open for mtime");
        f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .expect("set mtime");
    }

    pub fn scan(&mut self) -> ScanReport {
        scan_library(&mut self.store, self.id).expect("scan")
    }

    pub fn row(&self, rel: &str) -> Option<Row> {
        self.store
            .conn()
            .query_row(
                "SELECT id, size, mtime, quick_hash, status, error FROM files \
                 WHERE library_id = ?1 AND rel_path = ?2",
                params![self.id, rel],
                |r| {
                    Ok(Row {
                        id: r.get(0)?,
                        size: r.get(1)?,
                        mtime: r.get(2)?,
                        quick_hash: r.get(3)?,
                        status: r.get(4)?,
                        error: r.get(5)?,
                    })
                },
            )
            .optional()
            .expect("query row")
    }

    /// Row at `rel`, which must exist.
    pub fn must(&self, rel: &str) -> Row {
        self.row(rel)
            .unwrap_or_else(|| panic!("no row for {rel}; have {:?}", self.paths()))
    }

    /// All `(rel_path, status)` of this library, sorted by path.
    pub fn paths(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT rel_path, status FROM files WHERE library_id = ?1 ORDER BY rel_path")
            .expect("prepare");
        stmt.query_map([self.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows")
    }

    /// Full `files` table dump, for "nothing changed" assertions.
    pub fn dump(&self) -> Vec<Vec<rusqlite::types::Value>> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT * FROM files ORDER BY id")
            .expect("prepare");
        let n = stmt.column_count();
        stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows")
    }

    /// Gives file `id` audio_meta + analysis rows and user data (favorite,
    /// tag, override).
    pub fn decorate(&self, id: i64) {
        let c = self.store.conn();
        c.execute(
            "INSERT INTO audio_meta (file_id, duration_ms) VALUES (?1, 1000)",
            [id],
        )
        .expect("meta");
        c.execute(
            "INSERT INTO analysis (file_id, bpm) VALUES (?1, 120.0)",
            [id],
        )
        .expect("analysis");
        c.execute(
            "UPDATE files SET status = 'analyzed', analysis_version = 3 WHERE id = ?1",
            [id],
        )
        .expect("status");
        c.execute(
            "INSERT INTO favorites (file_id, added_at) VALUES (?1, 1)",
            [id],
        )
        .expect("favorite");
        c.execute(
            "INSERT OR IGNORE INTO tags (id, name) VALUES (1, 'punchy')",
            [],
        )
        .expect("tag");
        c.execute(
            "INSERT INTO file_tags (file_id, tag_id) VALUES (?1, 1)",
            [id],
        )
        .expect("file tag");
        c.execute(
            "INSERT INTO overrides (file_id, field, value) VALUES (?1, 'bpm', 128.0)",
            [id],
        )
        .expect("override");
    }

    /// `(audio_meta, analysis, favorites, file_tags, overrides)` row counts
    /// for file `id`.
    pub fn attached(&self, id: i64) -> [i64; 5] {
        [
            "audio_meta",
            "analysis",
            "favorites",
            "file_tags",
            "overrides",
        ]
        .map(|t| {
            self.store
                .conn()
                .query_row(
                    &format!("SELECT count(*) FROM {t} WHERE file_id = ?1"),
                    [id],
                    |r| r.get(0),
                )
                .expect("count")
        })
    }
}

/// Scan-relevant columns of a `files` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub id: i64,
    pub size: i64,
    pub mtime: i64,
    pub quick_hash: Option<i64>,
    pub status: String,
    pub error: Option<String>,
}

/// Counts with only the given fields set, for compact assertions.
pub(super) fn counts(
    added: usize,
    changed: usize,
    moved: usize,
    removed: usize,
    unchanged: usize,
) -> ScanCounts {
    ScanCounts {
        added,
        changed,
        moved,
        removed,
        unchanged,
        ..ScanCounts::default()
    }
}

/// Sets `path`'s mode and restores `restore_to` on drop, so tempdir cleanup
/// works even when an assertion fails.
pub(super) struct ModeGuard {
    path: PathBuf,
    restore_to: u32,
}

impl ModeGuard {
    pub fn set(path: &Path, mode: u32, restore_to: u32) -> Self {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
        Self {
            path: path.to_path_buf(),
            restore_to,
        }
    }
}

impl Drop for ModeGuard {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.restore_to));
    }
}

/// True if permission bits are not enforced for us (running as root), in
/// which case permission tests are skipped.
pub(super) fn permissions_ignored(locked_file: &Path) -> bool {
    let ignored = fs::File::open(locked_file).is_ok();
    if ignored {
        eprintln!("skipping: permission bits not enforced (running as root?)");
    }
    ignored
}

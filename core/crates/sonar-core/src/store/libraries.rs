//! Library roots (`libraries` table): register, list, look up, online flag.
//!
//! A library is identified by its root directory, stored canonicalized
//! (absolute, symlinks and `.`/`..` resolved) so the same folder reached
//! through different spellings maps to one row. Roots must be valid UTF-8
//! because `root_path` is a TEXT column.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension, Row};

use super::Store;
use crate::{Error, Result};

/// A library root registered in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// `libraries.id`.
    pub id: i64,
    /// Canonical absolute root directory.
    pub root_path: PathBuf,
    /// False when the last scan found the root missing or unreadable
    /// (e.g. an unmounted external drive); its file rows are kept.
    pub online: bool,
    /// When the library was added, unix seconds.
    pub added_at: i64,
}

const SELECT: &str = "SELECT id, root_path, online, added_at FROM libraries";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Library> {
    Ok(Library {
        id: row.get(0)?,
        root_path: PathBuf::from(row.get::<_, String>(1)?),
        online: row.get(2)?,
        added_at: row.get(3)?,
    })
}

impl Store {
    /// Registers `root` as a library and returns it. Adding a root that is
    /// already registered (under any spelling that canonicalizes to the
    /// same directory) returns the existing library unchanged.
    ///
    /// # Errors
    /// [`Error::Io`] if `root` does not exist or is not a directory,
    /// [`Error::NonUtf8Path`] if its canonical form is not UTF-8,
    /// [`Error::Sqlite`].
    pub fn add_library(&self, root: impl AsRef<Path>) -> Result<Library> {
        let root = root.as_ref();
        let canon = fs::canonicalize(root).map_err(|e| Error::io(root, e))?;
        let meta = fs::metadata(&canon).map_err(|e| Error::io(&canon, e))?;
        if !meta.is_dir() {
            let e = io::Error::new(
                io::ErrorKind::NotADirectory,
                "library root is not a directory",
            );
            return Err(Error::io(canon, e));
        }
        let text = canon
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path {
                path: canon.clone(),
            })?
            .to_owned();
        self.conn().execute(
            "INSERT INTO libraries (root_path, added_at) VALUES (?1, ?2) \
             ON CONFLICT (root_path) DO NOTHING",
            params![text, unix_now()],
        )?;
        let sql = format!("{SELECT} WHERE root_path = ?1");
        Ok(self.conn().query_row(&sql, [&text], from_row)?)
    }

    /// Removes a library and, by cascade, its file rows and everything
    /// attached to them (analysis, stages, tags, favourites...): this is the
    /// user's explicit "forget this folder". Nothing on disk is touched.
    /// Returns the ids of the removed files (so derived indexes can drop
    /// them), or `None` if there was no such library.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn remove_library(&mut self, id: i64) -> Result<Option<Vec<i64>>> {
        let tx = self.conn_mut().transaction()?;
        let ids = {
            let mut stmt = tx.prepare("SELECT id FROM files WHERE library_id = ?1")?;
            let rows = stmt.query_map([id], |r| r.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let gone = tx.execute("DELETE FROM libraries WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok((gone > 0).then_some(ids))
    }

    /// All libraries, ordered by id.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn libraries(&self) -> Result<Vec<Library>> {
        let mut stmt = self.conn().prepare(&format!("{SELECT} ORDER BY id"))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The library with this id, if any.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn library(&self, id: i64) -> Result<Option<Library>> {
        let sql = format!("{SELECT} WHERE id = ?1");
        Ok(self.conn().query_row(&sql, [id], from_row).optional()?)
    }

    /// The library rooted at `root`, if any. `root` is canonicalized when
    /// it exists; otherwise (e.g. an offline volume) its nearest existing
    /// ancestor is, and the missing tail is appended (so `/var/...` still
    /// matches a stored `/private/var/...` on macOS).
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn library_by_root(&self, root: impl AsRef<Path>) -> Result<Option<Library>> {
        let root = root.as_ref();
        let canon = resolve_maybe_missing(root);
        let Some(text) = canon.to_str() else {
            return Ok(None);
        };
        let sql = format!("{SELECT} WHERE root_path = ?1");
        Ok(self.conn().query_row(&sql, [text], from_row).optional()?)
    }

    /// Sets `libraries.online` (scanner: root reachable or not).
    pub(crate) fn set_library_online(&self, id: i64, online: bool) -> Result<()> {
        self.conn().execute(
            "UPDATE libraries SET online = ?2 WHERE id = ?1",
            params![id, online],
        )?;
        Ok(())
    }
}

/// Canonical form of `path`, which may not exist: the longest existing
/// ancestor of its absolute form is canonicalized and the rest appended.
fn resolve_maybe_missing(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    for ancestor in abs.ancestors() {
        if let Ok(canon) = fs::canonicalize(ancestor) {
            return match abs.strip_prefix(ancestor) {
                Ok(rest) if !rest.as_os_str().is_empty() => canon.join(rest),
                _ => canon,
            };
        }
    }
    abs
}

/// Current time in unix seconds (0 if the clock is before 1970).
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Store {
        Store::open_in_memory().expect("open in-memory store")
    }

    #[test]
    fn add_is_idempotent_across_spellings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("lib");
        fs::create_dir(&root).expect("mkdir");
        let store = mem();
        let a = store.add_library(&root).expect("add");
        let canon = fs::canonicalize(&root).expect("canonicalize");
        assert_eq!(a.root_path, canon);
        assert!(a.online);
        assert!(a.added_at > 1_600_000_000, "{}", a.added_at);

        let b = store.add_library(root.join(".")).expect("re-add");
        assert_eq!(a, b);
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&root, &link).expect("symlink");
        assert_eq!(store.add_library(&link).expect("via symlink"), a);
        assert_eq!(store.libraries().expect("list"), vec![a.clone()]);
    }

    #[test]
    fn lookups_by_id_and_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (r1, r2) = (tmp.path().join("one"), tmp.path().join("two"));
        for r in [&r1, &r2] {
            fs::create_dir(r).expect("mkdir");
        }
        let store = mem();
        let l1 = store.add_library(&r1).expect("add 1");
        let l2 = store.add_library(&r2).expect("add 2");
        assert_ne!(l1.id, l2.id);
        assert_eq!(
            store.libraries().expect("list"),
            vec![l1.clone(), l2.clone()]
        );
        assert_eq!(store.library(l2.id).expect("get"), Some(l2.clone()));
        assert_eq!(store.library(9999).expect("get"), None);
        assert_eq!(store.library_by_root(&r1).expect("by root"), Some(l1));
        assert_eq!(store.library_by_root(tmp.path()).expect("by root"), None);
        // A root that no longer exists is matched by its stored spelling.
        fs::remove_dir(&r2).expect("rmdir");
        assert_eq!(
            store.library_by_root(&l2.root_path).expect("offline"),
            Some(l2.clone())
        );
        // ...and by the spelling it was added with, even through symlinked
        // ancestors (macOS temp dirs live under the `/var` symlink).
        assert_eq!(store.library_by_root(&r2).expect("offline"), Some(l2));
        assert_eq!(store.library_by_root(r1.join("nope")).expect("child"), None);
    }

    #[test]
    fn remove_library_drops_its_files_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (r1, r2) = (tmp.path().join("one"), tmp.path().join("two"));
        for r in [&r1, &r2] {
            fs::create_dir(r).expect("mkdir");
        }
        let mut store = mem();
        let l1 = store.add_library(&r1).expect("add 1");
        let l2 = store.add_library(&r2).expect("add 2");
        for (lib, name) in [(l1.id, "a.wav"), (l2.id, "b.wav")] {
            store
                .conn()
                .execute(
                    "INSERT INTO files (library_id, rel_path, size, mtime) VALUES (?1, ?2, 1, 1)",
                    params![lib, name],
                )
                .expect("insert file");
        }
        let ids = store.remove_library(l1.id).expect("remove").expect("known");
        assert_eq!(ids.len(), 1);
        assert_eq!(store.libraries().expect("list"), vec![l2]);
        let left: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .expect("count");
        assert_eq!(left, 1);
        assert_eq!(store.remove_library(l1.id).expect("again"), None);
    }

    #[test]
    fn online_flag_round_trips() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = mem();
        let lib = store.add_library(tmp.path()).expect("add");
        store.set_library_online(lib.id, false).expect("offline");
        let got = store.library(lib.id).expect("get").expect("exists");
        assert!(!got.online);
        store.set_library_online(lib.id, true).expect("online");
        assert!(store.library(lib.id).expect("get").expect("exists").online);
    }

    #[test]
    fn rejects_missing_root_and_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = mem();
        let missing = tmp.path().join("nope");
        assert!(matches!(store.add_library(&missing), Err(Error::Io { .. })));
        let file = tmp.path().join("f.wav");
        fs::write(&file, b"x").expect("write");
        assert!(matches!(store.add_library(&file), Err(Error::Io { .. })));
        assert!(store.libraries().expect("list").is_empty());
    }
}

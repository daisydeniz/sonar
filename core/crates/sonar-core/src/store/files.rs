//! Lookups on the `files` table for display (search results, CLI).

use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::Result;

impl Store {
    /// Absolute path of file `id` (library root joined with the relative
    /// path), `None` if there is no such file row.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`].
    pub fn file_path(&self, id: i64) -> Result<Option<PathBuf>> {
        let row = self
            .conn()
            .prepare_cached(
                "SELECT l.root_path, f.rel_path FROM files f \
                 JOIN libraries l ON l.id = f.library_id WHERE f.id = ?1",
            )?
            .query_row([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .optional()?;
        Ok(row.map(|(root, rel)| PathBuf::from(root).join(rel)))
    }

    /// The id of the file at absolute `path` (inside one of the libraries,
    /// not `missing`), `None` if there is none. Symlinks in `path` are
    /// resolved first when the file exists, as library roots are.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`].
    pub fn file_id_by_path(&self, path: &Path) -> Result<Option<i64>> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut st = self.conn().prepare_cached(
            "SELECT id FROM files WHERE library_id = ?1 AND rel_path = ?2 AND status <> 'missing'",
        )?;
        for lib in self.libraries()? {
            let Ok(rel) = path.strip_prefix(&lib.root_path) else {
                continue;
            };
            let Some(rel) = rel.to_str() else { continue };
            if let Some(id) = st
                .query_row(params![lib.id, rel], |r| r.get(0))
                .optional()?
            {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_path_joins_root_and_rel_path() {
        let tmp = tempfile::tempdir().expect("tmp");
        let store = Store::open_in_memory().expect("store");
        let lib = store.add_library(tmp.path()).expect("lib");
        store
            .conn()
            .execute(
                "INSERT INTO files (library_id, rel_path, size, mtime) VALUES (?1, 'a/b.wav', 1, 1)",
                [lib.id],
            )
            .expect("insert");
        let id = store.conn().last_insert_rowid();
        let path = store.file_path(id).expect("lookup");
        assert_eq!(path, Some(lib.root_path.join("a/b.wav")));
        assert_eq!(store.file_path(id + 1).expect("lookup"), None);
        let abs = lib.root_path.join("a/b.wav");
        assert_eq!(store.file_id_by_path(&abs).expect("by path"), Some(id));
        let other = lib.root_path.join("a/c.wav");
        assert_eq!(store.file_id_by_path(&other).expect("by path"), None);
        assert_eq!(
            store
                .file_id_by_path(Path::new("/elsewhere/b.wav"))
                .unwrap(),
            None
        );
    }
}

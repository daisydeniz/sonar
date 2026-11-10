//! Lookups on the `files` table for display (search results, CLI).

use std::path::PathBuf;

use rusqlite::OptionalExtension;

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
    }
}

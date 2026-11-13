//! User data, part 2 (T5.5b, ADR-031): collections, ordered lists of files.
//!
//! The text index has no collection field, so membership changes need no
//! index refresh. Names are trimmed, 1..=64 characters and unique ignoring
//! case. Files are appended at the end in the order given; adding a file
//! that is already in the collection keeps its place. Deleting a collection
//! or a file never touches the other side's files.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::user_data::{clean_name, require_files, unix_now};
use super::Store;
use crate::{Error, Result};

/// A collection and its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionInfo {
    /// `collections.id`.
    pub id: i64,
    /// Name as entered.
    pub name: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Files in it.
    pub files: i64,
}

impl Store {
    /// Creates an empty collection and returns its id.
    ///
    /// # Errors
    /// [`Error::InvalidInput`] (bad name, or one that exists ignoring case),
    /// [`Error::Sqlite`].
    pub fn create_collection(&mut self, name: &str) -> Result<i64> {
        let name = clean_name(name, "collection name")?;
        let tx = self.conn_mut().transaction()?;
        ensure_name_free(&tx, &name, None)?;
        tx.execute(
            "INSERT INTO collections (name, created_at) VALUES (?1, ?2)",
            params![name, unix_now()],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    /// Renames a collection.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`], [`Error::InvalidInput`] (bad or taken
    /// name), [`Error::Sqlite`].
    pub fn rename_collection(&mut self, id: i64, name: &str) -> Result<()> {
        let name = clean_name(name, "collection name")?;
        let tx = self.conn_mut().transaction()?;
        require_collection(&tx, id)?;
        ensure_name_free(&tx, &name, Some(id))?;
        tx.execute(
            "UPDATE collections SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes a collection (its files stay). `false` if there is none.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn delete_collection(&mut self, id: i64) -> Result<bool> {
        Ok(self
            .conn()
            .execute("DELETE FROM collections WHERE id = ?1", [id])?
            > 0)
    }

    /// Appends `ids` to the collection (already-present files keep their
    /// place). Returns how many were added.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`], [`Error::FileNotFound`],
    /// [`Error::Sqlite`].
    pub fn add_to_collection(&mut self, collection: i64, ids: &[i64]) -> Result<usize> {
        let tx = self.conn_mut().transaction()?;
        require_collection(&tx, collection)?;
        require_files(&tx, ids)?;
        let mut next: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM collection_items WHERE collection_id = ?1",
            [collection],
            |r| r.get(0),
        )?;
        let mut added = 0;
        {
            let mut st = tx.prepare_cached(
                "INSERT OR IGNORE INTO collection_items (collection_id, file_id, position) \
                 VALUES (?1, ?2, ?3)",
            )?;
            for &id in ids {
                let n = st.execute(params![collection, id, next])?;
                next += i64::try_from(n).unwrap_or(0);
                added += n;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// Removes `ids` from the collection. Returns how many were in it.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`], [`Error::Sqlite`].
    pub fn remove_from_collection(&mut self, collection: i64, ids: &[i64]) -> Result<usize> {
        let tx = self.conn_mut().transaction()?;
        require_collection(&tx, collection)?;
        let mut removed = 0;
        {
            let mut st = tx.prepare_cached(
                "DELETE FROM collection_items WHERE collection_id = ?1 AND file_id = ?2",
            )?;
            for &id in ids {
                removed += st.execute(params![collection, id])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// All collections by name (ignoring case).
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn collections(&self) -> Result<Vec<CollectionInfo>> {
        let mut st = self.conn().prepare_cached(
            "SELECT c.id, c.name, c.created_at, COUNT(ci.file_id) FROM collections c \
             LEFT JOIN collection_items ci ON ci.collection_id = c.id \
             GROUP BY c.id ORDER BY c.name COLLATE NOCASE, c.id",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(CollectionInfo {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created_at: r.get(2)?,
                    files: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    /// File ids of a collection in their order.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`], [`Error::Sqlite`].
    pub fn collection_items(&self, collection: i64) -> Result<Vec<i64>> {
        let exists = self
            .conn()
            .query_row(
                "SELECT 1 FROM collections WHERE id = ?1",
                [collection],
                |_| Ok(()),
            )
            .optional()?;
        if exists.is_none() {
            return Err(Error::CollectionNotFound { id: collection });
        }
        let mut st = self.conn().prepare_cached(
            "SELECT file_id FROM collection_items WHERE collection_id = ?1 ORDER BY position",
        )?;
        let rows = st
            .query_map([collection], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }
}

fn require_collection(tx: &rusqlite::Transaction<'_>, id: i64) -> Result<()> {
    let found = tx
        .query_row("SELECT 1 FROM collections WHERE id = ?1", [id], |_| Ok(()))
        .optional()?;
    found.ok_or(Error::CollectionNotFound { id })
}

fn ensure_name_free(tx: &rusqlite::Transaction<'_>, name: &str, except: Option<i64>) -> Result<()> {
    let taken = tx
        .query_row(
            "SELECT 1 FROM collections WHERE name = ?1 COLLATE NOCASE AND id IS NOT ?2",
            params![name, except],
            |_| Ok(()),
        )
        .optional()?;
    match taken {
        Some(()) => Err(Error::InvalidInput {
            reason: format!("a collection named '{name}' already exists"),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::user_data::store_with_files;

    #[test]
    fn create_rename_delete_and_unique_names() {
        let mut s = store_with_files();
        let a = s.create_collection(" Kicks ").unwrap();
        assert!(matches!(
            s.create_collection("kicks"),
            Err(Error::InvalidInput { .. })
        ));
        assert!(s.create_collection("  ").is_err());
        let b = s.create_collection("Snares").unwrap();
        s.rename_collection(a, "Kicks 2").unwrap();
        s.rename_collection(a, "kicks 2").unwrap();
        assert!(s.rename_collection(a, "SNARES").is_err());
        assert!(matches!(
            s.rename_collection(99, "x"),
            Err(Error::CollectionNotFound { id: 99 })
        ));
        let names: Vec<_> = s
            .collections()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["kicks 2", "Snares"]);
        assert!(s.delete_collection(b).unwrap());
        assert!(!s.delete_collection(b).unwrap());
    }

    #[test]
    fn items_keep_order_and_membership_is_idempotent() {
        let mut s = store_with_files();
        let c = s.create_collection("Mix").unwrap();
        assert_eq!(s.add_to_collection(c, &[3, 1]).unwrap(), 2);
        assert_eq!(
            s.add_to_collection(c, &[1, 2]).unwrap(),
            1,
            "1 keeps its place"
        );
        assert_eq!(s.collection_items(c).unwrap(), [3, 1, 2]);
        assert_eq!(s.remove_from_collection(c, &[1, 99]).unwrap(), 1);
        assert_eq!(s.collection_items(c).unwrap(), [3, 2]);
        assert_eq!(s.collections().unwrap()[0].files, 2);
        // A later append goes after the current last, not into a gap.
        s.add_to_collection(c, &[1]).unwrap();
        assert_eq!(s.collection_items(c).unwrap(), [3, 2, 1]);
    }

    #[test]
    fn unknown_ids_are_errors_and_change_nothing() {
        let mut s = store_with_files();
        let c = s.create_collection("Mix").unwrap();
        assert!(matches!(
            s.add_to_collection(c, &[1, 42]),
            Err(Error::FileNotFound { id: 42 })
        ));
        assert!(matches!(
            s.add_to_collection(77, &[1]),
            Err(Error::CollectionNotFound { id: 77 })
        ));
        assert!(s.collection_items(c).unwrap().is_empty());
        assert!(s.collection_items(77).is_err());
    }

    #[test]
    fn deleting_a_collection_keeps_files() {
        let mut s = store_with_files();
        let c = s.create_collection("Mix").unwrap();
        s.add_to_collection(c, &[1]).unwrap();
        s.delete_collection(c).unwrap();
        let n: i64 = s
            .conn()
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
    }
}

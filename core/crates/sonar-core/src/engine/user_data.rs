//! User-data setters (T5.5b, ADR-031): favourites, hidden, tags,
//! collections and overrides.
//!
//! Every setter takes a slice of file ids (multi-select), validates, writes
//! in one transaction (all-or-nothing, see `Store::set_favorite` and
//! friends) and, when something changed and the text index is built,
//! refreshes exactly those ids with `TextIndex::refresh_files` *before it
//! returns*, so the next [`Engine::search`] sees the change. Unknown file
//! ids fail with [`Error::FileNotFound`], bad values with
//! [`Error::InvalidInput`]; nothing is written then. The return value is
//! the number of files whose state changed (0 = it was already so).
//!
//! Collections are not in the text index, so their changes never refresh.

use std::sync::atomic::Ordering;

use super::{lock, Engine};
use crate::store::{CollectionInfo, OverrideField, SimilarPair, TagInfo};
use crate::{Result, Store};

impl Engine {
    /// Runs a write on the store, then refreshes the index for `ids` if it
    /// changed anything.
    fn write_and_refresh(
        &self,
        ids: &[i64],
        write: impl FnOnce(&mut Store) -> Result<usize>,
    ) -> Result<usize> {
        let mut store = lock(&self.inner.store);
        let changed = write(&mut store)?;
        if changed > 0 && self.inner.index_ready.load(Ordering::SeqCst) {
            self.inner.sync_index(&store, ids)?;
        }
        Ok(changed)
    }

    /// Marks files as favourites or removes the mark.
    ///
    /// # Errors
    /// [`Error::FileNotFound`](crate::Error::FileNotFound),
    /// [`Error::Sqlite`](crate::Error::Sqlite),
    /// [`Error::Index`](crate::Error::Index).
    pub fn set_favorite(&self, ids: &[i64], on: bool) -> Result<usize> {
        self.write_and_refresh(ids, |s| s.set_favorite(ids, on))
    }

    /// Hides files from search (they stay in the library) or shows them again.
    ///
    /// # Errors
    /// As [`Engine::set_favorite`].
    pub fn set_hidden(&self, ids: &[i64], on: bool) -> Result<usize> {
        self.write_and_refresh(ids, |s| s.set_hidden(ids, on))
    }

    /// Adds tags to files (a tag that exists ignoring case keeps its first
    /// spelling).
    ///
    /// # Errors
    /// As [`Engine::set_favorite`], plus
    /// [`Error::InvalidInput`](crate::Error::InvalidInput) for an empty,
    /// over-long or control-character tag.
    pub fn add_tags(&self, ids: &[i64], tags: &[String]) -> Result<usize> {
        self.write_and_refresh(ids, |s| s.add_tags(ids, tags))
    }

    /// Removes tags (matched ignoring case) from files.
    ///
    /// # Errors
    /// As [`Engine::set_favorite`].
    pub fn remove_tags(&self, ids: &[i64], tags: &[String]) -> Result<usize> {
        self.write_and_refresh(ids, |s| s.remove_tags(ids, tags))
    }

    /// Every tag with its file count.
    ///
    /// # Errors
    /// [`Error::Sqlite`](crate::Error::Sqlite).
    pub fn tags(&self) -> Result<Vec<TagInfo>> {
        lock(&self.inner.store).tags()
    }

    /// Runs a tag operation that returns the files it touched, then
    /// refreshes the index for exactly those.
    fn tag_op(&self, op: impl FnOnce(&mut Store) -> Result<Vec<i64>>) -> Result<Vec<i64>> {
        let mut store = lock(&self.inner.store);
        let ids = op(&mut store)?;
        if !ids.is_empty() && self.inner.index_ready.load(Ordering::SeqCst) {
            self.inner.sync_index(&store, &ids)?;
        }
        Ok(ids)
    }

    /// Renames a tag; onto an existing tag it merges (ADR-043). Returns
    /// the ids of the files that carried it.
    ///
    /// # Errors
    /// As [`Engine::add_tags`]; an unknown tag is
    /// [`Error::InvalidInput`](crate::Error::InvalidInput).
    pub fn rename_tag(&self, from: &str, to: &str) -> Result<Vec<i64>> {
        self.tag_op(|s| s.rename_tag(from, to))
    }

    /// Merges tags into `target` (created if new). Returns the ids of the
    /// files that carried any source.
    ///
    /// # Errors
    /// As [`Engine::rename_tag`].
    pub fn merge_tags(&self, sources: &[String], target: &str) -> Result<Vec<i64>> {
        self.tag_op(|s| s.merge_tags(sources, target))
    }

    /// Deletes a tag from all files. Returns the ids of the files that
    /// carried it.
    ///
    /// # Errors
    /// As [`Engine::rename_tag`].
    pub fn delete_tag(&self, name: &str) -> Result<Vec<i64>> {
        self.tag_op(|s| s.delete_tag(name))
    }

    /// Tags that look like the same word, with a reason each, minus the
    /// ignored pairs.
    ///
    /// # Errors
    /// [`Error::Sqlite`](crate::Error::Sqlite).
    pub fn similar_tags(&self) -> Result<Vec<SimilarPair>> {
        lock(&self.inner.store).similar_tags()
    }

    /// Never suggests this pair of tags as similar again.
    ///
    /// # Errors
    /// [`Error::InvalidInput`](crate::Error::InvalidInput).
    pub fn ignore_similar_tags(&self, a: &str, b: &str) -> Result<()> {
        lock(&self.inner.store).ignore_similar_tags(a, b)
    }

    /// Sets an override of `field` (`bpm`, `key`, `category`, `kind`) on
    /// files. `value` is validated and normalized (BPM: a plain number;
    /// key, category and kind: see `OverrideField`).
    ///
    /// # Errors
    /// As [`Engine::set_favorite`], plus
    /// [`Error::InvalidInput`](crate::Error::InvalidInput) for a bad value.
    pub fn set_override(&self, ids: &[i64], field: OverrideField, value: &str) -> Result<usize> {
        self.write_and_refresh(ids, |s| s.set_override(ids, field, value))
    }

    /// Removes an override: the detected value (or filename prior) applies
    /// again.
    ///
    /// # Errors
    /// As [`Engine::set_favorite`].
    pub fn clear_override(&self, ids: &[i64], field: OverrideField) -> Result<usize> {
        self.write_and_refresh(ids, |s| s.clear_override(ids, field))
    }

    /// The overrides of one file as `(field, value)`.
    ///
    /// # Errors
    /// [`Error::Sqlite`](crate::Error::Sqlite).
    pub fn overrides_of(&self, file_id: i64) -> Result<Vec<(String, String)>> {
        lock(&self.inner.store).overrides_of(file_id)
    }

    /// Creates a collection and returns its id.
    ///
    /// # Errors
    /// [`Error::InvalidInput`](crate::Error::InvalidInput) (bad or taken
    /// name).
    pub fn create_collection(&self, name: &str) -> Result<i64> {
        lock(&self.inner.store).create_collection(name)
    }

    /// Renames a collection.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`](crate::Error::CollectionNotFound),
    /// [`Error::InvalidInput`](crate::Error::InvalidInput).
    pub fn rename_collection(&self, id: i64, name: &str) -> Result<()> {
        lock(&self.inner.store).rename_collection(id, name)
    }

    /// Deletes a collection, not its files. `false` if there is none.
    ///
    /// # Errors
    /// [`Error::Sqlite`](crate::Error::Sqlite).
    pub fn delete_collection(&self, id: i64) -> Result<bool> {
        lock(&self.inner.store).delete_collection(id)
    }

    /// Appends files to a collection; returns how many were new to it.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`](crate::Error::CollectionNotFound),
    /// [`Error::FileNotFound`](crate::Error::FileNotFound).
    pub fn add_to_collection(&self, collection: i64, ids: &[i64]) -> Result<usize> {
        lock(&self.inner.store).add_to_collection(collection, ids)
    }

    /// Removes files from a collection; returns how many were in it.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`](crate::Error::CollectionNotFound).
    pub fn remove_from_collection(&self, collection: i64, ids: &[i64]) -> Result<usize> {
        lock(&self.inner.store).remove_from_collection(collection, ids)
    }

    /// All collections with their file counts.
    ///
    /// # Errors
    /// [`Error::Sqlite`](crate::Error::Sqlite).
    pub fn collections(&self) -> Result<Vec<CollectionInfo>> {
        lock(&self.inner.store).collections()
    }

    /// File ids of a collection, in order.
    ///
    /// # Errors
    /// [`Error::CollectionNotFound`](crate::Error::CollectionNotFound).
    pub fn collection_items(&self, collection: i64) -> Result<Vec<i64>> {
        lock(&self.inner.store).collection_items(collection)
    }
}

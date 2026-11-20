//! Tag management (T12.2, ADR-043): rename, merge, delete, similar-tag
//! suggestions and ignored pairs. Adding and removing tags on files is in
//! `user_data.rs`; listing (with counts, unused tags included) is
//! `Store::tags`.
//!
//! Every operation normalises its names (`clean_tag`), runs in one
//! transaction and returns the sorted ids of the files whose tags changed,
//! so the caller can refresh the index and offer Undo. Tags are unique
//! ignoring case (ASCII only, SQLite `NOCASE`), so renaming onto an existing
//! tag is a merge and keeps the existing spelling.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension, Transaction};

use super::tag_similar::{pair_key, similar_pairs, SimilarPair};
use super::user_data::{clean_tag, collapse_spaces};
use super::Store;
use crate::{Error, Result};

fn unknown(name: &str) -> Error {
    Error::InvalidInput {
        reason: format!("no such tag: {name}"),
    }
}

fn find(tx: &Transaction<'_>, name: &str) -> Result<Option<i64>> {
    Ok(tx
        .prepare_cached("SELECT id FROM tags WHERE name = ?1")?
        .query_row([name], |r| r.get(0))
        .optional()?)
}

fn require(tx: &Transaction<'_>, name: &str) -> Result<i64> {
    find(tx, name)?.ok_or_else(|| unknown(name))
}

fn files_of(tx: &Transaction<'_>, tag: i64) -> Result<Vec<i64>> {
    let mut st = tx.prepare_cached("SELECT file_id FROM file_tags WHERE tag_id = ?1")?;
    let ids = st
        .query_map([tag], |r| r.get(0))?
        .collect::<std::result::Result<Vec<i64>, rusqlite::Error>>()?;
    Ok(ids)
}

/// Moves every link of `from` to `into`, then deletes `from`.
fn fold_into(tx: &Transaction<'_>, from: i64, into: i64) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO file_tags (file_id, tag_id) \
         SELECT file_id, ?2 FROM file_tags WHERE tag_id = ?1",
        params![from, into],
    )?;
    tx.execute("DELETE FROM file_tags WHERE tag_id = ?1", [from])?;
    tx.execute("DELETE FROM tags WHERE id = ?1", [from])?;
    Ok(())
}

fn sorted(mut ids: Vec<i64>) -> Vec<i64> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

impl Store {
    /// Renames a tag (matched ignoring case). If `to` is another existing
    /// tag the two are merged and the existing tag's spelling stays. Returns
    /// the files that carried `from`.
    ///
    /// # Errors
    /// [`Error::InvalidInput`] (unknown `from`, or a bad `to`),
    /// [`Error::Sqlite`].
    pub fn rename_tag(&mut self, from: &str, to: &str) -> Result<Vec<i64>> {
        let to = clean_tag(to)?;
        let from = collapse_spaces(from);
        let tx = self.conn_mut().transaction()?;
        let from_id = require(&tx, &from)?;
        let files = files_of(&tx, from_id)?;
        match find(&tx, &to)? {
            Some(other) if other != from_id => fold_into(&tx, from_id, other)?,
            _ => {
                tx.execute(
                    "UPDATE tags SET name = ?1 WHERE id = ?2",
                    params![to, from_id],
                )?;
            }
        }
        tx.commit()?;
        Ok(sorted(files))
    }

    /// Merges `sources` into `target` (created if it does not exist; an
    /// existing target keeps its spelling). A source equal to the target is
    /// skipped. Returns the files that carried any source.
    ///
    /// # Errors
    /// [`Error::InvalidInput`] (no sources, an unknown source or a bad
    /// name; nothing is changed), [`Error::Sqlite`].
    pub fn merge_tags(&mut self, sources: &[String], target: &str) -> Result<Vec<i64>> {
        if sources.is_empty() {
            return Err(Error::InvalidInput {
                reason: "no tags to merge".to_owned(),
            });
        }
        let target = clean_tag(target)?;
        let sources = sources
            .iter()
            .map(|s| clean_tag(s))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.conn_mut().transaction()?;
        let source_ids = sources
            .iter()
            .map(|s| require(&tx, s))
            .collect::<Result<Vec<_>>>()?;
        let target_id = match find(&tx, &target)? {
            Some(id) => id,
            None => {
                tx.execute("INSERT INTO tags (name) VALUES (?1)", [&target])?;
                tx.last_insert_rowid()
            }
        };
        let mut files = Vec::new();
        let mut done = HashSet::new();
        for id in source_ids {
            if id == target_id || !done.insert(id) {
                continue;
            }
            files.extend(files_of(&tx, id)?);
            fold_into(&tx, id, target_id)?;
        }
        tx.commit()?;
        Ok(sorted(files))
    }

    /// Deletes a tag from every file. Returns the files that carried it.
    ///
    /// # Errors
    /// [`Error::InvalidInput`] (unknown tag), [`Error::Sqlite`].
    pub fn delete_tag(&mut self, name: &str) -> Result<Vec<i64>> {
        let name = collapse_spaces(name);
        let tx = self.conn_mut().transaction()?;
        let id = require(&tx, &name)?;
        let files = files_of(&tx, id)?;
        tx.execute("DELETE FROM file_tags WHERE tag_id = ?1", [id])?;
        tx.execute("DELETE FROM tags WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(sorted(files))
    }

    /// Pairs of tags that look like the same word, per the ADR-043 rule,
    /// minus the pairs the user ignored.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn similar_tags(&self) -> Result<Vec<SimilarPair>> {
        let tags: Vec<(String, i64)> = self
            .tags()?
            .into_iter()
            .map(|t| (t.name, t.files))
            .collect();
        let mut st = self
            .conn()
            .prepare_cached("SELECT a, b FROM tag_ignored_pairs")?;
        let ignored = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<HashSet<_>, rusqlite::Error>>()?;
        Ok(similar_pairs(&tags, &ignored))
    }

    /// Remembers that `a` and `b` are not the same tag: the pair is never
    /// suggested again (order and case do not matter; the pair survives
    /// either tag being renamed away, it is simply never matched again).
    ///
    /// # Errors
    /// [`Error::InvalidInput`] (bad names, or both are the same tag),
    /// [`Error::Sqlite`].
    pub fn ignore_similar_tags(&mut self, a: &str, b: &str) -> Result<()> {
        let (a, b) = pair_key(&clean_tag(a)?, &clean_tag(b)?);
        if a == b {
            return Err(Error::InvalidInput {
                reason: "a tag cannot be similar to itself".to_owned(),
            });
        }
        self.conn().execute(
            "INSERT OR IGNORE INTO tag_ignored_pairs (a, b) VALUES (?1, ?2)",
            [a, b],
        )?;
        Ok(())
    }
}

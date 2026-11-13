//! User data, part 1 (T5.5b, ADR-031): favourites, hidden flags and tags.
//! Collections are in `collections.rs`, overrides in `overrides.rs`.
//!
//! These tables are written only here (and by the engine on the user's
//! behalf), never by analysis or scan code. Every setter takes a slice of
//! file ids (multi-select), runs in one transaction and is all-or-nothing:
//! an unknown id fails the call with [`Error::FileNotFound`] before anything
//! is changed. Setting what is already set is a no-op. Each returns how many
//! files actually changed, so a caller can skip a pointless index refresh.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Transaction};
use serde::Serialize;

use super::Store;
use crate::{Error, Result};

/// Longest tag or collection name, in characters.
pub(super) const MAX_NAME_CHARS: usize = 64;

/// A tag and how many files carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagInfo {
    /// The tag in the spelling it was first entered with (tags are unique
    /// ignoring case).
    pub name: String,
    /// Files that carry it.
    pub files: i64,
}

/// Fails with [`Error::FileNotFound`] for the first id without a `files` row.
pub(super) fn require_files(tx: &Transaction<'_>, ids: &[i64]) -> Result<()> {
    let mut st = tx.prepare_cached("SELECT 1 FROM files WHERE id = ?1")?;
    for &id in ids {
        if !st.exists([id])? {
            return Err(Error::FileNotFound { id });
        }
    }
    Ok(())
}

/// Trims a user-entered tag or collection name; rejects empty, over-long
/// and control-character names.
pub(super) fn clean_name(raw: &str, what: &str) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(Error::InvalidInput {
            reason: format!("{what} is empty"),
        });
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::InvalidInput {
            reason: format!("{what} is longer than {MAX_NAME_CHARS} characters"),
        });
    }
    if name.chars().any(char::is_control) {
        return Err(Error::InvalidInput {
            reason: format!("{what} contains a control character"),
        });
    }
    Ok(name.to_owned())
}

pub(super) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

impl Store {
    /// Marks files as favourites (`on`) or removes the mark.
    ///
    /// # Errors
    /// [`Error::FileNotFound`], [`Error::Sqlite`].
    pub fn set_favorite(&mut self, ids: &[i64], on: bool) -> Result<usize> {
        self.set_flag(
            ids,
            on,
            "INSERT OR IGNORE INTO favorites (file_id, added_at) VALUES (?1, ?2)",
            "DELETE FROM favorites WHERE file_id = ?1",
        )
    }

    /// Hides files from search (`on`) or shows them again.
    ///
    /// # Errors
    /// [`Error::FileNotFound`], [`Error::Sqlite`].
    pub fn set_hidden(&mut self, ids: &[i64], on: bool) -> Result<usize> {
        self.set_flag(
            ids,
            on,
            "INSERT OR IGNORE INTO hidden (file_id) VALUES (?1)",
            "DELETE FROM hidden WHERE file_id = ?1",
        )
    }

    fn set_flag(&mut self, ids: &[i64], on: bool, insert: &str, delete: &str) -> Result<usize> {
        let now = unix_now();
        let tx = self.conn_mut().transaction()?;
        require_files(&tx, ids)?;
        let mut changed = 0;
        {
            let mut st = tx.prepare_cached(if on { insert } else { delete })?;
            for &id in ids {
                changed += if on && insert.contains("?2") {
                    st.execute(params![id, now])?
                } else {
                    st.execute([id])?
                };
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Ids of all favourite files, ascending.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn favorites(&self) -> Result<Vec<i64>> {
        let mut st = self
            .conn()
            .prepare_cached("SELECT file_id FROM favorites ORDER BY file_id")?;
        let ids = st
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
        Ok(ids)
    }

    /// Adds `tags` to every file in `ids`. A tag that exists ignoring case
    /// keeps its first spelling. Returns the number of new file-tag links.
    ///
    /// # Errors
    /// [`Error::InvalidInput`] (empty, over-long or control-character tag),
    /// [`Error::FileNotFound`], [`Error::Sqlite`].
    pub fn add_tags(&mut self, ids: &[i64], tags: &[String]) -> Result<usize> {
        let tags = tags
            .iter()
            .map(|t| clean_name(t, "tag"))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.conn_mut().transaction()?;
        require_files(&tx, ids)?;
        let mut added = 0;
        for tag in &tags {
            tx.prepare_cached("INSERT OR IGNORE INTO tags (name) VALUES (?1)")?
                .execute([tag])?;
            let tag_id: i64 = tx
                .prepare_cached("SELECT id FROM tags WHERE name = ?1")?
                .query_row([tag], |r| r.get(0))?;
            let mut link = tx.prepare_cached(
                "INSERT OR IGNORE INTO file_tags (file_id, tag_id) VALUES (?1, ?2)",
            )?;
            for &id in ids {
                added += link.execute(params![id, tag_id])?;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// Removes `tags` (matched ignoring case) from every file in `ids`; a
    /// tag nobody carries any more is deleted. Unknown tags are ignored.
    /// Returns the number of links removed.
    ///
    /// # Errors
    /// [`Error::FileNotFound`], [`Error::Sqlite`].
    pub fn remove_tags(&mut self, ids: &[i64], tags: &[String]) -> Result<usize> {
        let tx = self.conn_mut().transaction()?;
        require_files(&tx, ids)?;
        let mut removed = 0;
        {
            let mut unlink = tx.prepare_cached(
                "DELETE FROM file_tags WHERE file_id = ?1 \
                 AND tag_id = (SELECT id FROM tags WHERE name = ?2)",
            )?;
            for tag in tags {
                for &id in ids {
                    removed += unlink.execute(params![id, tag.trim()])?;
                }
            }
        }
        tx.execute(
            "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM file_tags)",
            [],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// Every tag with its file count, by name (ignoring case).
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn tags(&self) -> Result<Vec<TagInfo>> {
        let mut st = self.conn().prepare_cached(
            "SELECT t.name, COUNT(ft.file_id) FROM tags t \
             LEFT JOIN file_tags ft ON ft.tag_id = t.id \
             GROUP BY t.id ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(TagInfo {
                    name: r.get(0)?,
                    files: r.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    /// Tags of one file, by name (ignoring case).
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn file_tags(&self, file_id: i64) -> Result<Vec<String>> {
        let mut st = self.conn().prepare_cached(
            "SELECT t.name FROM file_tags ft JOIN tags t ON t.id = ft.tag_id \
             WHERE ft.file_id = ?1 ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = st
            .query_map([file_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }
}

/// Test helper shared by the user-data modules: a store with files 1..=3.
#[cfg(test)]
pub(super) fn store_with_files() -> Store {
    let store = Store::open_in_memory().expect("store");
    store
        .conn()
        .execute_batch(
            "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/lib', 0);
             INSERT INTO files (id, library_id, rel_path, size, mtime) VALUES
               (1, 1, 'a.wav', 10, 0), (2, 1, 'b.wav', 10, 0), (3, 1, 'c.wav', 10, 0);",
        )
        .expect("seed");
    store
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_and_hidden_toggle_idempotently() {
        let mut s = store_with_files();
        assert_eq!(s.set_favorite(&[1, 2], true).unwrap(), 2);
        assert_eq!(
            s.set_favorite(&[2, 3], true).unwrap(),
            1,
            "2 was already set"
        );
        assert_eq!(s.favorites().unwrap(), [1, 2, 3]);
        assert_eq!(s.set_favorite(&[1, 3], false).unwrap(), 2);
        assert_eq!(s.favorites().unwrap(), [2]);
        assert_eq!(s.set_hidden(&[3], true).unwrap(), 1);
        assert_eq!(s.set_hidden(&[3], false).unwrap(), 1);
        assert_eq!(s.set_hidden(&[3], false).unwrap(), 0);
    }

    #[test]
    fn unknown_file_fails_the_whole_call() {
        let mut s = store_with_files();
        let err = s.set_favorite(&[1, 42], true).unwrap_err();
        assert!(matches!(err, Error::FileNotFound { id: 42 }), "{err}");
        assert!(s.favorites().unwrap().is_empty(), "nothing was changed");
        assert!(matches!(
            s.add_tags(&[42], &["x".into()]),
            Err(Error::FileNotFound { id: 42 })
        ));
        assert!(s.tags().unwrap().is_empty());
    }

    #[test]
    fn tags_keep_first_spelling_and_count_files() {
        let mut s = store_with_files();
        assert_eq!(
            s.add_tags(&[1, 2], &["  Punchy ".into(), "dark".into()])
                .unwrap(),
            4
        );
        assert_eq!(s.add_tags(&[3], &["PUNCHY".into()]).unwrap(), 1);
        assert_eq!(s.add_tags(&[3], &["punchy".into()]).unwrap(), 0);
        let tags = s.tags().unwrap();
        let shown: Vec<_> = tags.iter().map(|t| (t.name.as_str(), t.files)).collect();
        assert_eq!(shown, [("dark", 2), ("Punchy", 3)]);
        assert_eq!(s.file_tags(3).unwrap(), ["Punchy"]);
    }

    #[test]
    fn removing_tags_drops_orphans_and_ignores_unknown() {
        let mut s = store_with_files();
        s.add_tags(&[1, 2], &["a".into(), "b".into()]).unwrap();
        assert_eq!(
            s.remove_tags(&[1], &["A".into(), "nope".into()]).unwrap(),
            1
        );
        assert_eq!(s.tags().unwrap().len(), 2, "a is still on file 2");
        assert_eq!(s.remove_tags(&[2], &["a".into()]).unwrap(), 1);
        assert_eq!(s.tags().unwrap().len(), 1, "a is gone with its last file");
    }

    #[test]
    fn bad_tag_names_are_errors_not_panics() {
        let mut s = store_with_files();
        for bad in ["", "   ", "a\u{1f}b", "line\nbreak", &"x".repeat(65)] {
            let err = s.add_tags(&[1], &[bad.to_owned()]).unwrap_err();
            assert!(matches!(err, Error::InvalidInput { .. }), "{bad:?}: {err}");
        }
        assert!(s.tags().unwrap().is_empty());
        assert!(s.add_tags(&[1], &["x".repeat(64)]).is_ok());
    }
}

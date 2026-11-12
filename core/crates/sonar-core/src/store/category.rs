//! Category columns of the `analysis` table (T5.3, ADR-028): `category`,
//! `category_conf`, `category_source`.
//!
//! The `dsp` stage writes the DSP + name category ([`upsert_category`] with
//! `keep_refined`), the `embed` stage overwrites it with the CLAP-refined
//! one. `keep_refined` makes the `dsp` write a no-op for a file whose
//! current-version `embed` result exists, so re-running only the DSP stage
//! (a `DSP_VERSION` bump) never replaces a CLAP-refined category with the
//! weaker DSP-only one. Only these three columns are written; user
//! overrides live in `overrides` and are never touched.

use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::classify::{Category, CategoryResult, CategorySource};
use crate::jobs::{Stage, EMBED_VERSION};
use crate::Result;

/// The category of one file as stored.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CategoryRow {
    /// Category; `None` when there was no evidence.
    pub category: Option<Category>,
    /// Confidence of `category` (`0..=1`).
    pub conf: Option<f32>,
    /// Where it came from (`detected` / `filename`).
    pub source: Option<CategorySource>,
}

impl CategoryRow {
    /// Row values for a classification (`Fallback` → all NULL).
    pub fn new(r: &CategoryResult) -> Self {
        match (r.category, r.source) {
            (Some(c), s) if s != CategorySource::Fallback => Self {
                category: Some(c),
                conf: Some(r.confidence),
                source: Some(s),
            },
            _ => Self::default(),
        }
    }
}

/// Writes the three category columns of `file_id` (inserting the row if
/// needed). With `keep_refined`, a file whose current `embed` stage
/// finished (`ok`/`partial`) is left alone.
///
/// # Errors
/// [`crate::Error::Sqlite`], e.g. a foreign-key violation when `file_id`
/// has no `files` row.
pub fn upsert_category(
    conn: &Connection,
    file_id: i64,
    v: &CategoryRow,
    keep_refined: bool,
) -> Result<()> {
    if keep_refined {
        let refined: Option<i64> = conn
            .prepare_cached(
                "SELECT 1 FROM file_stages WHERE file_id = ?1 AND stage = ?2 \
                 AND version >= ?3 AND outcome IN ('ok', 'partial')",
            )?
            .query_row(
                params![file_id, Stage::Embed.as_str(), EMBED_VERSION],
                |r| r.get(0),
            )
            .optional()?;
        if refined.is_some() {
            return Ok(());
        }
    }
    let category = v.category.map(Category::as_str);
    let conf = category.and(
        v.conf
            .filter(|c| c.is_finite())
            .map(|c| f64::from(c.clamp(0.0, 1.0))),
    );
    let source = category.and(v.source.and_then(CategorySource::as_db));
    conn.prepare_cached(
        "INSERT INTO analysis (file_id, category, category_conf, category_source) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (file_id) DO UPDATE SET category = excluded.category, \
           category_conf = excluded.category_conf, category_source = excluded.category_source",
    )?
    .execute(params![file_id, category, conf, source])?;
    Ok(())
}

/// The category columns of `file_id`, if it has an `analysis` row. An
/// unknown id reads as `None`.
///
/// # Errors
/// [`crate::Error::Sqlite`].
pub fn get_category(conn: &Connection, file_id: i64) -> Result<Option<CategoryRow>> {
    let raw: Option<(Option<String>, Option<f64>, Option<String>)> = conn
        .query_row(
            "SELECT category, category_conf, category_source FROM analysis WHERE file_id = ?1",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(raw.map(|(c, conf, source)| CategoryRow {
        category: c.as_deref().and_then(Category::parse),
        conf: conf.map(|x| x as f32),
        source: match source.as_deref() {
            Some("detected") => Some(CategorySource::Detected),
            Some("filename") => Some(CategorySource::Filename),
            _ => None,
        },
    }))
}

impl Store {
    /// [`get_category`] on this store's connection.
    ///
    /// # Errors
    /// See [`get_category`].
    pub fn category(&self, file_id: i64) -> Result<Option<CategoryRow>> {
        get_category(self.conn(), file_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_file() -> (Store, i64) {
        let store = Store::open_in_memory().unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/lib', 0);
                 INSERT INTO files (id, library_id, rel_path, size, mtime)
                   VALUES (7, 1, 'a.wav', 10, 0);
                 INSERT INTO analysis (file_id, kind, key) VALUES (7, 'loop', 'Am');",
            )
            .unwrap();
        (store, 7)
    }

    fn row(c: Category, conf: f32) -> CategoryRow {
        CategoryRow {
            category: Some(c),
            conf: Some(conf),
            source: Some(CategorySource::Detected),
        }
    }

    #[test]
    fn round_trip_keeps_other_columns_and_clamps() {
        let (store, id) = store_with_file();
        upsert_category(store.conn(), id, &row(Category::Hat, 1.5), true).unwrap();
        assert_eq!(store.category(id).unwrap(), Some(row(Category::Hat, 1.0)));
        let (kind, key): (String, String) = store
            .conn()
            .query_row(
                "SELECT kind, key FROM analysis WHERE file_id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((kind.as_str(), key.as_str()), ("loop", "Am"));
        upsert_category(store.conn(), id, &CategoryRow::default(), true).unwrap();
        assert_eq!(store.category(id).unwrap(), Some(CategoryRow::default()));
        assert!(upsert_category(store.conn(), 999, &row(Category::Kick, 0.5), false).is_err());
    }

    #[test]
    fn dsp_write_never_replaces_a_refined_category() {
        let (store, id) = store_with_file();
        upsert_category(store.conn(), id, &row(Category::Bass, 0.9), false).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO file_stages (file_id, stage, version, outcome, done_at) \
                 VALUES (?1, 'embed', ?2, 'ok', 0)",
                params![id, EMBED_VERSION],
            )
            .unwrap();
        upsert_category(store.conn(), id, &row(Category::Kick, 0.4), true).unwrap();
        assert_eq!(store.category(id).unwrap(), Some(row(Category::Bass, 0.9)));
        upsert_category(store.conn(), id, &row(Category::Kick, 0.4), false).unwrap();
        assert_eq!(store.category(id).unwrap(), Some(row(Category::Kick, 0.4)));
    }
}

//! What gets indexed for a file, read from the store.
//!
//! Every file row except `status = 'missing'` is indexed (files of an
//! offline library keep their status, so they stay searchable; hidden
//! files are indexed with a flag and filtered at query time). Category,
//! kind, BPM and key are effective values (ADR-018): a user override wins
//! (also an explicit NULL, which clears the value), else the analysis
//! column, else — BPM and key only — the filename prior (ADR-016).

use std::path::Path;

use rusqlite::{params, Row};
use tantivy::TantivyDocument;

use super::schema::Fields;
use crate::search::keys::{key_code, parse_key};
use crate::text::{filename_prior, fold, strip_audio_extension, Key};
use crate::{Result, Store};

/// What is indexed for one file: searchable text plus filter values.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IndexDoc {
    /// `files.id`.
    pub file_id: i64,
    /// File name without its media extension (`Kick_01`).
    pub name: String,
    /// Library root folder name and the folders below it, `/`-separated
    /// (`Splice/Drums/Kicks`).
    pub folders: String,
    /// User tags.
    pub tags: Vec<String>,
    /// Effective category (`kick`, `bass`, ...), if known.
    pub category: Option<String>,
    /// Effective kind (`loop` / `one_shot`), if known.
    pub kind: Option<String>,
    /// `files.library_id` (0 = none, e.g. docs built in tests).
    pub library_id: i64,
    /// Effective tempo in BPM, if known.
    pub bpm: Option<f64>,
    /// Effective key, if known (mode `None` is filtered as major).
    pub key: Option<Key>,
    /// Duration in milliseconds (`audio_meta`), if measured.
    pub duration_ms: Option<u64>,
    /// In the user's favourites.
    pub favorite: bool,
    /// Hidden by the user (excluded from results unless asked for).
    pub hidden: bool,
}

impl IndexDoc {
    /// A document with name and folders taken from the library `root` and
    /// the file's `/`-separated `rel_path`; no tags/category/kind.
    pub fn from_paths(file_id: i64, root: &Path, rel_path: &str) -> Self {
        let (dirs, file) = rel_path.rsplit_once('/').unwrap_or(("", rel_path));
        let root_name = root.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let folders = match (root_name.is_empty(), dirs.is_empty()) {
            (false, false) => format!("{root_name}/{dirs}"),
            (false, true) => root_name.to_owned(),
            _ => dirs.to_owned(),
        };
        Self {
            file_id,
            name: strip_audio_extension(file).to_owned(),
            folders,
            ..Self::default()
        }
    }

    /// The Tantivy document, `None` for a negative id (never produced by
    /// SQLite rowids).
    pub(crate) fn to_document(&self, f: &Fields) -> Option<TantivyDocument> {
        let mut doc = TantivyDocument::default();
        doc.add_u64(f.file_id, u64::try_from(self.file_id).ok()?);
        self.add_filter_values(f, &mut doc);
        doc.add_text(f.name, &self.name);
        doc.add_text(f.path, &self.folders);
        for tag in &self.tags {
            doc.add_text(f.tags, tag);
        }
        if let Some(c) = &self.category {
            doc.add_text(f.category, c);
        }
        if let Some(k) = &self.kind {
            doc.add_text(f.kind, k);
        }
        Some(doc)
    }

    /// Filter / sort / facet values ([`super::schema::ff`]); unknown
    /// values are left out.
    fn add_filter_values(&self, f: &Fields, doc: &mut TantivyDocument) {
        let ff = &f.ff;
        doc.add_text(ff.name, fold(&self.name));
        for (field, value) in [(ff.category, &self.category), (ff.kind, &self.kind)] {
            if let Some(v) = value.as_deref().map(facet_value).filter(|v| !v.is_empty()) {
                doc.add_text(field, v);
            }
        }
        for tag in self.tags.iter().map(|t| facet_value(t)) {
            if !tag.is_empty() {
                doc.add_text(ff.tags, tag);
            }
        }
        if let Some(k) = self.key {
            doc.add_u64(ff.key, u64::from(key_code(k)));
        }
        if let Some(centi) = self.bpm.and_then(bpm_centi) {
            doc.add_u64(ff.bpm, centi);
        }
        if let Some(ms) = self.duration_ms {
            doc.add_u64(ff.duration, ms);
        }
        if let Ok(lib) = u64::try_from(self.library_id) {
            doc.add_u64(ff.library, lib);
        }
        if self.favorite {
            doc.add_u64(ff.favorite, 1);
        }
        if self.hidden {
            doc.add_u64(ff.hidden, 1);
        }
    }
}

/// Normal form of a string filter value (category, kind, tag): trimmed
/// and folded (lowercase, no diacritics). Index and filters both use it.
pub(crate) fn facet_value(raw: &str) -> String {
    fold(raw.trim())
}

/// BPM as a whole number of hundredths (`120.5` → `12050`), `None` for
/// non-finite or non-positive values.
pub(crate) fn bpm_centi(bpm: f64) -> Option<u64> {
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    // Finite and positive: the cast saturates, it cannot wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((bpm * 100.0).round() as u64)
}

/// Tag separator inside `group_concat` (ASCII unit separator).
const TAG_SEP: char = '\u{1f}';

/// Effective value of an overridable analysis column (NULL if unknown or
/// cleared by an override).
macro_rules! effective {
    ($field:literal) => {
        concat!(
            "CASE WHEN EXISTS (SELECT 1 FROM overrides o WHERE o.file_id = f.id AND o.field = '",
            $field,
            "') THEN (SELECT CAST(o.value AS TEXT) FROM overrides o WHERE o.file_id = f.id AND o.field = '",
            $field,
            "') ELSE a.",
            $field,
            " END"
        )
    };
}

/// Like [`effective!`] but as TEXT, and `''` when an override cleared the
/// value, so the caller can tell "cleared" (no filename prior either)
/// from "unknown" (NULL: the filename prior may fill in).
macro_rules! effective_or_cleared {
    ($field:literal) => {
        concat!(
            "CAST(CASE WHEN EXISTS (SELECT 1 FROM overrides o WHERE o.file_id = f.id AND o.field = '",
            $field,
            "') THEN COALESCE((SELECT CAST(o.value AS TEXT) FROM overrides o WHERE o.file_id = f.id AND o.field = '",
            $field,
            "'), '') ELSE a.",
            $field,
            " END AS TEXT)"
        )
    };
}

const SELECT: &str = concat!(
    "SELECT f.id, f.rel_path, l.root_path, ",
    "(SELECT group_concat(t.name, char(31)) FROM file_tags ft JOIN tags t ON t.id = ft.tag_id ",
    "WHERE ft.file_id = f.id), ",
    effective!("category"),
    ", ",
    effective!("kind"),
    ", f.library_id, ",
    effective_or_cleared!("bpm"),
    ", ",
    effective_or_cleared!("key"),
    ", m.duration_ms, ",
    "EXISTS (SELECT 1 FROM favorites fv WHERE fv.file_id = f.id), ",
    "EXISTS (SELECT 1 FROM hidden h WHERE h.file_id = f.id)",
    " FROM files f JOIN libraries l ON l.id = f.library_id ",
    "LEFT JOIN analysis a ON a.file_id = f.id ",
    "LEFT JOIN audio_meta m ON m.file_id = f.id ",
    "WHERE f.status <> 'missing'"
);

fn from_row(row: &Row<'_>) -> rusqlite::Result<IndexDoc> {
    let root: String = row.get(2)?;
    let rel: String = row.get(1)?;
    let mut doc = IndexDoc::from_paths(row.get(0)?, Path::new(&root), &rel);
    let tags: Option<String> = row.get(3)?;
    doc.tags = tags
        .map(|t| t.split(TAG_SEP).map(str::to_owned).collect())
        .unwrap_or_default();
    doc.category = row.get(4)?;
    doc.kind = row.get(5)?;
    doc.library_id = row.get(6)?;
    let bpm: Option<String> = row.get(7)?;
    let key: Option<String> = row.get(8)?;
    // Filename priors only where neither an override nor analysis spoke.
    let prior = (bpm.is_none() || key.is_none()).then(|| filename_prior(Path::new(&rel)));
    doc.bpm = match bpm {
        Some(v) => v
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|&b| bpm_centi(b).is_some()),
        None => prior.and_then(|p| p.bpm).map(|b| f64::from(b.bpm)),
    };
    doc.key = match key {
        Some(v) => parse_key(&v),
        None => prior.and_then(|p| p.key).map(|k| k.key),
    };
    let duration: Option<i64> = row.get(9)?;
    doc.duration_ms = duration.and_then(|d| u64::try_from(d).ok());
    doc.favorite = row.get(10)?;
    doc.hidden = row.get(11)?;
    Ok(doc)
}

/// Calls `f` for every indexable file, in id order, streaming rows.
pub(crate) fn for_each_doc(store: &Store, mut f: impl FnMut(IndexDoc) -> Result<()>) -> Result<()> {
    let mut stmt = store.conn().prepare(&format!("{SELECT} ORDER BY f.id"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        f(from_row(row)?)?;
    }
    Ok(())
}

/// The document of file `id`, `None` if it does not exist or is missing.
pub(crate) fn doc(store: &Store, id: i64) -> Result<Option<IndexDoc>> {
    let mut stmt = store
        .conn()
        .prepare_cached(&format!("{SELECT} AND f.id = ?1"))?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(from_row(row)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_folders_from_paths() {
        let d = IndexDoc::from_paths(7, Path::new("/Vol/Splice"), "Drums/Kicks/Kick_01.wav");
        assert_eq!(d.file_id, 7);
        assert_eq!(d.name, "Kick_01");
        assert_eq!(d.folders, "Splice/Drums/Kicks");
        let d = IndexDoc::from_paths(1, Path::new("/Vol/Lib"), "Snare.aif");
        assert_eq!((d.name.as_str(), d.folders.as_str()), ("Snare", "Lib"));
        let d = IndexDoc::from_paths(1, Path::new("/"), "a/b.mp3");
        assert_eq!((d.name.as_str(), d.folders.as_str()), ("b", "a"));
    }
}

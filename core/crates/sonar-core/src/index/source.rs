//! What gets indexed for a file, read from the store.
//!
//! Every file row except `status = 'missing'` is indexed (files of an
//! offline library keep their status, so they stay searchable). Category
//! and kind are the effective values: a user override wins (also an
//! explicit NULL, which clears the value), else the analysis column.

use std::path::Path;

use rusqlite::{params, Row};
use tantivy::TantivyDocument;

use super::schema::Fields;
use crate::text::strip_audio_extension;
use crate::{Result, Store};

/// The searchable text of one file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
}

/// Tag separator inside `group_concat` (ASCII unit separator).
const TAG_SEP: char = '\u{1f}';

/// Effective value of an overridable analysis column.
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

const SELECT: &str = concat!(
    "SELECT f.id, f.rel_path, l.root_path, ",
    "(SELECT group_concat(t.name, char(31)) FROM file_tags ft JOIN tags t ON t.id = ft.tag_id ",
    "WHERE ft.file_id = f.id), ",
    effective!("category"),
    ", ",
    effective!("kind"),
    " FROM files f JOIN libraries l ON l.id = f.library_id ",
    "LEFT JOIN analysis a ON a.file_id = f.id ",
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

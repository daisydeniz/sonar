//! Display rows (T5.6): what a result list shows for a file, with effective
//! values (ADR-018: user override, else analysis, else filename prior) read
//! from the store with the same query the text index uses, so a row never
//! disagrees with what search filtered on.

use std::path::PathBuf;

use super::{lock, Engine};
use crate::index::file_doc;
use crate::{Result, Store};

/// Everything a result row shows for one file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    /// `files.id`.
    pub file_id: i64,
    /// Library the file belongs to.
    pub library_id: i64,
    /// Absolute path.
    pub path: PathBuf,
    /// File name without its media extension.
    pub name: String,
    /// Library folder name and the folders below it, `/`-separated.
    pub folders: String,
    /// Effective category (`kick`, ...), if known.
    pub category: Option<String>,
    /// Effective kind (`loop` / `one_shot`), if known.
    pub kind: Option<String>,
    /// Effective tempo, if known.
    pub bpm: Option<f64>,
    /// Effective key as text (`C#m`), if known.
    pub key: Option<String>,
    /// Playable length (ms), once the meta stage ran.
    pub duration_ms: Option<u64>,
    /// Sample rate of the stored format (Hz), once the meta stage ran.
    pub sample_rate: Option<u32>,
    /// Channel count of the stored format, once the meta stage ran.
    pub channels: Option<u16>,
    /// In the user's favourites.
    pub favorite: bool,
    /// Hidden by the user.
    pub hidden: bool,
    /// User tags.
    pub tags: Vec<String>,
}

fn load(store: &Store, id: i64) -> Result<Option<FileRow>> {
    let Some(doc) = file_doc(store, id)? else {
        return Ok(None);
    };
    let Some(path) = store.file_path(id)? else {
        return Ok(None);
    };
    let meta = store.audio_meta(id)?;
    Ok(Some(FileRow {
        file_id: doc.file_id,
        library_id: doc.library_id,
        path,
        name: doc.name,
        folders: doc.folders,
        category: doc.category,
        kind: doc.kind,
        bpm: doc.bpm,
        key: doc.key.map(|k| k.to_string()),
        duration_ms: doc.duration_ms,
        sample_rate: meta.as_ref().map(|m| m.sample_rate),
        channels: meta.as_ref().map(|m| m.channels),
        favorite: doc.favorite,
        hidden: doc.hidden,
        tags: doc.tags,
    }))
}

impl Engine {
    /// The display row of file `id`; `None` if the id is unknown or the
    /// file is marked missing.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`].
    pub fn row(&self, id: i64) -> Result<Option<FileRow>> {
        load(&lock(&self.inner.store), id)
    }

    /// Rows for `ids` in the order given (one store lock for the whole
    /// page); unknown or missing ids are skipped.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`].
    pub fn rows(&self, ids: &[i64]) -> Result<Vec<FileRow>> {
        let store = lock(&self.inner.store);
        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            out.extend(load(&store, id)?);
        }
        Ok(out)
    }
}

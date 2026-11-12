//! Store of CLAP audio embeddings (T5.2, ADR-027): one 512-d, L2-normalised
//! `f32` row per file, in two append-only files under `<db dir>/index/clap`
//! next to the aspect vector store (`index/vectors`).
//!
//! - `ids.i64`: file id of each row (little endian); `emb.f32`: the rows
//!   (`rows x 512 x f32`, little endian). Row `r` of both is one file.
//! - **Append**: the embedding rows are written first, the ids after them,
//!   so the committed row count is `min(ids rows, embedding rows)`. A torn
//!   append (kill -9, power loss) is cut off by the next writer on open and
//!   loses only that batch; the job pipeline appends *before* its SQLite
//!   batch commit, so that batch is simply embedded again.
//! - **Duplicates**: appending a file id that already has a row makes the
//!   new row win; the old row stays as a dead row (counted by
//!   [`EmbedStats::dead`]) until a compaction (not implemented yet).
//!   `file id -> row` is rebuilt from `ids.i64` on open.
//! - **Readers** ([`EmbedStore::open_read`]) memory-map the committed rows
//!   and never lock; one writer per directory ([`EmbedStore::open`], an
//!   exclusive `LOCK` file lock). Rows of deleted files are not removed:
//!   consumers filter by the live file set, as for the text index.
//! - `meta.json` records the format and dimension; a store of another
//!   format is replaced by an empty one by a writer (the stage then embeds
//!   everything again, see `jobs::embed`) and refused by readers.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use memmap2::Mmap;
use serde::Serialize;

use super::{Embedding, EMBED_DIM};
use crate::index::vectors::{as_f32, as_i64};
use crate::{Error, Result};

/// Store directory relative to the data directory.
pub const EMBED_SUBDIR: [&str; 2] = ["index", "clap"];
/// On-disk format version (`meta.json`).
pub const FORMAT_VERSION: u32 = 1;
const IDS_FILE: &str = "ids.i64";
const ROWS_FILE: &str = "emb.f32";
const META_FILE: &str = "meta.json";
const LOCK_FILE: &str = "LOCK";
/// How long a writer waits for the directory lock (see `open`).
const LOCK_PATIENCE: std::time::Duration = std::time::Duration::from_millis(500);
const ROW_BYTES: u64 = (EMBED_DIM * 4) as u64;

/// Embedding store directory for the database at `db_path`
/// (`<db dir>/index/clap`).
pub fn dir_for_db(db_path: &Path) -> PathBuf {
    let mut dir = db_path.parent().map(Path::to_path_buf).unwrap_or_default();
    dir.extend(EMBED_SUBDIR);
    dir
}

/// Row counts and sizes (for `sonar embed --stats` and tests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmbedStats {
    /// Store directory.
    pub dir: PathBuf,
    /// Committed rows (live + dead).
    pub rows: u64,
    /// Rows that are the current embedding of a file.
    pub live: u64,
    /// Superseded rows.
    pub dead: u64,
    /// Bytes of the row files.
    pub bytes: u64,
}

/// What [`EmbedStore::open`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenState {
    /// Existing store (a torn tail, if any, was cut off).
    Ready,
    /// No store existed; an empty one was created.
    New,
    /// The store was of another format and was replaced by an empty one.
    Reset,
}

struct Writer {
    ids: File,
    rows: File,
    /// Held for the lifetime of the writer.
    _lock: File,
}

struct Maps {
    ids: Mmap,
    rows: Mmap,
}

/// The embedding store (see the module docs).
pub struct EmbedStore {
    dir: PathBuf,
    rows: u64,
    by_file: HashMap<i64, u32>,
    /// Writer-side id slots (readers use the mapped file).
    writer: Option<Writer>,
    maps: Option<Maps>,
}

impl std::fmt::Debug for EmbedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbedStore")
            .field("dir", &self.dir)
            .field("rows", &self.rows)
            .field("writable", &self.writer.is_some())
            .finish_non_exhaustive()
    }
}

fn store_err(dir: &Path, reason: impl Into<String>) -> Error {
    Error::VectorStore {
        path: dir.to_path_buf(),
        reason: reason.into(),
    }
}

fn meta_json() -> String {
    format!("{{\"format\":{FORMAT_VERSION},\"dim\":{EMBED_DIM}}}")
}

fn meta_matches(dir: &Path) -> bool {
    fs::read_to_string(dir.join(META_FILE)).is_ok_and(|s| s.trim() == meta_json())
}

/// Committed rows of the two files on disk: the shorter of the id and
/// embedding files, in whole rows.
fn committed_rows(dir: &Path) -> Result<u64> {
    let len = |name: &str| {
        let p = dir.join(name);
        match fs::metadata(&p) {
            Ok(m) => Ok(m.len()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(Error::io(p, e)),
        }
    };
    Ok((len(IDS_FILE)? / 8).min(len(ROWS_FILE)? / ROW_BYTES))
}

impl EmbedStore {
    /// Opens (creating if needed) the store in `dir` for writing.
    ///
    /// # Errors
    /// [`Error::Io`]; [`Error::VectorStore`] if another writer holds the
    /// directory lock.
    pub fn open(dir: impl AsRef<Path>) -> Result<(Self, OpenState)> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let lock_path = dir.join(LOCK_FILE);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| Error::io(&lock_path, e))?;
        // `flock` belongs to the open file description, and a child spawned
        // by another thread (`Command`) shares it until its `exec`, so a lock
        // just released by `drop` can stay held for a few milliseconds: wait
        // briefly before reporting a real second writer.
        let deadline = std::time::Instant::now() + LOCK_PATIENCE;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(store_err(dir, "another Sonar process is writing it"))
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(Error::io(lock_path, e)),
            }
        }
        let existing = dir.join(META_FILE).exists();
        let state = if !existing {
            OpenState::New
        } else if meta_matches(dir) {
            OpenState::Ready
        } else {
            tracing::warn!(dir = %dir.display(), "embedding store of another format, starting a new one");
            for name in [IDS_FILE, ROWS_FILE] {
                let p = dir.join(name);
                if let Err(e) = fs::remove_file(&p) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(Error::io(p, e));
                    }
                }
            }
            OpenState::Reset
        };
        let rows = committed_rows(dir)?;
        let open = |name: &str| -> Result<File> {
            let p = dir.join(name);
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&p)
                .map_err(|e| Error::io(p, e))
        };
        let (ids, emb) = (open(IDS_FILE)?, open(ROWS_FILE)?);
        // Cut a torn tail so the next append starts on a row boundary.
        let cut = |f: &File, name: &str, len: u64| {
            f.set_len(len).map_err(|e| Error::io(dir.join(name), e))
        };
        cut(&ids, IDS_FILE, rows * 8)?;
        cut(&emb, ROWS_FILE, rows * ROW_BYTES)?;
        let meta_path = dir.join(META_FILE);
        fs::write(&meta_path, meta_json()).map_err(|e| Error::io(meta_path, e))?;
        let by_file = read_ids(&dir.join(IDS_FILE), rows)?;
        let store = Self {
            dir: dir.to_path_buf(),
            rows,
            by_file,
            writer: Some(Writer {
                ids,
                rows: emb,
                _lock: lock,
            }),
            maps: None,
        };
        Ok((store, state))
    }

    /// Opens a read-only, memory-mapped view of the committed rows.
    ///
    /// # Errors
    /// [`Error::VectorStore`] if there is no store or it is of another
    /// format; [`Error::Io`].
    pub fn open_read(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        if !dir.join(META_FILE).exists() {
            return Err(store_err(dir, "no embedding store yet"));
        }
        if !meta_matches(dir) {
            return Err(store_err(dir, "embedding store of another format"));
        }
        let rows = committed_rows(dir)?;
        let map = |name: &str| -> Result<Mmap> {
            let p = dir.join(name);
            let f = File::open(&p).map_err(|e| Error::io(&p, e))?;
            // SAFETY: the file is only ever appended to past the committed
            // rows (the only bytes read) or cut back to them by a writer
            // that has just opened it; nothing shrinks it below `rows`.
            unsafe { Mmap::map(&f) }.map_err(|e| Error::io(p, e))
        };
        let maps = if rows == 0 {
            None
        } else {
            Some(Maps {
                ids: map(IDS_FILE)?,
                rows: map(ROWS_FILE)?,
            })
        };
        let by_file = read_ids(&dir.join(IDS_FILE), rows)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            rows,
            by_file,
            writer: None,
            maps,
        })
    }

    /// Appends one embedding per `(file id, embedding)`; the new rows win
    /// over older rows of the same file. File ids must be positive.
    ///
    /// # Errors
    /// [`Error::VectorStore`] on a read-only view; [`Error::Io`].
    pub fn append(&mut self, batch: &[(i64, &Embedding)]) -> Result<()> {
        let dir = self.dir.clone();
        let Some(w) = self.writer.as_mut() else {
            return Err(store_err(&dir, "opened read-only"));
        };
        if batch.is_empty() {
            return Ok(());
        }
        if batch.iter().any(|(id, _)| *id <= 0) {
            return Err(store_err(&dir, "file ids must be positive"));
        }
        let mut emb = Vec::with_capacity(batch.len() * EMBED_DIM * 4);
        let mut ids = Vec::with_capacity(batch.len() * 8);
        for (id, e) in batch {
            ids.extend_from_slice(&id.to_le_bytes());
            for v in e.iter() {
                emb.extend_from_slice(&v.to_le_bytes());
            }
        }
        let at = |f: &mut File, name: &str, bytes: &[u8], len: u64| -> Result<()> {
            use std::io::{Seek, SeekFrom};
            let p = dir.join(name);
            f.seek(SeekFrom::Start(len)).map_err(|e| Error::io(&p, e))?;
            f.write_all(bytes).map_err(|e| Error::io(&p, e))
        };
        // Rows first, ids second: the ids file defines what is committed.
        at(&mut w.rows, ROWS_FILE, &emb, self.rows * ROW_BYTES)?;
        at(&mut w.ids, IDS_FILE, &ids, self.rows * 8)?;
        for (i, (id, _)) in batch.iter().enumerate() {
            let row = u32::try_from(self.rows + i as u64)
                .map_err(|_| store_err(&dir, "more than 4 billion rows"))?;
            self.by_file.insert(*id, row);
        }
        self.rows += batch.len() as u64;
        Ok(())
    }

    /// Committed rows (live + dead).
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Files with a current embedding.
    pub fn live_rows(&self) -> u64 {
        self.by_file.len() as u64
    }

    /// Whether `file_id` has a current embedding.
    pub fn contains(&self, file_id: i64) -> bool {
        self.by_file.contains_key(&file_id)
    }

    /// File ids with a current embedding (any order).
    pub fn file_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.by_file.keys().copied()
    }

    /// Row of `file_id`'s current embedding.
    pub fn row_of(&self, file_id: i64) -> Option<u64> {
        self.by_file.get(&file_id).map(|&r| u64::from(r))
    }

    /// All committed rows, row-major (`rows() x 512`); empty without
    /// rows. Read-only views only (`None` for a writer).
    pub fn matrix(&self) -> Option<&[f32]> {
        let m = self.maps.as_ref()?;
        let n = usize::try_from(self.rows).ok()? * EMBED_DIM;
        as_f32(m.rows.get(..n * 4)?)
    }

    /// File id slot of every committed row (superseded rows keep their id;
    /// use [`EmbedStore::get`] or [`EmbedStore::iter`] for live rows).
    pub fn id_slots(&self) -> Option<&[i64]> {
        let m = self.maps.as_ref()?;
        as_i64(m.ids.get(..usize::try_from(self.rows).ok()? * 8)?)
    }

    /// The current embedding of `file_id` (read-only views).
    pub fn get(&self, file_id: i64) -> Option<&Embedding> {
        let row = usize::try_from(self.row_of(file_id)?).ok()?;
        let m = self.matrix()?;
        m.get(row * EMBED_DIM..(row + 1) * EMBED_DIM)?
            .try_into()
            .ok()
    }

    /// `(file id, embedding)` of every live row, in row order (read-only
    /// views). This is the read API of T5.3/T5.4.
    pub fn iter(&self) -> impl Iterator<Item = (i64, &Embedding)> + '_ {
        let ids = self.id_slots().unwrap_or(&[]);
        let matrix = self.matrix().unwrap_or(&[]);
        ids.iter().enumerate().filter_map(move |(row, &id)| {
            (self.by_file.get(&id).map(|&r| r as usize) == Some(row))
                .then(|| {
                    let e: &Embedding = matrix
                        .get(row * EMBED_DIM..(row + 1) * EMBED_DIM)?
                        .try_into()
                        .ok()?;
                    Some((id, e))
                })
                .flatten()
        })
    }

    /// Counts and sizes.
    pub fn stats(&self) -> EmbedStats {
        EmbedStats {
            dir: self.dir.clone(),
            rows: self.rows,
            live: self.live_rows(),
            dead: self.rows - self.live_rows(),
            bytes: self.rows * (ROW_BYTES + 8),
        }
    }
}

/// `file id -> last row` of the first `rows` ids of `path`.
fn read_ids(path: &Path, rows: u64) -> Result<HashMap<i64, u32>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(Error::io(path, e)),
    };
    let n = usize::try_from(rows)
        .unwrap_or(usize::MAX)
        .min(bytes.len() / 8);
    let mut map = HashMap::with_capacity(n);
    for (row, c) in bytes.as_chunks::<8>().0.iter().take(n).enumerate() {
        let id = i64::from_le_bytes(*c);
        if id > 0 {
            let row = u32::try_from(row).map_err(|_| store_err(path, "too many rows"))?;
            map.insert(id, row);
        }
    }
    Ok(map)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

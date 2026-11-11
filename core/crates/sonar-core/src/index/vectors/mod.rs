//! Aspect vector store (T4.1, ADR-023, docs/ARCHITECTURE.md §3/§5): raw
//! per-file aspect vectors in memory-mapped, fixed-width row files under
//! `<data dir>/index/vectors`, next to the text index.
//!
//! - **Rows**: one dense row per stored vector set. `ids.i64` holds the
//!   file id of each row; `spectrum.f32`, `timbre.f32`, `pitch.f32` and
//!   `amplitude.f32` hold the **raw** vectors ([`RAW_ASPECTS`]), row-major,
//!   so row `r` of every file belongs to the same file id.
//! - **Mapping**: file id → row is rebuilt in memory from `ids.i64` on open
//!   (no SQLite table). Appending a file id that already has a row makes
//!   the new row win and tombstones the old one ([`TOMBSTONE`] in its id
//!   slot); a crash between the two still resolves by "last row wins".
//! - **Normalization** is not stored per row: `norm.json` keeps the
//!   [`LibraryNorm`] over every committed row (live or dead), merged per
//!   appended batch. Removed rows are not subtracted; compaction drops them
//!   and recomputes the stats exactly from the live rows. z-scored aspects
//!   and Overall are derived at query time ([`VectorStore::normalized`]).
//! - **Crash safety**: an append writes the rows past the committed count,
//!   then bumps the row count in every header. On open the committed count
//!   is the smallest header count / whole rows on disk, and a writer cuts
//!   anything beyond it, so a torn append (kill -9) loses only that batch.
//!   The jobs pipeline appends *before* its SQLite batch commit: a crash in
//!   between re-analyzes the batch, which appends again (last row wins).
//! - **Generations**: compaction and resets write a new `gen-<n>/` and swap
//!   the `CURRENT` pointer by rename (see [`generation`]).
//! - **Concurrency**: one writer per directory (an exclusive `LOCK` file
//!   lock, held by [`VectorStore::open`]); any number of read-only
//!   [`VectorStore::open_read`] views.

mod file;
mod generation;
mod write;

#[cfg(test)]
mod tests;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::dsp::aspects::{Aspect, AspectVectors, LibraryNorm, NormalizedAspects};
use crate::{paths, Error, Result};

pub use file::FORMAT_VERSION;
pub use write::CompactReport;

use file::{as_f32, as_i64};
use generation::Generation;

/// Vector store directory relative to the data directory.
pub const VECTORS_SUBDIR: [&str; 2] = ["index", "vectors"];
/// Aspects stored as raw matrices, in file/kind order (Overall is derived).
pub const RAW_ASPECTS: [Aspect; 4] = [
    Aspect::Spectrum,
    Aspect::Timbre,
    Aspect::Pitch,
    Aspect::Amplitude,
];
/// Id slot value of a dead row (SQLite file ids start at 1).
pub const TOMBSTONE: i64 = 0;
const LOCK_FILE: &str = "LOCK";
/// Attempts of a reader racing a compaction that swaps generations.
const READ_ATTEMPTS: usize = 3;

/// Vector store directory for the store database at `db_path`
/// (`<db dir>/index/vectors`, the data directory for the default store).
pub fn dir_for_db(db_path: &Path) -> PathBuf {
    let mut dir = db_path.parent().map(Path::to_path_buf).unwrap_or_default();
    dir.extend(VECTORS_SUBDIR);
    dir
}

/// Default location: `<data dir>/index/vectors`.
///
/// # Errors
/// [`Error::NoDataDir`], [`Error::Io`].
pub fn default_dir() -> Result<PathBuf> {
    let mut dir = paths::data_dir()?;
    dir.extend(VECTORS_SUBDIR);
    Ok(dir)
}

/// What a writable [`VectorStore::open`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenState {
    /// Existing store opened (a torn tail, if any, was dropped).
    Ready,
    /// No store existed; an empty one was created.
    New,
    /// The store could not be used (other format/aspects version or
    /// dimension, damaged files) and was replaced by an empty one.
    Reset {
        /// Why.
        reason: String,
    },
}

/// Row counts and sizes for `sonar vectors`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VectorStats {
    /// Store directory.
    pub dir: PathBuf,
    /// Live generation number.
    pub generation: u64,
    /// Committed rows (live + dead).
    pub rows: u64,
    /// Rows that are the current vectors of a file.
    pub live: u64,
    /// Tombstoned or superseded rows (dropped by compaction).
    pub dead: u64,
    /// Files whose vectors the normalization stats include.
    pub norm_count: u64,
    /// Bytes of the row files.
    pub bytes: u64,
    /// `(aspect, dims)` of the stored raw matrices.
    pub aspects: Vec<(String, usize)>,
}

/// The vector store (see the module docs).
#[derive(Debug)]
pub struct VectorStore {
    dir: PathBuf,
    generation: u64,
    gen: Generation,
    /// Held for the lifetime of a writer (`None` for read-only views).
    lock: Option<File>,
}

impl VectorStore {
    /// Opens (creating if needed) the store in `dir` for writing. A store
    /// this build cannot use is replaced by an empty one
    /// ([`OpenState::Reset`]); a torn append is cut off.
    ///
    /// # Errors
    /// [`Error::Io`]; [`Error::VectorStore`] if another writer holds the
    /// directory lock.
    pub fn open(dir: impl AsRef<Path>) -> Result<(Self, OpenState)> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let lock = take_lock(dir)?;
        let current = generation::read_current(dir)?;
        generation::remove_stale(dir, current)?;
        let (g, state) = match current {
            None => (1, OpenState::New),
            Some(g) => match generation::open(&generation::gen_dir(dir, g), true) {
                Ok(gen) => {
                    let store = Self {
                        dir: dir.into(),
                        generation: g,
                        gen,
                        lock: Some(lock),
                    };
                    return Ok((store, OpenState::Ready));
                }
                Err(e) if is_unusable(&e) => {
                    tracing::warn!(error = %e, "vector store unusable, starting a new one");
                    (
                        g + 1,
                        OpenState::Reset {
                            reason: e.to_string(),
                        },
                    )
                }
                Err(e) => return Err(e),
            },
        };
        let gen_path = generation::create(dir, g)?;
        let gen = generation::open(&gen_path, true)?;
        for f in gen.files() {
            f.sync()?;
        }
        generation::write_current(dir, g)?;
        generation::remove_stale(dir, Some(g))?;
        let store = Self {
            dir: dir.into(),
            generation: g,
            gen,
            lock: Some(lock),
        };
        Ok((store, state))
    }

    /// Opens a read-only view of the committed rows in `dir`.
    ///
    /// # Errors
    /// [`Error::VectorStore`] if there is no store or it does not match
    /// this build (version, dims, damaged header); [`Error::Io`].
    pub fn open_read(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut last = None;
        for _ in 0..READ_ATTEMPTS {
            let Some(g) = generation::read_current(dir)? else {
                return Err(Error::VectorStore {
                    path: dir.into(),
                    reason: "no vector store yet".into(),
                });
            };
            match generation::open(&generation::gen_dir(dir, g), false) {
                Ok(gen) => {
                    return Ok(Self {
                        dir: dir.into(),
                        generation: g,
                        gen,
                        lock: None,
                    })
                }
                // A compaction may have swapped generations meanwhile.
                Err(e) if generation::read_current(dir)? != Some(g) => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| Error::VectorStore {
            path: dir.into(),
            reason: "generation keeps changing".into(),
        }))
    }

    /// Store directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Committed rows (live + dead).
    pub fn rows(&self) -> u64 {
        self.gen.rows
    }

    /// Rows that are the current vectors of a file.
    pub fn live_rows(&self) -> u64 {
        self.gen.by_file.len() as u64
    }

    /// Row of `file_id`'s current vectors.
    pub fn row_of(&self, file_id: i64) -> Option<u64> {
        self.gen.by_file.get(&file_id).map(|&r| u64::from(r))
    }

    /// File id of `row` if the row is live (not tombstoned or superseded).
    pub fn file_id_at(&self, row: u64) -> Option<i64> {
        let id = *self.id_slots().get(usize::try_from(row).ok()?)?;
        (self.row_of(id) == Some(row)).then_some(id)
    }

    /// Raw id slot of every committed row ([`TOMBSTONE`] for deleted rows;
    /// superseded rows keep their id, use [`Self::file_id_at`]).
    pub fn id_slots(&self) -> &[i64] {
        as_i64(self.gen.ids.data(self.gen.rows)).unwrap_or(&[])
    }

    /// File ids with live vectors (any order).
    pub fn file_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.gen.by_file.keys().copied()
    }

    /// The raw matrix of `aspect` (row-major, `rows() × aspect.dim()`);
    /// `None` for [`Aspect::Overall`], which is derived.
    pub fn matrix(&self, aspect: Aspect) -> Option<&[f32]> {
        let k = RAW_ASPECTS.iter().position(|&a| a == aspect)?;
        as_f32(self.gen.aspects[k].data(self.gen.rows))
    }

    /// Raw vector of `aspect` in `row` (live or dead).
    pub fn row(&self, aspect: Aspect, row: u64) -> Option<&[f32]> {
        let d = aspect.dim();
        let r = usize::try_from(row).ok()?;
        self.matrix(aspect)?.get(r * d..(r + 1) * d)
    }

    /// Raw vectors of `file_id`.
    pub fn vectors(&self, file_id: i64) -> Option<AspectVectors> {
        let row = self.row_of(file_id)?;
        let get = |a| self.row(a, row).map(<[f32]>::to_vec);
        Some(AspectVectors {
            spectrum: get(Aspect::Spectrum)?,
            timbre: get(Aspect::Timbre)?,
            pitch: get(Aspect::Pitch)?,
            amplitude: get(Aspect::Amplitude)?,
        })
    }

    /// z-scored aspects and Overall of `file_id` with the library stats.
    pub fn normalized(&self, file_id: i64) -> Option<NormalizedAspects> {
        self.vectors(file_id).map(|v| self.gen.norm.normalize(&v))
    }

    /// Library normalization stats over every committed row.
    pub fn norm(&self) -> &LibraryNorm {
        &self.gen.norm
    }

    /// Counts and sizes.
    pub fn stats(&self) -> VectorStats {
        VectorStats {
            dir: self.dir.clone(),
            generation: self.generation,
            rows: self.rows(),
            live: self.live_rows(),
            dead: self.rows() - self.live_rows(),
            norm_count: self.gen.norm.count(),
            bytes: self.gen.files().map(|f| f.mapped_len()).sum(),
            aspects: RAW_ASPECTS
                .iter()
                .map(|a| (a.name().to_owned(), a.dim()))
                .collect(),
        }
    }

    fn writable(&self) -> Result<()> {
        match self.lock {
            Some(_) => Ok(()),
            None => Err(Error::VectorStore {
                path: self.dir.clone(),
                reason: "opened read-only".into(),
            }),
        }
    }
}

/// Errors after which a writer replaces the store instead of failing.
fn is_unusable(e: &Error) -> bool {
    match e {
        Error::VectorStore { .. } => true,
        Error::Io { source, .. } => source.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

fn take_lock(dir: &Path) -> Result<File> {
    let path = dir.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| Error::io(&path, e))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(Error::VectorStore {
            path,
            reason: "another Sonar process is writing it".into(),
        }),
        Err(std::fs::TryLockError::Error(e)) => Err(Error::io(path, e)),
    }
}

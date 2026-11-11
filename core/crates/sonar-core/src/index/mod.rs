//! Full-text index (Tantivy) over file names, folders, tags, category and
//! kind, plus filter/sort/facet fields (docs/ARCHITECTURE.md §6, ADR-017,
//! ADR-018). Queries: [`crate::search`].
//!
//! - One document per indexable file ([`IndexDoc`]), keyed by the
//!   `file_id` u64 field (indexed for delete-by-id, fast for reading hits).
//! - Text fields use the Sonar analysis chain ([`analyzer`]) with positions.
//! - `ff_*` fields hold effective values (override ?? detected ?? filename
//!   prior) as raw strings / u64 fast fields for filters, sort and facets.
//! - On disk under `<data dir>/index/text` with a version file holding the
//!   schema and analyzer versions; a mismatch (or a build that never
//!   finished) wipes the directory and reports [`OpenState::NeedsRebuild`],
//!   so an old index never answers with wrong tokens.
//! - Freshness: [`TextIndex::rebuild`] from the store, or
//!   [`TextIndex::refresh_files`] with the ids a scan/watch/analysis batch
//!   touched (re-reads them; deleted or missing files leave the index).
//!
//! Aspect vectors for similarity live next to it in [`vectors`]
//! (`<data dir>/index/vectors`, ADR-023).

pub mod analyzer;
mod schema;
mod source;
pub mod vectors;
mod write;

#[cfg(test)]
mod tests;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tantivy::{Index, IndexReader, ReloadPolicy, Searcher};

pub use analyzer::SonarTokenizer;
pub use schema::SCHEMA_VERSION;
pub(crate) use schema::{ff, Fields, FILE_ID as FILE_ID_FIELD};
pub use source::IndexDoc;
pub(crate) use source::{bpm_centi, facet_value};
pub use write::BuildReport;

use crate::text::ANALYZER_VERSION;
use crate::{paths, Error, Result};

/// Index directory relative to the data directory.
pub const INDEX_SUBDIR: [&str; 2] = ["index", "text"];

/// File in the index directory recording which build wrote it.
const VERSION_FILE: &str = "sonar-index-version";

/// Tantivy's own metadata file (marks a directory as a Tantivy index).
const TANTIVY_META: &str = "meta.json";

/// Content of the version file for this build.
pub fn index_version() -> String {
    format!("sonar text index: schema {SCHEMA_VERSION}, analyzer {ANALYZER_VERSION}")
}

/// Whether an opened index can be searched as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenState {
    /// Built by this version; contents are as fresh as the last write.
    Ready,
    /// Empty: call [`TextIndex::rebuild`] before searching.
    NeedsRebuild(RebuildReason),
}

/// Why an index has to be rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildReason {
    /// No index existed.
    New,
    /// Written by another schema/analyzer version.
    VersionChanged {
        /// Version line found on disk.
        found: String,
    },
    /// A previous build did not finish (no version file).
    Incomplete,
    /// Tantivy could not open it (corrupt, schema mismatch).
    Unreadable {
        /// What failed.
        reason: String,
    },
}

impl fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::New => f.write_str("no index yet"),
            Self::VersionChanged { found } => write!(f, "index version changed (was: {found})"),
            Self::Incomplete => f.write_str("previous build did not finish"),
            Self::Unreadable { reason } => write!(f, "index unreadable: {reason}"),
        }
    }
}

/// The text index: Tantivy index, a manually reloaded reader and the
/// field handles. Writes are serialized inside the process; Tantivy's
/// directory lock rejects a second writing process.
pub struct TextIndex {
    index: Index,
    reader: IndexReader,
    fields: Fields,
    dir: Option<PathBuf>,
    write_lock: Mutex<()>,
}

impl fmt::Debug for TextIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextIndex")
            .field("dir", &self.dir)
            .field("num_docs", &self.num_docs())
            .finish_non_exhaustive()
    }
}

impl TextIndex {
    /// Default location: `<data dir>/index/text`.
    ///
    /// # Errors
    /// [`Error::NoDataDir`], [`Error::Io`].
    pub fn default_dir() -> Result<PathBuf> {
        let mut dir = paths::data_dir()?;
        dir.extend(INDEX_SUBDIR);
        Ok(dir)
    }

    /// Opens the index in [`TextIndex::default_dir`] (see [`TextIndex::open`]).
    ///
    /// # Errors
    /// See [`TextIndex::open`].
    pub fn open_default() -> Result<(Self, OpenState)> {
        Self::open(Self::default_dir()?)
    }

    /// Opens the index in `dir`, creating the directory if needed. An index
    /// of another version, an unfinished build or an unreadable index is
    /// wiped and replaced by an empty one ([`OpenState::NeedsRebuild`]).
    ///
    /// # Errors
    /// [`Error::Io`], [`Error::ForeignIndexDir`] if `dir` holds files that
    /// are not a Sonar index (never deleted), [`Error::Index`].
    pub fn open(dir: impl AsRef<Path>) -> Result<(Self, OpenState)> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let reason = match read_version(dir)? {
            Some(v) if v == index_version() => match Self::open_existing(dir) {
                Ok(ix) => return Ok((ix, OpenState::Ready)),
                Err(e) => RebuildReason::Unreadable {
                    reason: e.to_string(),
                },
            },
            Some(found) => RebuildReason::VersionChanged { found },
            None if is_empty_dir(dir)? => RebuildReason::New,
            None if dir.join(TANTIVY_META).exists() => RebuildReason::Incomplete,
            None => return Err(Error::ForeignIndexDir { path: dir.into() }),
        };
        if reason != RebuildReason::New {
            fs::remove_dir_all(dir).map_err(|e| Error::io(dir, e))?;
            fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        let (schema, _) = Fields::schema();
        let index = Index::create_in_dir(dir, schema)?;
        Ok((
            Self::from_index(index, Some(dir.into()))?,
            OpenState::NeedsRebuild(reason),
        ))
    }

    /// A fresh in-memory index (tests, benchmarks).
    ///
    /// # Errors
    /// [`Error::Index`].
    pub fn in_ram() -> Result<Self> {
        let (schema, _) = Fields::schema();
        Self::from_index(Index::create_in_ram(schema), None)
    }

    fn open_existing(dir: &Path) -> Result<Self> {
        let index = Index::open_in_dir(dir)?;
        if index.schema() != Fields::schema().0 {
            return Err(Error::Index(tantivy::TantivyError::SchemaError(
                "schema differs from this build".into(),
            )));
        }
        Self::from_index(index, Some(dir.into()))
    }

    fn from_index(index: Index, dir: Option<PathBuf>) -> Result<Self> {
        index
            .tokenizers()
            .register(analyzer::TOKENIZER_NAME, SonarTokenizer::default());
        let fields = Fields::from_schema(&index.schema()).ok_or_else(|| {
            Error::Index(tantivy::TantivyError::SchemaError(
                "missing Sonar fields".into(),
            ))
        })?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        Ok(Self {
            index,
            reader,
            fields,
            dir,
            write_lock: Mutex::new(()),
        })
    }

    /// Index directory, `None` in memory.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Number of searchable documents.
    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// A searcher on the last committed state.
    pub(crate) fn searcher(&self) -> Searcher {
        self.reader.searcher()
    }

    /// Field handles.
    pub(crate) fn fields(&self) -> &Fields {
        &self.fields
    }
}

/// Trimmed content of the version file, `None` if there is none.
fn read_version(dir: &Path) -> Result<Option<String>> {
    let path = dir.join(VERSION_FILE);
    match fs::read_to_string(&path) {
        Ok(v) => Ok(Some(v.trim().to_owned())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path, e)),
    }
}

fn is_empty_dir(dir: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    Ok(entries.next().is_none())
}

//! `SonarEngine`: the one object the app talks to (ADR-032). Every method is
//! a thin call into `sonar_core::engine::Engine` plus type conversion, run
//! under `guard` so errors become `FfiError` and panics never unwind into
//! Swift.
//!
//! # Threading
//! The object is `Send + Sync`; Swift may call it from any thread. Methods
//! marked **blocking** do real work on the caller's thread (disk, decode,
//! scan): call them off the main actor (`Task.detached` or a background
//! queue). All others return in well under a millisecond.

use std::sync::Arc;

use sonar_core::engine::{Engine, EngineOptions};
use sonar_core::jobs::{JobOptions, Stage};
use sonar_core::similar::{Seed, SimilarOptions};
use sonar_core::watch::WatchOptions;

use crate::convert::{self, count};
use crate::dupes_types::{self, DuplicatesQuery, DuplicatesResult};
use crate::error::{guard, guard_ffi, FfiError};
use crate::facet_types::SearchFacets;
use crate::similar_types::{self, SimilarHit, SimilarPage, SimilarQuery};
use crate::types::{
    AddedLibrary, CollectionInfo, EngineEvent, FileRow, IndexingProgress, LibraryInfo,
    OverrideField, PreviewPcm, ScanInfo, SearchPage, SearchQuery, TagInfo, WaveformPeak,
};

/// Largest page a search returns (a bigger `limit` is cut to it).
pub const MAX_PAGE: u32 = 1000;

/// The Sonar core: libraries, indexing, search, user data, audio helpers.
#[derive(uniffi::Object)]
pub struct SonarEngine {
    engine: Engine,
}

#[uniffi::export]
impl SonarEngine {
    /// Opens (or creates) the data directory `home`: database, search
    /// index and vectors live below it. **Blocking** (may rebuild the search
    /// index of a large library the first time).
    #[uniffi::constructor]
    pub fn open(home: String) -> Result<Arc<Self>, FfiError> {
        guard(|| Engine::open_with(home, EngineOptions::default())).map(Self::wrap)
    }

    /// Opens the default data directory (`SONAR_HOME` or Application
    /// Support). **Blocking** like `open`.
    #[uniffi::constructor]
    pub fn open_default() -> Result<Arc<Self>, FfiError> {
        guard(Engine::open_default).map(Self::wrap)
    }

    // ---- libraries ----------------------------------------------------

    /// Registered libraries, oldest first.
    pub fn libraries(&self) -> Result<Vec<LibraryInfo>, FfiError> {
        guard(|| self.engine.libraries()).map(|l| l.iter().map(LibraryInfo::from).collect())
    }

    /// Adds the folder as a library and scans it; its files are searchable
    /// by name when this returns (analysis follows via `startIndexing`).
    /// **Blocking** (a scan).
    pub fn add_library(&self, path: String) -> Result<AddedLibrary, FfiError> {
        let (library, report) = guard(|| self.engine.add_library(&path))?;
        Ok(AddedLibrary {
            library: LibraryInfo::from(&library),
            scan: ScanInfo::from(&report),
        })
    }

    /// Rescans one library. **Blocking.**
    pub fn rescan_library(&self, library_id: i64) -> Result<ScanInfo, FfiError> {
        guard(|| self.engine.scan_library(library_id)).map(|r| ScanInfo::from(&r))
    }

    /// Removes a library and its files from Sonar (never from disk);
    /// `false` if it did not exist. **Blocking** (touches the index).
    pub fn remove_library(&self, library_id: i64) -> Result<bool, FfiError> {
        guard(|| self.engine.remove_library(library_id))
    }

    /// Starts watching every library for changes; new files are indexed
    /// automatically while indexing is on. Results arrive in `pollEvents`.
    pub fn start_watching(&self) -> Result<(), FfiError> {
        guard(|| self.engine.start_watching(WatchOptions::default()))
    }

    /// Stops watching. May block until a running scan aborts: call it off
    /// the main actor.
    pub fn stop_watching(&self) {
        self.engine.stop_watching();
    }

    // ---- indexing -------------------------------------------------------

    /// Starts analyzing everything pending (format and duration, then
    /// tempo/key/category/similarity) on core-owned threads and keeps doing
    /// so for files found later. `include_embeddings` adds the CLAP stage,
    /// which is skipped when the model files are absent. Returns `false`
    /// if a job was already running. Poll `indexingProgress`.
    pub fn start_indexing(&self, include_embeddings: bool) -> Result<bool, FfiError> {
        let mut stages = vec![Stage::Meta, Stage::Dsp];
        if include_embeddings {
            stages.push(Stage::Embed);
        }
        guard(|| {
            self.engine.start_indexing(JobOptions {
                stages,
                ..JobOptions::default()
            })
        })
    }

    /// Pauses the running job (files in flight finish first).
    pub fn pause_indexing(&self) {
        self.engine.pause_indexing();
    }

    /// Continues a paused job.
    pub fn resume_indexing(&self) {
        self.engine.resume_indexing();
    }

    /// Stops the job and turns automatic indexing off.
    pub fn cancel_indexing(&self) {
        self.engine.cancel_indexing();
    }

    /// Current indexing state; cheap, poll it (e.g. every 250 ms).
    pub fn indexing_progress(&self) -> IndexingProgress {
        convert::progress(&self.engine.indexing_status())
    }

    /// Drains background events (oldest first, at most 1024 are kept).
    pub fn poll_events(&self) -> Vec<EngineEvent> {
        self.engine
            .poll_events()
            .into_iter()
            .filter_map(convert::event)
            .collect()
    }

    // ---- search ---------------------------------------------------------

    /// One page of results with display fields. **Blocking** (milliseconds;
    /// semantic search may load the CLAP text model on first use). Latest
    /// query wins: a call still running when a newer `search` or
    /// `cancelSearch` arrives throws `FfiError.Cancelled`; never show its
    /// result. `limit` is cut to 1000.
    pub fn search(&self, query: SearchQuery) -> Result<SearchPage, FfiError> {
        let mut req = convert::request(query)?;
        req.limit = req.limit.min(MAX_PAGE as usize);
        let limit = req.limit;
        let results = guard(|| self.engine.search(&req))?;
        let ids: Vec<i64> = results.hits.iter().map(|h| h.file_id).collect();
        let rows = guard(|| self.engine.rows(&ids))?;
        Ok(SearchPage {
            has_more: limit > 0 && results.hits.len() >= limit,
            rows: rows.into_iter().map(FileRow::from).collect(),
            total: count(results.text_total),
            mode: results.mode.into(),
            fallback: results.fallback,
            facets: results.facets.map(SearchFacets::from),
        })
    }

    /// Cancels the search in flight (it throws `FfiError.Cancelled`).
    pub fn cancel_search(&self) {
        self.engine.cancel_search();
    }

    /// Files that sound like `file_id`, nearest first. Needs the file to be
    /// analyzed (`FfiError.NotFound` for an unknown file, `Internal` while
    /// no similarity vectors exist yet). **Blocking.**
    pub fn similar(&self, file_id: i64, limit: u32) -> Result<Vec<FileRow>, FfiError> {
        let opts = SimilarOptions {
            limit: limit.min(MAX_PAGE) as usize,
            ..SimilarOptions::new()
        };
        let found = guard(|| self.engine.similar(&Seed::File(file_id), &opts))?;
        let ids: Vec<i64> = found.hits.iter().map(|h| h.file_id).collect();
        guard(|| self.engine.rows(&ids)).map(|r| r.into_iter().map(FileRow::from).collect())
    }

    /// Files that sound like the seed (a library file or any audio file on
    /// disk), nearest first, with aspect weights and filters. **Blocking**
    /// (milliseconds for a library seed; an external seed is decoded and
    /// analyzed first). Shares the search cancel flag, latest wins: a newer
    /// `search`/`similarTo` or `cancelSearch` makes a running call throw
    /// `FfiError.Cancelled` (also during decoding); never show its result.
    /// A `file` seed is never in its own result. Errors: `NotFound` (seed
    /// not analyzed), `InvalidInput` (weights, filters), `Audio`/`Io`
    /// (external file), `Internal` (no analysis has run yet: "no vector
    /// store yet").
    pub fn similar_to(&self, query: SimilarQuery) -> Result<SimilarPage, FfiError> {
        let (seed, opts) = similar_types::query(query)?;
        let found = guard(|| self.engine.similar_latest(&seed, &opts))?;
        let ids: Vec<i64> = found.hits.iter().map(|h| h.file_id).collect();
        let rows = guard(|| self.engine.rows(&ids))?;
        // Keep rows and hits aligned even if a file vanished meanwhile.
        let mut page = SimilarPage {
            rows: Vec::with_capacity(rows.len()),
            hits: Vec::with_capacity(rows.len()),
            candidates: count(found.candidates),
        };
        for row in rows {
            if let Some(hit) = found.hits.iter().find(|h| h.file_id == row.file_id) {
                page.hits.push(SimilarHit::from(hit));
                page.rows.push(FileRow::from(row));
            }
        }
        Ok(page)
    }

    /// Duplicate groups over all libraries: byte-identical files and the
    /// same sound in other files (ADR-036). **Blocking**: the first run
    /// reads every file that shares size and head/tail bytes with another
    /// (hashes are cached), the near pass takes about a second per 500k
    /// files. Latest wins on its own flag (search does not cancel it): a
    /// newer `duplicates` or `cancelDuplicates` makes a running call throw
    /// `FfiError.Cancelled`. Unreadable/offline files are left out, not
    /// errors. `InvalidInput` for a threshold outside 0...0.5.
    pub fn duplicates(&self, query: DuplicatesQuery) -> Result<DuplicatesResult, FfiError> {
        let opts = dupes_types::options(query);
        guard(|| self.engine.duplicates_latest(&opts)).map(Into::into)
    }

    /// Cancels the running `duplicates` call (it throws `Cancelled`).
    pub fn cancel_duplicates(&self) {
        self.engine.cancel_duplicates();
    }

    /// The row of one file, `nil` if unknown. `row`/`rows` wait on the
    /// store lock (held by `removeLibrary` during a large delete): call them
    /// off the main actor.
    pub fn row(&self, file_id: i64) -> Result<Option<FileRow>, FfiError> {
        guard(|| self.engine.row(file_id)).map(|r| r.map(FileRow::from))
    }

    /// Rows for ids in the order given; unknown ids are skipped.
    pub fn rows(&self, file_ids: Vec<i64>) -> Result<Vec<FileRow>, FfiError> {
        guard(|| self.engine.rows(&file_ids)).map(|r| r.into_iter().map(FileRow::from).collect())
    }

    // ---- user data (all setters: multi-id, all-or-nothing, the next
    // search sees the change; they return how many files changed) ---------

    /// Adds or removes favourites.
    pub fn set_favorite(&self, file_ids: Vec<i64>, on: bool) -> Result<u64, FfiError> {
        guard(|| self.engine.set_favorite(&file_ids, on)).map(count)
    }

    /// Hides or unhides files (hidden files drop out of search).
    pub fn set_hidden(&self, file_ids: Vec<i64>, on: bool) -> Result<u64, FfiError> {
        guard(|| self.engine.set_hidden(&file_ids, on)).map(count)
    }

    /// Adds tags (created on first use).
    pub fn add_tags(&self, file_ids: Vec<i64>, tags: Vec<String>) -> Result<u64, FfiError> {
        guard(|| self.engine.add_tags(&file_ids, &tags)).map(count)
    }

    /// Removes tags.
    pub fn remove_tags(&self, file_ids: Vec<i64>, tags: Vec<String>) -> Result<u64, FfiError> {
        guard(|| self.engine.remove_tags(&file_ids, &tags)).map(count)
    }

    /// All tags with file counts.
    pub fn tags(&self) -> Result<Vec<TagInfo>, FfiError> {
        guard(|| self.engine.tags()).map(|t| {
            t.into_iter()
                .map(|t| TagInfo {
                    name: t.name,
                    files: u64::try_from(t.files).unwrap_or(0),
                })
                .collect()
        })
    }

    /// Overrides BPM / key / category / kind by hand; invalid values throw
    /// `FfiError.InvalidInput` (e.g. BPM `"128"`, key `"Am"`, kind
    /// `"loop"`).
    pub fn set_override(
        &self,
        file_ids: Vec<i64>,
        field: OverrideField,
        value: String,
    ) -> Result<u64, FfiError> {
        guard(|| self.engine.set_override(&file_ids, field.into(), &value)).map(count)
    }

    /// Clears an override; detection shows again.
    pub fn clear_override(
        &self,
        file_ids: Vec<i64>,
        field: OverrideField,
    ) -> Result<u64, FfiError> {
        guard(|| self.engine.clear_override(&file_ids, field.into())).map(count)
    }

    /// Creates a collection and returns its id.
    pub fn create_collection(&self, name: String) -> Result<i64, FfiError> {
        guard(|| self.engine.create_collection(&name))
    }

    /// Renames a collection.
    pub fn rename_collection(&self, id: i64, name: String) -> Result<(), FfiError> {
        guard(|| self.engine.rename_collection(id, &name))
    }

    /// Deletes a collection (not its files); `false` if unknown.
    pub fn delete_collection(&self, id: i64) -> Result<bool, FfiError> {
        guard(|| self.engine.delete_collection(id))
    }

    /// Appends files to a collection.
    pub fn add_to_collection(&self, id: i64, file_ids: Vec<i64>) -> Result<u64, FfiError> {
        guard(|| self.engine.add_to_collection(id, &file_ids)).map(count)
    }

    /// Removes files from a collection.
    pub fn remove_from_collection(&self, id: i64, file_ids: Vec<i64>) -> Result<u64, FfiError> {
        guard(|| self.engine.remove_from_collection(id, &file_ids)).map(count)
    }

    /// All collections.
    pub fn collections(&self) -> Result<Vec<CollectionInfo>, FfiError> {
        guard(|| self.engine.collections()).map(|c| {
            c.into_iter()
                .map(|c| CollectionInfo {
                    id: c.id,
                    name: c.name,
                    files: u64::try_from(c.files).unwrap_or(0),
                })
                .collect()
        })
    }

    /// Rows of a collection in the order they were added.
    pub fn collection_rows(&self, id: i64) -> Result<Vec<FileRow>, FfiError> {
        guard_ffi(|| {
            let ids = guard(|| self.engine.collection_items(id))?;
            let rows = guard(|| self.engine.rows(&ids))?;
            Ok(rows.into_iter().map(FileRow::from).collect())
        })
    }

    // ---- audio ------------------------------------------------------------

    /// `buckets` (1...16384) min/max pairs over the whole file for a
    /// waveform. **Blocking** (decodes the file).
    pub fn waveform_peaks(
        &self,
        file_id: i64,
        buckets: u32,
    ) -> Result<Vec<WaveformPeak>, FfiError> {
        let n = usize::try_from(buckets).unwrap_or(usize::MAX);
        guard(|| self.engine.waveform_peaks(file_id, n)).map(|p| {
            p.into_iter()
                .map(|(min, max)| WaveformPeak { min, max })
                .collect()
        })
    }

    /// The file decoded to mono `Float32` PCM at its own sample rate, for
    /// formats AVFoundation cannot play (Ogg Vorbis). At most 10 minutes.
    /// **Blocking** (decodes the file).
    pub fn preview_pcm(&self, file_id: i64) -> Result<PreviewPcm, FfiError> {
        guard(|| self.engine.preview_pcm(file_id)).map(|p| PreviewPcm {
            sample_rate: p.sample_rate,
            channels: p.channels,
            samples: p.samples,
            truncated: p.truncated,
        })
    }
}

impl SonarEngine {
    fn wrap(engine: Engine) -> Arc<Self> {
        Arc::new(Self { engine })
    }
}

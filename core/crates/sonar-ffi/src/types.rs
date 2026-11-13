//! FFI records and enums (ADR-032). Plain data, no logic: conversions to and
//! from `sonar-core` types are in `convert.rs`. Ids are `Int64` in Swift
//! (`files.id`, `libraries.id`).

/// A registered library root.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LibraryInfo {
    /// Library id.
    pub id: i64,
    /// Absolute root folder.
    pub root_path: String,
    /// False while the folder is missing (e.g. an unmounted drive); its
    /// files stay searchable.
    pub online: bool,
    /// When it was added (unix seconds).
    pub added_at: i64,
}

/// What a scan did to a library.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct ScanInfo {
    /// New files.
    pub added: u64,
    /// Files whose content changed.
    pub changed: u64,
    /// Files that moved (row kept).
    pub moved: u64,
    /// Files newly missing.
    pub removed: u64,
    /// Missing files that came back.
    pub restored: u64,
    /// Unreadable files.
    pub errors: u64,
    /// The root was missing; nothing was scanned.
    pub offline: bool,
}

/// Result of adding a library (the scan has run when this returns).
#[derive(Debug, Clone, uniffi::Record)]
pub struct AddedLibrary {
    /// The library (new, or the existing one for the same folder).
    pub library: LibraryInfo,
    /// What the first scan found.
    pub scan: ScanInfo,
}

/// Indexing stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IndexStage {
    /// Format, duration.
    Meta,
    /// Peak, loudness, kind, tempo, key, category, similarity vectors.
    Dsp,
    /// CLAP embeddings (skipped without model files).
    Embed,
}

/// Polled state of indexing.
#[derive(Debug, Clone, uniffi::Record)]
pub struct IndexingProgress {
    /// Indexing is on: new files found by scans are analyzed automatically.
    pub enabled: bool,
    /// A job is running (possibly paused).
    pub running: bool,
    /// The running job is paused.
    pub paused: bool,
    /// Stage being processed, if a job ran.
    pub stage: Option<IndexStage>,
    /// Pending files when the stage started.
    pub total: u64,
    /// Files handled in the stage so far.
    pub done: u64,
    /// Of those, failed (the file stays searchable by name).
    pub errors: u64,
}

/// Something that happened in the background; see `SonarEngine.pollEvents`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EngineEvent {
    /// A watched library was rescanned; the search index is up to date.
    LibraryScanned {
        /// Library id.
        library_id: i64,
        /// Files added or changed by the scan.
        files_changed: u64,
    },
    /// A library folder became unavailable.
    LibraryOffline {
        /// Library id.
        library_id: i64,
    },
    /// A library folder is available again.
    LibraryOnline {
        /// Library id.
        library_id: i64,
    },
    /// An indexing job started.
    IndexingStarted,
    /// The job ended.
    IndexingFinished {
        /// Stopped by `cancelIndexing`.
        cancelled: bool,
        /// Why the job failed, if it did.
        error: Option<String>,
    },
    /// The search index caught up with this many changed files; re-run the
    /// visible search.
    IndexRefreshed {
        /// Files re-read.
        files: u64,
    },
    /// A background step failed; the engine keeps running.
    Error {
        /// Human-readable cause.
        message: String,
    },
}

/// Which search legs run. Hybrid and semantic fall back to text when the
/// CLAP model or embeddings are missing (`SearchPage.fallback` says why).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchMode {
    /// Text and CLAP fused.
    Hybrid,
    /// Text only.
    Text,
    /// CLAP only.
    Semantic,
}

/// Result order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SortKey {
    /// Best match first (by name when there is no query text).
    Relevance,
    /// File name.
    Name,
    /// Tempo.
    Bpm,
    /// Key.
    Key,
    /// Duration.
    Duration,
    /// Category.
    Category,
    /// Loop before one-shot.
    Kind,
    /// Library.
    Library,
}

/// Filters; every default means "no restriction" except hidden files, which
/// are left out unless `include_hidden`.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct SearchFilters {
    /// Category is one of these (`kick`, `snare`, ...).
    #[uniffi(default = [])]
    pub categories: Vec<String>,
    /// Kind is one of these (`loop`, `one_shot`).
    #[uniffi(default = [])]
    pub kinds: Vec<String>,
    /// Lowest tempo.
    #[uniffi(default = None)]
    pub bpm_min: Option<f64>,
    /// Highest tempo.
    #[uniffi(default = None)]
    pub bpm_max: Option<f64>,
    /// Key is one of these (`Am`, `C#`, Camelot `8A`).
    #[uniffi(default = [])]
    pub keys: Vec<String>,
    /// Also accept harmonically compatible keys.
    #[uniffi(default = false)]
    pub compatible_keys: bool,
    /// Shortest duration (ms).
    #[uniffi(default = None)]
    pub duration_min_ms: Option<u64>,
    /// Longest duration (ms).
    #[uniffi(default = None)]
    pub duration_max_ms: Option<u64>,
    /// Only these libraries.
    #[uniffi(default = [])]
    pub library_ids: Vec<i64>,
    /// Carries all of these tags.
    #[uniffi(default = [])]
    pub tags: Vec<String>,
    /// Only favourites.
    #[uniffi(default = false)]
    pub favorites_only: bool,
    /// Also hidden files.
    #[uniffi(default = false)]
    pub include_hidden: bool,
}

/// One search request. Only `text` is required in Swift:
/// `SearchQuery(text: "kick")`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchQuery {
    /// Query text; empty browses by filters.
    pub text: String,
    /// `nil` = hybrid (text-only when CLAP is unavailable).
    #[uniffi(default = None)]
    pub mode: Option<SearchMode>,
    /// `nil` = no filters (hidden files still left out).
    #[uniffi(default = None)]
    pub filters: Option<SearchFilters>,
    /// `nil` = relevance.
    #[uniffi(default = None)]
    pub sort: Option<SortKey>,
    /// Largest or last first (ignored for relevance).
    #[uniffi(default = false)]
    pub descending: bool,
    /// Hits to skip (paging).
    #[uniffi(default = 0)]
    pub offset: u32,
    /// Hits per page.
    #[uniffi(default = 50)]
    pub limit: u32,
    /// Also match an unfinished last word (as-you-type).
    #[uniffi(default = true)]
    pub prefix: bool,
}

/// Everything a result row shows for one file (effective values: user
/// override, else analysis, else filename prior).
#[derive(Debug, Clone, uniffi::Record)]
pub struct FileRow {
    /// File id.
    pub file_id: i64,
    /// Library id.
    pub library_id: i64,
    /// Absolute path.
    pub path: String,
    /// Name without extension.
    pub name: String,
    /// Library folder and sub-folders, `/`-separated.
    pub folders: String,
    /// Category, if known.
    pub category: Option<String>,
    /// `loop` or `one_shot`, if known.
    pub kind: Option<String>,
    /// Tempo, if known.
    pub bpm: Option<f64>,
    /// Key (`C#m`), if known.
    pub key: Option<String>,
    /// Duration (ms), once indexed.
    pub duration_ms: Option<u64>,
    /// Sample rate (Hz), once indexed.
    pub sample_rate: Option<u32>,
    /// Channels, once indexed.
    pub channels: Option<u16>,
    /// Favourite.
    pub favorite: bool,
    /// Hidden.
    pub hidden: bool,
    /// Tags.
    pub tags: Vec<String>,
}

/// One page of search results.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchPage {
    /// Rows of this page, best first.
    pub rows: Vec<FileRow>,
    /// Files matched by the text leg (all of them, not just this page). In
    /// hybrid or semantic mode semantic hits come on top, so use `hasMore`
    /// to decide whether to fetch another page.
    pub total: u64,
    /// The page is full, so another one may follow.
    pub has_more: bool,
    /// The legs that actually ran.
    pub mode: SearchMode,
    /// Why hybrid/semantic ran as text, if it did.
    pub fallback: Option<String>,
}

/// Which field of a file the user overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OverrideField {
    /// Tempo (plain number 20...400).
    Bpm,
    /// Key (`Am`, `C#`, `8A`).
    Key,
    /// Category id.
    Category,
    /// `loop` or `one_shot`.
    Kind,
}

/// A tag and how many files carry it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TagInfo {
    /// Name as first entered.
    pub name: String,
    /// Files with this tag.
    pub files: u64,
}

/// A collection.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CollectionInfo {
    /// Collection id.
    pub id: i64,
    /// Name.
    pub name: String,
    /// Files in it.
    pub files: u64,
}

/// One bucket of a waveform: the lowest and highest sample.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct WaveformPeak {
    /// Lowest sample in the bucket (-1...1).
    pub min: f32,
    /// Highest sample in the bucket (-1...1).
    pub max: f32,
}

/// Decoded audio for playback of formats AVFoundation cannot read.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PreviewPcm {
    /// Sample rate (Hz), the file's own.
    pub sample_rate: u32,
    /// Channels in `samples` (v1: 1).
    pub channels: u16,
    /// Interleaved `Float32` samples, -1...1.
    pub samples: Vec<f32>,
    /// The file was longer than 10 minutes and cut.
    pub truncated: bool,
}

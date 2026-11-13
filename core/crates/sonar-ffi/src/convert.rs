//! Conversions between FFI types and `sonar-core` types. No product logic.

use sonar_core::engine::{self as core_engine, EngineEvent as CoreEvent, SearchRequest};
use sonar_core::jobs::Stage;
use sonar_core::scan::ScanReport;
use sonar_core::search::keys::parse_key;
use sonar_core::search::{self, Filters, ValueRange};
use sonar_core::store::OverrideField as CoreField;
use sonar_core::watch::WatchEvent;
use sonar_core::Library;

use crate::error::FfiError;
use crate::types::{
    EngineEvent, FileRow, IndexStage, IndexingProgress, LibraryInfo, OverrideField, ScanInfo,
    SearchFilters, SearchMode, SearchQuery, SortKey,
};

/// Count as `u64` (saturating; counts never get near it).
pub fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

impl From<&Library> for LibraryInfo {
    fn from(l: &Library) -> Self {
        Self {
            id: l.id,
            root_path: l.root_path.to_string_lossy().into_owned(),
            online: l.online,
            added_at: l.added_at,
        }
    }
}

impl From<&ScanReport> for ScanInfo {
    fn from(r: &ScanReport) -> Self {
        let c = &r.counts;
        Self {
            added: count(c.added),
            changed: count(c.changed),
            moved: count(c.moved),
            removed: count(c.removed),
            restored: count(c.restored),
            errors: count(c.errors),
            offline: r.offline,
        }
    }
}

impl From<Stage> for IndexStage {
    fn from(s: Stage) -> Self {
        match s {
            Stage::Meta => Self::Meta,
            Stage::Dsp => Self::Dsp,
            Stage::Embed => Self::Embed,
        }
    }
}

/// Progress from the engine's polled status.
pub fn progress(s: &core_engine::IndexingStatus) -> IndexingProgress {
    let p = s.progress.as_ref();
    IndexingProgress {
        enabled: s.enabled,
        running: s.running,
        paused: s.paused,
        stage: p.and_then(|p| p.stage).map(IndexStage::from),
        total: p.map_or(0, |p| p.total),
        done: p.map_or(0, |p| p.done),
        errors: p.map_or(0, |p| p.errors),
    }
}

/// `None` for events the UI has no use for.
pub fn event(e: CoreEvent) -> Option<EngineEvent> {
    Some(match e {
        CoreEvent::Watch(WatchEvent::Scanned {
            library_id,
            summary,
            ..
        }) => EngineEvent::LibraryScanned {
            library_id,
            files_changed: count(summary.added + summary.changed + summary.moved),
        },
        CoreEvent::Watch(WatchEvent::Offline { library_id }) => {
            EngineEvent::LibraryOffline { library_id }
        }
        CoreEvent::Watch(WatchEvent::Online { library_id }) => {
            EngineEvent::LibraryOnline { library_id }
        }
        CoreEvent::Watch(WatchEvent::Error { message, .. }) => EngineEvent::Error { message },
        CoreEvent::IndexingStarted => EngineEvent::IndexingStarted,
        CoreEvent::IndexingFinished { cancelled, error } => {
            EngineEvent::IndexingFinished { cancelled, error }
        }
        CoreEvent::IndexRefreshed { files } => EngineEvent::IndexRefreshed {
            files: count(files),
        },
        CoreEvent::Error { message } => EngineEvent::Error { message },
        _ => return None,
    })
}

impl From<SearchMode> for search::SearchMode {
    fn from(m: SearchMode) -> Self {
        match m {
            SearchMode::Hybrid => Self::Hybrid,
            SearchMode::Text => Self::Text,
            SearchMode::Semantic => Self::Semantic,
        }
    }
}

impl From<search::SearchMode> for SearchMode {
    fn from(m: search::SearchMode) -> Self {
        match m {
            search::SearchMode::Hybrid => Self::Hybrid,
            search::SearchMode::Text => Self::Text,
            search::SearchMode::Semantic => Self::Semantic,
        }
    }
}

impl From<SortKey> for search::SortField {
    fn from(k: SortKey) -> Self {
        match k {
            SortKey::Relevance => Self::Relevance,
            SortKey::Name => Self::Name,
            SortKey::Bpm => Self::Bpm,
            SortKey::Key => Self::Key,
            SortKey::Duration => Self::Duration,
            SortKey::Category => Self::Category,
            SortKey::Kind => Self::Kind,
            SortKey::Library => Self::Library,
        }
    }
}

impl From<OverrideField> for CoreField {
    fn from(f: OverrideField) -> Self {
        match f {
            OverrideField::Bpm => Self::Bpm,
            OverrideField::Key => Self::Key,
            OverrideField::Category => Self::Category,
            OverrideField::Kind => Self::Kind,
        }
    }
}

impl From<core_engine::FileRow> for FileRow {
    fn from(r: core_engine::FileRow) -> Self {
        Self {
            file_id: r.file_id,
            library_id: r.library_id,
            path: r.path.to_string_lossy().into_owned(),
            name: r.name,
            folders: r.folders,
            category: r.category,
            kind: r.kind,
            bpm: r.bpm,
            key: r.key,
            duration_ms: r.duration_ms,
            sample_rate: r.sample_rate,
            channels: r.channels,
            preview_gain_db: r.preview_gain_db,
            favorite: r.favorite,
            hidden: r.hidden,
            tags: r.tags,
        }
    }
}

/// Filters from the FFI record; an unknown key spelling is an error.
pub fn filters(f: Option<SearchFilters>) -> Result<Filters, FfiError> {
    let Some(f) = f else {
        return Ok(Filters::default());
    };
    let keys = f
        .keys
        .iter()
        .map(|k| {
            parse_key(k).ok_or_else(|| FfiError::InvalidInput {
                message: format!("unknown key '{k}'"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    fn range<T>(lo: Option<T>, hi: Option<T>) -> Option<ValueRange<T>> {
        (lo.is_some() || hi.is_some()).then(|| ValueRange::new(lo, hi))
    }
    Ok(Filters {
        categories: f.categories,
        kinds: f.kinds,
        bpm: range(f.bpm_min, f.bpm_max),
        keys,
        compatible_keys: f.compatible_keys,
        duration_ms: range(f.duration_min_ms, f.duration_max_ms),
        libraries: f.library_ids,
        tags: f.tags,
        favorites_only: f.favorites_only,
        include_hidden: f.include_hidden,
    })
}

/// The engine request for an FFI query.
pub fn request(q: SearchQuery) -> Result<SearchRequest, FfiError> {
    Ok(SearchRequest {
        query: q.text,
        mode: q.mode.map_or_else(Default::default, Into::into),
        filters: filters(q.filters)?,
        sort: search::Sort {
            field: q.sort.map_or_else(Default::default, Into::into),
            descending: q.descending,
        },
        offset: usize::try_from(q.offset).unwrap_or(usize::MAX),
        limit: usize::try_from(q.limit).unwrap_or(usize::MAX),
        prefix: q.prefix,
        facets: q.facets,
    })
}

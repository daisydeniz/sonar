//! [`Engine::search`]: text / hybrid / semantic queries with
//! latest-query-wins cancellation.
//!
//! # Latest query wins
//! Every call gets a fresh cancel flag and sets the previous call's flag, so
//! an older query still in flight stops at its next check
//! ([`hybrid_search`] looks before the text leg and per kNN chunk) and
//! returns [`Error::Cancelled`]. A query that finishes after it was
//! cancelled also returns `Cancelled`: a caller never receives results of a
//! superseded query. A Tantivy text query itself is not interruptible (it
//! takes milliseconds); only its result is discarded. The flag is
//! engine-wide: two independent callers searching at once cancel each
//! other, so a host should route one search box through one engine.
//! [`Engine::cancel_search`] cancels the newest query without starting one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::{lock, Engine};
use crate::embed::store::dir_for_db;
use crate::index::vectors::{dir_for_db as vector_dir, VectorStore};
use crate::search::{
    embedded_file_ids, hybrid_search, Filters, HybridOptions, HybridResults, SearchMode,
    SearchOptions, SemanticSearch, Sort, DEFAULT_LIMIT,
};
use crate::similar::{similar, Seed, SimilarOptions, SimilarResults};
use crate::{Error, Result};

/// One query for [`Engine::search`].
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRequest {
    /// Query text (empty = browse by filters).
    pub query: String,
    /// Legs to run; hybrid and semantic fall back to text when CLAP is not
    /// available (see [`HybridResults::fallback`]).
    pub mode: SearchMode,
    /// What to keep.
    pub filters: Filters,
    /// Result order.
    pub sort: Sort,
    /// Hits to skip (paging).
    pub offset: usize,
    /// Hits to return after `offset`.
    pub limit: usize,
    /// As-you-type prefix match of the last word.
    pub prefix: bool,
    /// Also count facets.
    pub facets: bool,
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: SearchMode::default(),
            filters: Filters::default(),
            sort: Sort::default(),
            offset: 0,
            limit: DEFAULT_LIMIT,
            prefix: true,
            facets: false,
        }
    }
}

/// The loaded CLAP query side and whether it must be reloaded.
pub(super) struct SemanticCache {
    /// Set when embeddings changed (an embed batch committed, a job ended);
    /// the next semantic query reloads.
    pub(super) stale: Arc<AtomicBool>,
    loaded: Mutex<Loaded>,
}

impl Default for SemanticCache {
    fn default() -> Self {
        Self {
            stale: Arc::new(AtomicBool::new(true)),
            loaded: Mutex::new(Loaded::Missing("not loaded".into())),
        }
    }
}

impl Engine {
    /// Runs a query (see the module docs for cancellation). Paging is
    /// `offset`/`limit` over the ranked list: the engine asks for
    /// `offset + limit` hits and drops the first `offset`, so
    /// [`HybridHit`](crate::search::HybridHit) ranks stay relative to the
    /// full list. A missing index (opened without
    /// [`super::EngineOptions::rebuild_index`]) is built first.
    ///
    /// # Errors
    /// [`Error::Cancelled`] if a newer search or [`Engine::cancel_search`]
    /// superseded this one, [`Error::Index`], [`Error::Sqlite`].
    pub fn search(&self, req: &SearchRequest) -> Result<HybridResults> {
        let token = self.begin_search();
        self.run_search(req, &token)
    }

    /// Supersedes the previous search and returns the new one's flag.
    pub(super) fn begin_search(&self) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        let mut current = lock(&self.inner.search_cancel);
        current.store(true, Ordering::SeqCst);
        *current = Arc::clone(&token);
        token
    }

    pub(super) fn run_search(
        &self,
        req: &SearchRequest,
        token: &Arc<AtomicBool>,
    ) -> Result<HybridResults> {
        self.ensure_index()?;
        let loaded = if req.mode == SearchMode::Text || req.query.trim().is_empty() {
            None
        } else {
            Some(self.semantic())
        };
        let semantic = match &loaded {
            Some(Loaded::Ready(s)) => Some(Arc::clone(s)),
            _ => None,
        };
        let opts = HybridOptions {
            mode: req.mode,
            search: SearchOptions {
                limit: req.offset.saturating_add(req.limit),
                prefix: req.prefix,
                filters: req.filters.clone(),
                sort: req.sort,
                facets: req.facets,
            },
            cancel: Some(Arc::clone(token)),
        };
        let mut results = hybrid_search(
            &self.inner.index,
            &self.inner.synonyms,
            semantic.as_deref(),
            &req.query,
            &opts,
        )?;
        if token.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        // Say why CLAP did not run (missing model, no embeddings yet), not
        // just that it was unavailable.
        if let Some(Loaded::Missing(why)) = &loaded {
            if results.fallback.as_deref() == Some("semantic search unavailable") {
                results.fallback = Some(why.clone());
            }
        }
        results.hits.drain(..req.offset.min(results.hits.len()));
        Ok(results)
    }

    /// Files that sound like `seed` (ADR-024) among those `opts.filters`
    /// allow, so favourites, hidden files and overrides apply as in
    /// [`Engine::search`]. Opens the aspect vector store read-only per call;
    /// runs on the caller's thread and honours `opts.cancel`.
    ///
    /// # Errors
    /// [`Error::VectorStore`] (no analysis yet), [`Error::NoVectors`], plus
    /// everything [`crate::similar::similar`] returns.
    pub fn similar(&self, seed: &Seed, opts: &SimilarOptions) -> Result<SimilarResults> {
        self.ensure_index()?;
        let vs = VectorStore::open_read(vector_dir(&self.inner.db_path))?;
        similar(&vs, &self.inner.index, seed, opts)
    }

    /// [`Engine::similar`] sharing the search cancel token (latest wins):
    /// a newer `search`/`similar_latest` or [`Engine::cancel_search`]
    /// makes this return [`Error::Cancelled`], also while an external seed
    /// is being decoded. `opts.cancel` is replaced by that token.
    ///
    /// # Errors
    /// As [`Engine::similar`], plus [`Error::Cancelled`].
    pub fn similar_latest(&self, seed: &Seed, opts: &SimilarOptions) -> Result<SimilarResults> {
        let token = self.begin_search();
        let opts = SimilarOptions {
            cancel: Some(Arc::clone(&token)),
            ..opts.clone()
        };
        let results = self.similar(seed, &opts)?;
        if token.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        Ok(results)
    }

    /// Cancels the newest search, if it is still running.
    pub fn cancel_search(&self) {
        lock(&self.inner.search_cancel).store(true, Ordering::SeqCst);
    }

    /// Builds the text index if it was left unbuilt at open.
    fn ensure_index(&self) -> Result<()> {
        if self.inner.index_ready.load(Ordering::SeqCst) {
            return Ok(());
        }
        let store = lock(&self.inner.store);
        // Another thread may have built it while we waited for the store.
        if !self.inner.index_ready.load(Ordering::SeqCst) {
            self.inner.index.rebuild(&store)?;
            self.inner.index_ready.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    /// The CLAP text tower plus current embeddings, loaded on first use and
    /// again after embeddings changed; `None` (text-only) if the model or
    /// embeddings are missing. The failure is remembered until the next
    /// change, so a text-only setup does not touch the disk per query.
    fn semantic(&self) -> Loaded {
        let cache = &self.inner.semantic;
        let mut loaded = lock(&cache.loaded);
        if cache.stale.swap(false, Ordering::SeqCst) {
            *loaded = match self.load_semantic() {
                Ok(s) => Loaded::Ready(Arc::new(s)),
                Err(why) => Loaded::Missing(why),
            };
        }
        loaded.clone()
    }

    /// Whether the semantic leg can run now (model files present and at
    /// least one embedding); loads it if needed.
    pub fn semantic_available(&self) -> bool {
        matches!(self.semantic(), Loaded::Ready(_))
    }

    fn load_semantic(&self) -> std::result::Result<SemanticSearch, String> {
        let opts = &self.inner.options.embed;
        if let Some(reason) = opts.unavailable_reason() {
            return Err(reason);
        }
        let current = embedded_file_ids(&lock(&self.inner.store))
            .map_err(|e| format!("reading embed results: {e}"))?;
        if current.is_empty() {
            return Err(
                "no CLAP embeddings yet (run `sonar analyze --stages meta,dsp,embed`)".into(),
            );
        }
        SemanticSearch::load(opts, &dir_for_db(&self.inner.db_path), Some(&current))
            .map_err(|e| format!("loading CLAP search: {e}"))
    }
}

/// Outcome of loading the CLAP query side.
#[derive(Clone)]
enum Loaded {
    Ready(Arc<SemanticSearch>),
    /// Why it is not available (shown as the search's fallback reason).
    Missing(String),
}

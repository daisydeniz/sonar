//! Search over the [`crate::index`] (docs/ARCHITECTURE.md §6): text
//! (ADR-017: query syntax, synonyms, BM25 with field weights, exclusions,
//! as-you-type prefix matching) plus filters, sort and facet counts
//! (ADR-018).
//!
//! ```text
//! dark kick -vinyl     every word narrows, -word excludes
//! bd                   synonyms: also kick, bassdrum, "bass drum"
//! one shot             multi-word synonym: also oneshot
//! Cmin / c minor / cm  key:cm (keys are recognised in lowercase too)
//! 120 bpm / 120bpm     120bpm
//! kic                  unfinished last word matches as a prefix
//! ```
//!
//! [`Filters`] narrow on effective values (category, kind, BPM range,
//! keys with optional Camelot-compatible expansion, duration range,
//! libraries, tags, favourites; hidden files are left out by default).
//! [`Sort`] orders by relevance or a field (unknown values last). Empty
//! text + filters browses every matching file (by name). [`Facets`] count
//! values over the matching set with multi-select semantics.
//!
//! [`hybrid`] adds natural-language search: a CLAP text-embedding kNN over
//! the audio embeddings ([`semantic`]) fused with this text search by
//! reciprocal-rank fusion (T5.4, ADR-029).

mod allowed;
mod build;
mod collect;
mod facets;
mod filters;
pub mod hybrid;
pub mod keys;
mod parse;
pub mod semantic;
mod sideways;
mod sort;
mod stats;
mod synonyms;

#[cfg(test)]
mod facet_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod tests;

use serde::Serialize;
use tantivy::query::{BooleanQuery, Occur, Query};

pub use allowed::{allowed_file_ids, filtered_file_ids};
pub use facets::{FacetCount, Facets, LibraryCount};
pub use filters::{normalize_kind, Filters, ValueRange};
pub use hybrid::{
    hybrid_search, HybridHit, HybridOptions, HybridResults, QueryEmbedder, SearchMode,
    SemanticSearch,
};
pub use semantic::{embedded_file_ids, SemanticHit, SemanticIndex};
pub use sort::{ParseSortError, Sort, SortField};
pub use synonyms::{Synonyms, BUILTIN_SYNONYMS, SYNONYMS_FILE};

use crate::index::TextIndex;
use crate::Result;

/// Default number of hits.
pub const DEFAULT_LIMIT: usize = 50;

/// How to run a search.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOptions {
    /// Maximum number of hits returned (the total is always counted).
    pub limit: usize,
    /// As-you-type: an unfinished last word (no trailing space) also
    /// matches as a prefix.
    pub prefix: bool,
    /// What to keep.
    pub filters: Filters,
    /// Result order.
    pub sort: Sort,
    /// Also count facets ([`SearchResults::facets`]).
    pub facets: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            prefix: true,
            filters: Filters::default(),
            sort: Sort::default(),
            facets: false,
        }
    }
}

/// One result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Hit {
    /// `files.id`.
    pub file_id: i64,
    /// Relevance score (higher is better; only comparable within a query).
    /// 0 when results are not ordered by relevance (not computed).
    pub score: f32,
}

/// Results of [`search`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResults {
    /// Number of matching documents.
    pub total: usize,
    /// Best hits in the requested order, at most [`SearchOptions::limit`].
    pub hits: Vec<Hit>,
    /// Facet counts, if [`SearchOptions::facets`].
    pub facets: Option<Facets>,
}

/// Runs `query` with `opts` against `index` (see the module docs).
///
/// # Errors
/// [`crate::Error::Index`].
pub fn search(
    index: &TextIndex,
    synonyms: &Synonyms,
    query: &str,
    opts: &SearchOptions,
) -> Result<SearchResults> {
    let fields = index.fields();
    let built = build::build(query, opts.prefix, synonyms, fields)?;
    let searcher = index.searcher();
    let stats = stats::BlendedStats::new(&searcher, &built.blend)?;
    // With facets, the facet dimensions are checked by the collector
    // (drill-sideways); otherwise every filter is part of the query.
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, built.query)];
    clauses.extend(opts.filters.clauses(fields, !opts.facets));
    let query = BooleanQuery::new(clauses);
    let collector = collect::HitCollector {
        order: collect::Order::resolve(opts.sort, built.scored),
        limit: opts.limit,
        facets: opts.facets.then(|| sideways::Sideways::new(&opts.filters)),
    };
    let fruit = searcher.search_with_statistics_provider(&query, &collector, &stats)?;
    let hits = fruit
        .hits
        .into_iter()
        .filter_map(|c| {
            Some(Hit {
                file_id: i64::try_from(c.file_id).ok()?,
                score: c.score,
            })
        })
        .collect();
    Ok(SearchResults {
        total: fruit.total,
        hits,
        facets: fruit.facets.map(facets::RawFacets::finish),
    })
}

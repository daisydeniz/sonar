//! Text search over the [`crate::index`] (docs/ARCHITECTURE.md §6,
//! ADR-017): query syntax, synonyms, BM25 with field weights, exclusions
//! and as-you-type prefix matching. Filters, facets and sort come in T2.3.
//!
//! ```text
//! dark kick -vinyl     every word narrows, -word excludes
//! bd                   synonyms: also kick, bassdrum, "bass drum"
//! one shot             multi-word synonym: also oneshot
//! Cmin / c minor / cm  key:cm (keys are recognised in lowercase too)
//! 120 bpm / 120bpm     120bpm
//! kic                  unfinished last word matches as a prefix
//! ```

mod build;
mod parse;
mod stats;
mod synonyms;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde::Serialize;
use tantivy::collector::{Count, TopDocs};

pub use synonyms::{Synonyms, BUILTIN_SYNONYMS, SYNONYMS_FILE};

use crate::index::{TextIndex, FILE_ID_FIELD};
use crate::Result;

/// Default number of hits.
pub const DEFAULT_LIMIT: usize = 50;

/// How to run a text search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    /// Maximum number of hits returned (the total is always counted).
    pub limit: usize,
    /// As-you-type: an unfinished last word (no trailing space) also
    /// matches as a prefix.
    pub prefix: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            prefix: true,
        }
    }
}

/// One result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Hit {
    /// `files.id`.
    pub file_id: i64,
    /// Relevance score (higher is better; only comparable within a query).
    pub score: f32,
}

/// Results of [`search`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResults {
    /// Number of matching documents.
    pub total: usize,
    /// Best hits, best first, at most [`SearchOptions::limit`].
    pub hits: Vec<Hit>,
}

/// Runs `query` against `index` (see the module docs for the syntax).
///
/// # Errors
/// [`crate::Error::Index`].
pub fn search(
    index: &TextIndex,
    synonyms: &Synonyms,
    query: &str,
    opts: &SearchOptions,
) -> Result<SearchResults> {
    let built = build::build(query, opts.prefix, synonyms, index.fields())?;
    let searcher = index.searcher();
    let stats = stats::BlendedStats::new(&searcher, &built.blend)?;
    let collector = (
        TopDocs::with_limit(opts.limit.max(1)).order_by_score(),
        Count,
    );
    let (top, total) =
        searcher.search_with_statistics_provider(built.query.as_ref(), &collector, &stats)?;
    let mut columns = HashMap::new();
    let mut hits = Vec::with_capacity(top.len());
    for (score, addr) in top.into_iter().take(opts.limit) {
        let column = match columns.entry(addr.segment_ord) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(
                searcher
                    .segment_reader(addr.segment_ord)
                    .fast_fields()
                    .u64(FILE_ID_FIELD)?,
            ),
        };
        if let Some(id) = column
            .first(addr.doc_id)
            .and_then(|v| i64::try_from(v).ok())
        {
            hits.push(Hit { file_id: id, score });
        }
    }
    Ok(SearchResults { total, hits })
}

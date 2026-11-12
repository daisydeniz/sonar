//! The set of files a query may return: every indexed document that passes
//! the [`Filters`] (ADR-018 semantics, hidden files excluded unless
//! [`Filters::include_hidden`]), without text and without scoring. Used by
//! similarity search (ADR-024) and the semantic leg of hybrid search
//! (ADR-029, also minus the query's exclusions) to restrict the
//! brute-force scans.

use tantivy::collector::{Collector, SegmentCollector};
use tantivy::columnar::Column;
use tantivy::query::{AllQuery, BooleanQuery, Occur, Query};
use tantivy::{DocId, Score, SegmentOrdinal, SegmentReader};

use super::{build, Filters, Synonyms};
use crate::index::{TextIndex, FILE_ID_FIELD};
use crate::Result;

/// File ids (`files.id`) of every indexed file matching `filters`, sorted
/// ascending and deduplicated.
///
/// # Errors
/// [`crate::Error::Index`].
pub fn filtered_file_ids(index: &TextIndex, filters: &Filters) -> Result<Vec<i64>> {
    ids_matching(index, filters, Vec::new())
}

/// [`filtered_file_ids`] minus the files matched by the `-word` exclusions
/// of `query` (same semantics as text search, synonyms included); the
/// positive words of `query` are ignored. The candidate set of the
/// semantic leg of hybrid search.
///
/// # Errors
/// [`crate::Error::Index`].
pub fn allowed_file_ids(
    index: &TextIndex,
    synonyms: &Synonyms,
    query: &str,
    filters: &Filters,
) -> Result<Vec<i64>> {
    let excluded = build::exclusions(query, synonyms, index.fields())?;
    ids_matching(index, filters, excluded)
}

fn ids_matching(
    index: &TextIndex,
    filters: &Filters,
    excluded: Vec<Box<dyn Query>>,
) -> Result<Vec<i64>> {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, Box::new(AllQuery))];
    clauses.extend(filters.clauses(index.fields(), true));
    clauses.extend(excluded.into_iter().map(|q| (Occur::MustNot, q)));
    let query = BooleanQuery::new(clauses);
    let raw = index.searcher().search(&query, &IdCollector)?;
    let mut ids: Vec<i64> = raw
        .into_iter()
        .filter_map(|id| i64::try_from(id).ok())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// Collects the `file_id` fast field of every matching document.
struct IdCollector;

impl Collector for IdCollector {
    type Fruit = Vec<u64>;
    type Child = IdSegment;

    fn for_segment(&self, _: SegmentOrdinal, reader: &SegmentReader) -> tantivy::Result<IdSegment> {
        Ok(IdSegment {
            file_id: reader.fast_fields().u64(FILE_ID_FIELD)?,
            ids: Vec::new(),
        })
    }

    fn requires_scoring(&self) -> bool {
        false
    }

    fn merge_fruits(&self, fruits: Vec<Vec<u64>>) -> tantivy::Result<Vec<u64>> {
        Ok(fruits.concat())
    }
}

struct IdSegment {
    file_id: Column<u64>,
    ids: Vec<u64>,
}

impl SegmentCollector for IdSegment {
    type Fruit = Vec<u64>;

    fn collect(&mut self, doc: DocId, _: Score) {
        if let Some(id) = self.file_id.first(doc) {
            self.ids.push(id);
        }
    }

    fn harvest(self) -> Vec<u64> {
        self.ids
    }
}

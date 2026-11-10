//! BM25 statistics with blended document frequencies for alternatives.
//!
//! The alternatives at one query position (`kick` / `bd` / `bassdrum`,
//! `am` / `key:am`, `kicks` / `kick`) are scored as the same concept, so
//! they share one IDF: the one of the most frequent alternative, per field.
//! Without this a rare spelling (`bd`) gets a high IDF and outranks the
//! word the user actually typed. Same idea as Lucene's `SynonymQuery` /
//! Elasticsearch blended term frequencies.

use std::collections::HashMap;

use tantivy::query::Bm25StatisticsProvider;
use tantivy::schema::Field;
use tantivy::{Searcher, Term};

/// [`Bm25StatisticsProvider`] that overrides `doc_freq` of blended terms.
pub(crate) struct BlendedStats<'a> {
    searcher: &'a Searcher,
    doc_freq: HashMap<Term, u64>,
}

impl<'a> BlendedStats<'a> {
    /// Stats where every term of a group gets the group's highest
    /// document frequency (a term in several groups keeps the highest).
    pub(crate) fn new(searcher: &'a Searcher, groups: &[Vec<Term>]) -> tantivy::Result<Self> {
        let mut doc_freq: HashMap<Term, u64> = HashMap::new();
        for group in groups.iter().filter(|g| g.len() > 1) {
            let mut max = 0;
            for t in group {
                max = max.max(searcher.doc_freq(t)?);
            }
            for t in group {
                let e = doc_freq.entry(t.clone()).or_insert(0);
                *e = (*e).max(max);
            }
        }
        Ok(Self { searcher, doc_freq })
    }
}

impl Bm25StatisticsProvider for BlendedStats<'_> {
    fn total_num_tokens(&self, field: Field) -> tantivy::Result<u64> {
        self.searcher.total_num_tokens(field)
    }

    fn total_num_docs(&self) -> tantivy::Result<u64> {
        self.searcher.total_num_docs()
    }

    fn doc_freq(&self, term: &Term) -> tantivy::Result<u64> {
        match self.doc_freq.get(term) {
            Some(&df) => Ok(df),
            None => self.searcher.doc_freq(term),
        }
    }
}

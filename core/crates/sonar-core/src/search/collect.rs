//! One pass over the matching documents: total, top hits in the requested
//! order and (optionally) drill-sideways facet counts.
//!
//! Without facets the query already holds every filter and each matching
//! doc is a hit; with facets [`SegFacets`] decides which docs are hits
//! (drill-sideways, see [`super::sideways`]).
//!
//! Top-k per segment keeps raw values (string ordinals compare like the
//! strings within a segment); only the k winners are turned into strings
//! before segments are merged.

use std::cmp::Ordering;

use tantivy::collector::{Collector, SegmentCollector};
use tantivy::columnar::{Column, StrColumn};
use tantivy::{DocId, Score, SegmentOrdinal, SegmentReader};

use super::facets::RawFacets;
use super::sideways::{SegFacets, Sideways};
use super::sort::{Sort, SortField};
use crate::index::{ff, FILE_ID_FIELD};

/// How hits are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    /// BM25 score, best first.
    Score,
    /// A fast field; unknown values last.
    Field {
        /// Column name ([`ff`]).
        column: &'static str,
        /// String column (else u64).
        text: bool,
        /// Largest first.
        desc: bool,
    },
}

impl Order {
    /// The order for `sort`; relevance without scored text is by name.
    pub(crate) fn resolve(sort: Sort, scored: bool) -> Self {
        let (column, text) = match sort.field {
            SortField::Relevance if scored => return Self::Score,
            SortField::Relevance => {
                return Self::Field {
                    column: ff::NAME,
                    text: true,
                    desc: false,
                }
            }
            SortField::Name => (ff::NAME, true),
            SortField::Category => (ff::CATEGORY, true),
            SortField::Kind => (ff::KIND, true),
            SortField::Bpm => (ff::BPM, false),
            SortField::Key => (ff::KEY, false),
            SortField::Duration => (ff::DURATION, false),
            SortField::Library => (ff::LIBRARY, false),
        };
        Self::Field {
            column,
            text,
            desc: sort.descending,
        }
    }
}

/// The collector: see the module docs.
pub(crate) struct HitCollector {
    pub order: Order,
    pub limit: usize,
    /// `Some` = count facets (drill-sideways).
    pub facets: Option<Sideways>,
}

/// Sort value of a candidate after its segment is done.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Missing,
    Num(u64),
    Str(String),
}

/// One candidate hit.
#[derive(Debug, Clone)]
pub(crate) struct Cand<V> {
    pub value: V,
    pub score: Score,
    pub file_id: u64,
}

/// What one segment (and, merged, the whole search) produced.
pub(crate) struct Fruit {
    pub total: usize,
    pub hits: Vec<Cand<Value>>,
    pub facets: Option<RawFacets>,
}

impl Collector for HitCollector {
    type Fruit = Fruit;
    type Child = SegCollector;

    fn for_segment(
        &self,
        _: SegmentOrdinal,
        reader: &SegmentReader,
    ) -> tantivy::Result<SegCollector> {
        let fast = reader.fast_fields();
        let sort_col = match self.order {
            Order::Score => SortCol::Score,
            Order::Field {
                column, text: true, ..
            } => SortCol::Str(fast.str(column)?),
            Order::Field { column, .. } => SortCol::Num(fast.column_opt::<u64>(column)?),
        };
        let facets = match &self.facets {
            Some(s) => Some(SegFacets::new(reader, s)?),
            None => None,
        };
        Ok(SegCollector {
            file_id: fast.u64(FILE_ID_FIELD)?,
            sort_col,
            top: TopK::new(self.limit, self.order),
            total: 0,
            facets,
        })
    }

    fn requires_scoring(&self) -> bool {
        self.order == Order::Score
    }

    fn merge_fruits(&self, fruits: Vec<Fruit>) -> tantivy::Result<Fruit> {
        let mut total = 0;
        let mut hits = Vec::new();
        let mut facets = self.facets.as_ref().map(|_| RawFacets::default());
        for f in fruits {
            total += f.total;
            hits.extend(f.hits);
            if let (Some(all), Some(seg)) = (facets.as_mut(), f.facets) {
                all.merge(seg);
            }
        }
        hits.sort_by(|a, b| compare(self.order, a, b));
        hits.truncate(self.limit);
        Ok(Fruit {
            total,
            hits,
            facets,
        })
    }
}

/// Sort column of one segment.
enum SortCol {
    Score,
    Num(Option<Column<u64>>),
    Str(Option<StrColumn>),
}

impl SortCol {
    fn value(&self, doc: DocId) -> Option<u64> {
        match self {
            Self::Score => None,
            Self::Num(c) => c.as_ref().and_then(|c| c.first(doc)),
            Self::Str(c) => c.as_ref().and_then(|c| c.ords().first(doc)),
        }
    }

    /// Segment-independent value of a raw one.
    fn resolve(&self, raw: Option<u64>) -> Value {
        match (self, raw) {
            (_, None) | (Self::Score, _) => Value::Missing,
            (Self::Num(_), Some(v)) => Value::Num(v),
            (Self::Str(c), Some(ord)) => {
                let mut s = String::new();
                match c.as_ref().map(|c| c.ord_to_str(ord, &mut s)) {
                    Some(Ok(true)) => Value::Str(s),
                    _ => Value::Missing,
                }
            }
        }
    }
}

/// Per-segment state.
pub(crate) struct SegCollector {
    file_id: Column<u64>,
    sort_col: SortCol,
    top: TopK,
    total: usize,
    facets: Option<SegFacets>,
}

impl SegmentCollector for SegCollector {
    type Fruit = Fruit;

    fn collect(&mut self, doc: DocId, score: Score) {
        if let Some(f) = self.facets.as_mut() {
            if !f.collect(doc) {
                return;
            }
        }
        self.total += 1;
        if self.top.limit == 0 {
            return;
        }
        if let Some(file_id) = self.file_id.first(doc) {
            let value = self.sort_col.value(doc);
            self.top.push(Cand {
                value,
                score,
                file_id,
            });
        }
    }

    fn harvest(self) -> Fruit {
        let hits = self
            .top
            .into_sorted()
            .into_iter()
            .map(|c| Cand {
                value: self.sort_col.resolve(c.value),
                score: c.score,
                file_id: c.file_id,
            })
            .collect();
        Fruit {
            total: self.total,
            hits,
            facets: self.facets.map(SegFacets::harvest),
        }
    }
}

/// A sort value that may be unknown.
trait SortValue {
    /// Unknown value.
    fn missing(&self) -> bool;
    /// Order of two known values.
    fn cmp_known(&self, other: &Self) -> Ordering;
}

/// Raw per-segment value (ordinal or number).
impl SortValue for Option<u64> {
    fn missing(&self) -> bool {
        self.is_none()
    }
    fn cmp_known(&self, other: &Self) -> Ordering {
        self.cmp(other)
    }
}

impl SortValue for Value {
    fn missing(&self) -> bool {
        *self == Value::Missing
    }
    fn cmp_known(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Value::Num(x), Value::Num(y)) => x.cmp(y),
            (Value::Str(x), Value::Str(y)) => x.cmp(y),
            // Never mixed within one search.
            _ => Ordering::Equal,
        }
    }
}

/// `a` before `b`? Score: higher first. Field: known values first (in
/// both directions), then by value in the requested direction. Ties:
/// lower file id first.
fn compare<V: SortValue>(order: Order, a: &Cand<V>, b: &Cand<V>) -> Ordering {
    let primary = match order {
        Order::Score => b.score.total_cmp(&a.score),
        Order::Field { desc, .. } => match (a.value.missing(), b.value.missing()) {
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            (true, true) => Ordering::Equal,
            (false, false) if desc => b.value.cmp_known(&a.value),
            (false, false) => a.value.cmp_known(&b.value),
        },
    };
    primary.then(a.file_id.cmp(&b.file_id))
}

/// Bounded top-k: buffer up to 2k, then keep the best k (amortised
/// O(n) selection, like Tantivy's `TopNComputer`).
struct TopK {
    limit: usize,
    order: Order,
    buf: Vec<Cand<Option<u64>>>,
}

impl TopK {
    fn new(limit: usize, order: Order) -> Self {
        Self {
            limit,
            order,
            buf: Vec::new(),
        }
    }

    fn push(&mut self, c: Cand<Option<u64>>) {
        self.buf.push(c);
        if self.buf.len() >= self.limit.saturating_mul(2).max(64) {
            self.truncate();
        }
    }

    fn truncate(&mut self) {
        if self.buf.len() > self.limit {
            let order = self.order;
            self.buf
                .select_nth_unstable_by(self.limit, |a, b| compare(order, a, b));
            self.buf.truncate(self.limit);
        }
    }

    fn into_sorted(mut self) -> Vec<Cand<Option<u64>>> {
        self.truncate();
        let order = self.order;
        self.buf.sort_by(|a, b| compare(order, a, b));
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(value: Option<u64>, file_id: u64) -> Cand<Option<u64>> {
        Cand {
            value,
            score: 0.0,
            file_id,
        }
    }

    #[test]
    fn unknown_values_sort_last_both_ways() {
        let field = |desc| Order::Field {
            column: ff::BPM,
            text: false,
            desc,
        };
        for desc in [false, true] {
            let mut top = TopK::new(10, field(desc));
            for c in [
                cand(None, 1),
                cand(Some(5), 2),
                cand(Some(9), 3),
                cand(None, 0),
            ] {
                top.push(c);
            }
            let ids: Vec<u64> = top.into_sorted().iter().map(|c| c.file_id).collect();
            let want: [u64; 4] = if desc { [3, 2, 0, 1] } else { [2, 3, 0, 1] };
            assert_eq!(ids, want, "desc {desc}");
        }
    }

    #[test]
    fn top_k_keeps_the_best() {
        let mut top = TopK::new(3, Order::Score);
        for i in 0..1000u64 {
            top.push(Cand {
                value: None,
                score: (i % 97) as f32,
                file_id: i,
            });
        }
        let ids: Vec<u64> = top.into_sorted().iter().map(|c| c.file_id).collect();
        assert_eq!(ids, [96, 193, 290]);
    }
}

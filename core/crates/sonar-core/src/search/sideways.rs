//! Drill-sideways facet counting for one segment (ADR-018, see
//! [`super::facets`] for the semantics).
//!
//! With facets the query leaves out the facet dimensions' filters
//! (category, kind, key, library, favourites) and they are checked here
//! from fast fields: a doc passing all of them is a hit and counts in every
//! dimension; a doc failing exactly one dimension counts only in that
//! dimension (its value is what selecting it would add); a doc failing two
//! or more is ignored. One pass, exact multi-select counts (the Lucene
//! `DrillSideways` idea). Tags are counted on hits only.

use std::collections::HashMap;

use tantivy::columnar::{Column, StrColumn};
use tantivy::{DocId, SegmentReader};

use super::facets::RawFacets;
use super::filters::Filters;
use crate::index::ff;

/// Drill-sideways dimensions: category, kind, key, library, favourite
/// (in this order in [`SegFacets`]).
const DIMS: usize = 5;

/// The filter values of the drill-sideways dimensions (sorted; `None` =
/// dimension not filtered).
#[derive(Debug, Clone, Default)]
pub(crate) struct Sideways {
    categories: Option<Vec<String>>,
    kinds: Option<Vec<String>>,
    keys: Option<Vec<u64>>,
    libraries: Option<Vec<u64>>,
    favorites_only: bool,
}

impl Sideways {
    pub(crate) fn new(f: &Filters) -> Self {
        fn some<T>(v: Vec<T>) -> Option<Vec<T>> {
            (!v.is_empty()).then_some(v)
        }
        Self {
            categories: some(f.category_values()),
            kinds: some(f.kind_values()),
            keys: some(f.key_codes()),
            libraries: some(f.library_values()),
            favorites_only: f.favorites_only,
        }
    }
}

/// Counts per value of one dimension: dense for small value ranges
/// (string ordinals, keys, flags, typical library ids), else a map.
enum Counter {
    Dense(Vec<usize>),
    Sparse(HashMap<u64, usize>),
}

/// Largest value range counted in a dense vector.
const DENSE_MAX: u64 = 1 << 16;

impl Counter {
    fn new(bound: u64) -> Self {
        match usize::try_from(bound) {
            Ok(n) if bound <= DENSE_MAX => Self::Dense(vec![0; n]),
            _ => Self::Sparse(Default::default()),
        }
    }

    fn add(&mut self, v: u64) {
        match self {
            Self::Dense(c) => {
                if let Some(n) = usize::try_from(v).ok().and_then(|i| c.get_mut(i)) {
                    *n += 1;
                }
            }
            Self::Sparse(m) => *m.entry(v).or_default() += 1,
        }
    }

    fn into_counts(self) -> Vec<(u64, usize)> {
        match self {
            Self::Dense(c) => (0u64..).zip(c).filter(|&(_, n)| n > 0).collect(),
            Self::Sparse(m) => m.into_iter().collect(),
        }
    }
}

/// One drill-sideways dimension in one segment.
struct Dim {
    /// Values (string ordinals for string dimensions); `None` = no doc of
    /// the segment has a value.
    values: Option<Column<u64>>,
    /// Accepted values, sorted; `None` = not filtered.
    allowed: Option<Vec<u64>>,
    counts: Counter,
}

impl Dim {
    fn num(col: Option<Column<u64>>, allowed: Option<&Vec<u64>>) -> Self {
        let bound = col.as_ref().map_or(0, |c| c.max_value().saturating_add(1));
        Self {
            counts: Counter::new(bound),
            values: col,
            allowed: allowed.cloned(),
        }
    }

    /// A string dimension: filter values are mapped to this segment's
    /// ordinals (values absent from the segment accept nothing).
    fn text(col: Option<&StrColumn>, allowed: Option<&Vec<String>>) -> tantivy::Result<Self> {
        let allowed = match (allowed, col) {
            (None, _) => None,
            (Some(_), None) => Some(Vec::new()),
            (Some(vals), Some(c)) => {
                let mut ords = Vec::new();
                for v in vals {
                    if let Some(o) = c.dictionary().term_ord(v.as_bytes())? {
                        ords.push(o);
                    }
                }
                ords.sort_unstable();
                Some(ords)
            }
        };
        let bound = col.map_or(0, |c| c.num_terms() as u64);
        Ok(Self {
            values: col.map(|c| c.ords().clone()),
            allowed,
            counts: Counter::new(bound),
        })
    }
}

/// Facet state of one segment.
pub(crate) struct SegFacets {
    dims: [Dim; DIMS],
    categories: Option<StrColumn>,
    kinds: Option<StrColumn>,
    tags: Option<StrColumn>,
    tag_counts: Counter,
    /// Scratch: each dimension's value of the current doc.
    vals: [Option<u64>; DIMS],
}

impl SegFacets {
    pub(crate) fn new(reader: &SegmentReader, s: &Sideways) -> tantivy::Result<Self> {
        let fast = reader.fast_fields();
        let categories = fast.str(ff::CATEGORY)?;
        let kinds = fast.str(ff::KIND)?;
        let tags = fast.str(ff::TAGS)?;
        let fav_allowed = s.favorites_only.then(|| vec![1]);
        let dims = [
            Dim::text(categories.as_ref(), s.categories.as_ref())?,
            Dim::text(kinds.as_ref(), s.kinds.as_ref())?,
            Dim::num(fast.column_opt(ff::KEY)?, s.keys.as_ref()),
            Dim::num(fast.column_opt(ff::LIBRARY)?, s.libraries.as_ref()),
            Dim::num(fast.column_opt(ff::FAVORITE)?, fav_allowed.as_ref()),
        ];
        let tag_counts = Counter::new(tags.as_ref().map_or(0, |c| c.num_terms() as u64));
        Ok(Self {
            dims,
            categories,
            kinds,
            tags,
            tag_counts,
            vals: [None; DIMS],
        })
    }

    /// Counts `doc`; `true` if it passes every dimension (a hit).
    pub(crate) fn collect(&mut self, doc: DocId) -> bool {
        let mut failed = None;
        for (i, d) in self.dims.iter().enumerate() {
            let v = d.values.as_ref().and_then(|c| c.first(doc));
            self.vals[i] = v;
            let pass = match &d.allowed {
                None => true,
                Some(ok) => v.is_some_and(|v| ok.binary_search(&v).is_ok()),
            };
            if !pass {
                if failed.is_some() {
                    return false; // two dimensions fail: counts nowhere
                }
                failed = Some(i);
            }
        }
        if let Some(i) = failed {
            if let Some(v) = self.vals[i] {
                self.dims[i].counts.add(v);
            }
            return false;
        }
        for (d, v) in self.dims.iter_mut().zip(self.vals) {
            if let Some(v) = v {
                d.counts.add(v);
            }
        }
        if let Some(tags) = &self.tags {
            for ord in tags.term_ords(doc) {
                self.tag_counts.add(ord);
            }
        }
        true
    }

    pub(crate) fn harvest(self) -> RawFacets {
        let [cat, kind, key, lib, fav] = self.dims;
        let to_map = |c: Counter| c.into_counts().into_iter().collect();
        RawFacets {
            categories: labels(self.categories.as_ref(), cat.counts),
            kinds: labels(self.kinds.as_ref(), kind.counts),
            keys: to_map(key.counts),
            libraries: to_map(lib.counts),
            tags: labels(self.tags.as_ref(), self.tag_counts),
            favorites: fav
                .counts
                .into_counts()
                .into_iter()
                .filter(|&(v, _)| v == 1)
                .map(|(_, n)| n)
                .sum(),
        }
    }
}

/// Ordinal counts → string counts.
fn labels(col: Option<&StrColumn>, counts: Counter) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    let Some(col) = col else {
        return out;
    };
    for (ord, n) in counts.into_counts() {
        let mut s = String::new();
        if let Ok(true) = col.ord_to_str(ord, &mut s) {
            *out.entry(s).or_default() += n;
        }
    }
    out
}

//! Facet counts (ADR-018) for a filter-chip UI.
//!
//! Counts are over the files matching the query text, the range filters
//! (BPM, duration), the tags and the hidden rule. For the any-of
//! dimensions — category, kind, key, library — and the favourites toggle,
//! a dimension's own filter is ignored when counting that dimension, so
//! the chips of a selected dimension still show what selecting another
//! value would add ("multi-select" / drill-sideways faceting, as in
//! Lucene's `DrillSideways` and Elasticsearch `post_filter`). Tags are
//! all-of: they are counted on the final result (selecting a tag narrows
//! to exactly its count). BPM and duration are not faceted: they are range
//! sliders, not chips.
//!
//! Values are sorted by count (desc), then value; unknown values are not
//! counted.

use std::collections::HashMap;

use serde::Serialize;

use super::keys::key_label;

/// One facet value and how many matching files have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FacetCount {
    /// Folded value (`kick`, `one_shot`, `dark techno`) or key label
    /// (`Am`, `C#`).
    pub value: String,
    /// Matching files with this value.
    pub count: usize,
}

/// Matching files per library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryCount {
    /// `libraries.id`.
    pub library_id: i64,
    /// Matching files in it.
    pub count: usize,
}

/// Facet counts of one search (see the module docs for what is counted).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Facets {
    /// Per effective category.
    pub categories: Vec<FacetCount>,
    /// Per effective kind.
    pub kinds: Vec<FacetCount>,
    /// Per effective key (labels from [`key_label`]).
    pub keys: Vec<FacetCount>,
    /// Per library.
    pub libraries: Vec<LibraryCount>,
    /// Per tag, within the final result.
    pub tags: Vec<FacetCount>,
    /// Favourites among the files matching every other filter.
    pub favorites: usize,
}

/// Counts as collected, before sorting; summed over segments.
#[derive(Debug, Default)]
pub(crate) struct RawFacets {
    pub categories: HashMap<String, usize>,
    pub kinds: HashMap<String, usize>,
    pub keys: HashMap<u64, usize>,
    pub libraries: HashMap<u64, usize>,
    pub tags: HashMap<String, usize>,
    pub favorites: usize,
}

impl RawFacets {
    /// Adds the counts of `other`.
    pub(crate) fn merge(&mut self, other: Self) {
        fn add<K: std::hash::Hash + Eq>(a: &mut HashMap<K, usize>, b: HashMap<K, usize>) {
            for (k, n) in b {
                *a.entry(k).or_default() += n;
            }
        }
        add(&mut self.categories, other.categories);
        add(&mut self.kinds, other.kinds);
        add(&mut self.keys, other.keys);
        add(&mut self.libraries, other.libraries);
        add(&mut self.tags, other.tags);
        self.favorites += other.favorites;
    }

    /// Sorted public counts.
    pub(crate) fn finish(self) -> Facets {
        let keys = self
            .keys
            .into_iter()
            .filter_map(|(code, n)| Some((key_label(u8::try_from(code).ok()?), n)))
            .collect();
        let mut libraries: Vec<LibraryCount> = self
            .libraries
            .into_iter()
            .filter_map(|(id, count)| {
                Some(LibraryCount {
                    library_id: i64::try_from(id).ok()?,
                    count,
                })
            })
            .collect();
        libraries.sort_by(|a, b| b.count.cmp(&a.count).then(a.library_id.cmp(&b.library_id)));
        Facets {
            categories: sorted(self.categories),
            kinds: sorted(self.kinds),
            keys: sorted(keys),
            libraries,
            tags: sorted(self.tags),
            favorites: self.favorites,
        }
    }
}

fn sorted(counts: HashMap<String, usize>) -> Vec<FacetCount> {
    let mut v: Vec<FacetCount> = counts
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(value, count)| FacetCount { value, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_and_sort() {
        let mut a = RawFacets::default();
        a.categories.insert("kick".into(), 2);
        a.categories.insert("snare".into(), 3);
        a.keys.insert(19, 1);
        a.libraries.insert(2, 1);
        a.favorites = 1;
        let mut b = RawFacets::default();
        b.categories.insert("kick".into(), 2);
        b.categories.insert("bass".into(), 4);
        b.keys.insert(19, 1);
        b.libraries.insert(1, 1);
        b.favorites = 2;
        a.merge(b);
        let f = a.finish();
        let cats: Vec<(&str, usize)> = f
            .categories
            .iter()
            .map(|c| (c.value.as_str(), c.count))
            .collect();
        assert_eq!(cats, [("bass", 4), ("kick", 4), ("snare", 3)]);
        assert_eq!(f.keys[0].value, "Am");
        assert_eq!(f.keys[0].count, 2);
        let libs: Vec<i64> = f.libraries.iter().map(|l| l.library_id).collect();
        assert_eq!(libs, [1, 2]);
        assert_eq!(f.favorites, 3);
    }
}

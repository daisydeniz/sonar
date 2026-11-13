//! Facet counts for filter chips (ADR-018, ADR-033) and their conversions.

use sonar_core::search::{FacetCount as CoreCount, Facets, LibraryCount};

use crate::convert::count;

/// One facet value and how many matching files have it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FacetCount {
    /// Folded value (`kick`, `one_shot`, `dark techno`) or key label
    /// (`Am`, `C#`).
    pub value: String,
    /// Matching files with this value.
    pub count: u64,
}

/// Matching files per library.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LibraryFacet {
    /// Library id.
    pub library_id: i64,
    /// Matching files in it.
    pub count: u64,
}

/// Facet counts of one search, over the files matching the query text, the
/// BPM/duration ranges, the tags and the hidden rule. The any-of dimensions
/// (categories, kinds, keys, libraries) and `favorites` ignore their own
/// filter ("drill-sideways"), so chips of a selected dimension still show
/// what selecting another value would add. `tags` are counted on the final
/// result (all-of). Every list is sorted by count (desc), then value.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct SearchFacets {
    /// Per effective category.
    pub categories: Vec<FacetCount>,
    /// Per effective kind (`loop`, `one_shot`).
    pub kinds: Vec<FacetCount>,
    /// Per effective key label.
    pub keys: Vec<FacetCount>,
    /// Per library.
    pub libraries: Vec<LibraryFacet>,
    /// Per tag, within the final result.
    pub tags: Vec<FacetCount>,
    /// Favourites among the files matching every other filter.
    pub favorites: u64,
}

impl From<CoreCount> for FacetCount {
    fn from(c: CoreCount) -> Self {
        Self {
            value: c.value,
            count: count(c.count),
        }
    }
}

impl From<LibraryCount> for LibraryFacet {
    fn from(c: LibraryCount) -> Self {
        Self {
            library_id: c.library_id,
            count: count(c.count),
        }
    }
}

impl From<Facets> for SearchFacets {
    fn from(f: Facets) -> Self {
        let counts = |v: Vec<CoreCount>| v.into_iter().map(FacetCount::from).collect();
        Self {
            categories: counts(f.categories),
            kinds: counts(f.kinds),
            keys: counts(f.keys),
            libraries: f.libraries.into_iter().map(LibraryFacet::from).collect(),
            tags: counts(f.tags),
            favorites: count(f.favorites),
        }
    }
}

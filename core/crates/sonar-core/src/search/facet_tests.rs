//! Facet counts (drill-sideways semantics, ADR-018) on the corpus of
//! [`super::filter_tests`].

use std::collections::BTreeSet;

use super::filter_tests::{bpm, corpus, filtered, ids, keys, run, strs};
use super::*;

fn counts(f: &[FacetCount]) -> Vec<(&str, usize)> {
    f.iter().map(|c| (c.value.as_str(), c.count)).collect()
}

fn facets(ix: &TextIndex, q: &str, f: Filters) -> (BTreeSet<i64>, Facets) {
    let r = run(ix, q, filtered(f));
    let hits = r.hits.iter().map(|h| h.file_id).collect();
    (hits, r.facets.expect("facets"))
}

#[test]
fn facet_counts_without_filters() {
    let ix = corpus();
    let (_, f) = facets(&ix, "", Filters::default());
    assert_eq!(
        counts(&f.categories),
        [
            ("bass", 2),
            ("kick", 2),
            ("keys", 1),
            ("pad", 1),
            ("synth", 1)
        ]
    );
    assert_eq!(counts(&f.kinds), [("loop", 4), ("one_shot", 3)]);
    assert_eq!(
        counts(&f.keys),
        [
            ("Am", 1),
            ("C", 1),
            ("Dm", 1),
            ("Em", 1),
            ("F#m", 1),
            ("G", 1)
        ]
    );
    let libs: Vec<(i64, usize)> = f
        .libraries
        .iter()
        .map(|l| (l.library_id, l.count))
        .collect();
    assert_eq!(libs, [(1, 4), (2, 4)]);
    assert_eq!(
        counts(&f.tags),
        [("techno", 3), ("dark", 2), ("ambient", 1)]
    );
    assert_eq!(f.favorites, 3);
    // Text restricts the counted set.
    let (_, f) = facets(&ix, "kick", Filters::default());
    assert_eq!(counts(&f.categories), [("kick", 2)]);
    assert_eq!(f.favorites, 1);
}

#[test]
fn facets_ignore_their_own_any_of_filter() {
    let ix = corpus();
    let kick = Filters {
        categories: strs(&["kick"]),
        ..Filters::default()
    };
    let (hits, f) = facets(&ix, "", kick.clone());
    assert_eq!(hits, ids(&[1, 2]));
    // Category chips still show every category …
    assert_eq!(
        counts(&f.categories),
        [
            ("bass", 2),
            ("kick", 2),
            ("keys", 1),
            ("pad", 1),
            ("synth", 1)
        ]
    );
    // … the other dimensions count the kick result.
    assert_eq!(counts(&f.kinds), [("one_shot", 2)]);
    assert_eq!(counts(&f.tags), [("dark", 1)]);
    assert_eq!(f.favorites, 1);
    assert_eq!(f.libraries.len(), 1);

    let fav_kick = Filters {
        favorites_only: true,
        ..kick
    };
    let (hits, f) = facets(&ix, "", fav_kick);
    assert_eq!(hits, ids(&[1]));
    // Categories among favourites; favourites among kicks.
    assert_eq!(
        counts(&f.categories),
        [("bass", 1), ("kick", 1), ("pad", 1)]
    );
    assert_eq!(f.favorites, 1);
    assert_eq!(counts(&f.kinds), [("one_shot", 1)]);

    let am = Filters {
        keys: keys(&["Am"]),
        compatible_keys: true,
        ..Filters::default()
    };
    let (hits, f) = facets(&ix, "", am);
    assert_eq!(hits, ids(&[3, 4, 5, 8]));
    assert_eq!(f.keys.len(), 6, "key chips ignore the key filter");
    assert_eq!(counts(&f.kinds), [("loop", 2), ("one_shot", 1)]);
}

#[test]
fn tag_facets_count_the_final_result() {
    let ix = corpus();
    let techno = Filters {
        tags: strs(&["techno"]),
        ..Filters::default()
    };
    let (hits, f) = facets(&ix, "", techno);
    assert_eq!(hits, ids(&[3, 4, 9]));
    assert_eq!(counts(&f.tags), [("techno", 3), ("dark", 1)]);
    assert_eq!(counts(&f.categories), [("bass", 2), ("synth", 1)]);
    // Ranges always apply to the counts.
    let (_, f) = facets(&ix, "", bpm(Some(125.0), None));
    assert_eq!(counts(&f.kinds), [("loop", 2)]);
    assert_eq!(f.categories.len(), 2, "bass, synth (8 has none)");
}

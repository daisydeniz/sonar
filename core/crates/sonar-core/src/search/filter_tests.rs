//! Filters, sort and facets on a small in-RAM index (two segments, so
//! cross-segment merging of string sort values and counts is exercised).

use std::collections::BTreeSet;
use std::path::Path;

use super::keys::parse_key;
use super::*;
use crate::index::IndexDoc;

struct Row {
    id: i64,
    name: &'static str,
    cat: Option<&'static str>,
    kind: Option<&'static str>,
    bpm: Option<f64>,
    key: Option<&'static str>,
    dur: Option<u64>,
    lib: i64,
    tags: &'static [&'static str],
    fav: bool,
    hidden: bool,
}

const fn row(id: i64, name: &'static str, cat: Option<&'static str>, lib: i64) -> Row {
    Row {
        id,
        name,
        cat,
        kind: None,
        bpm: None,
        key: None,
        dur: None,
        lib,
        tags: &[],
        fav: false,
        hidden: false,
    }
}

/// Docs 1–5 go into the first segment, 6–9 into the second.
const CORPUS: [Row; 9] = [
    Row {
        kind: Some("one_shot"),
        dur: Some(300),
        tags: &["Dark"],
        fav: true,
        ..row(1, "Kick Hard", Some("kick"), 1)
    },
    Row {
        kind: Some("one_shot"),
        dur: Some(500),
        ..row(2, "Kick Soft", Some("Kick"), 1)
    },
    Row {
        kind: Some("loop"),
        bpm: Some(128.0),
        key: Some("Am"),
        dur: Some(8000),
        tags: &["dark", "techno"],
        fav: true,
        ..row(3, "Bass Loop", Some("bass"), 1)
    },
    Row {
        kind: Some("one_shot"),
        key: Some("C"),
        dur: Some(1200),
        tags: &["techno"],
        ..row(4, "Bass Sub", Some("bass"), 2)
    },
    Row {
        kind: Some("loop"),
        bpm: Some(120.0),
        key: Some("Em"),
        dur: Some(4000),
        ..row(5, "Chord Loop", Some("keys"), 2)
    },
    Row {
        kind: Some("loop"),
        bpm: Some(90.0),
        key: Some("F#m"),
        tags: &["ambient"],
        fav: true,
        ..row(6, "Pad Wide", Some("pad"), 2)
    },
    Row {
        kind: Some("one_shot"),
        dur: Some(250),
        hidden: true,
        ..row(7, "Snare Tight", Some("snare"), 1)
    },
    Row {
        bpm: Some(140.0),
        key: Some("Dm"),
        dur: Some(6000),
        ..row(8, "Mystery", None, 1)
    },
    Row {
        kind: Some("loop"),
        bpm: Some(128.0),
        key: Some("G"),
        tags: &["techno"],
        ..row(9, "Arp Loop", Some("synth"), 2)
    },
];

fn to_doc(r: &Row) -> IndexDoc {
    let mut d = IndexDoc::from_paths(r.id, Path::new("/v/Lib"), &format!("{}.wav", r.name));
    d.category = r.cat.map(str::to_owned);
    d.kind = r.kind.map(str::to_owned);
    d.bpm = r.bpm;
    d.key = r.key.and_then(parse_key);
    d.duration_ms = r.dur;
    d.library_id = r.lib;
    d.tags = r.tags.iter().map(|t| (*t).to_owned()).collect();
    d.favorite = r.fav;
    d.hidden = r.hidden;
    d
}

pub(super) fn corpus() -> TextIndex {
    let ix = TextIndex::in_ram().expect("index");
    let docs: Vec<IndexDoc> = CORPUS.iter().map(to_doc).collect();
    ix.upsert(&docs[..5]).expect("segment 1");
    ix.upsert(&docs[5..]).expect("segment 2");
    ix
}

pub(super) fn run(ix: &TextIndex, q: &str, opts: SearchOptions) -> SearchResults {
    let opts = SearchOptions {
        limit: 100,
        prefix: false,
        ..opts
    };
    let plain = search(ix, &Synonyms::builtin(), q, &opts).expect("search");
    // Facet counting (drill-sideways) never changes the hits.
    let faceted = SearchOptions {
        facets: true,
        ..opts.clone()
    };
    let with = search(ix, &Synonyms::builtin(), q, &faceted).expect("search");
    assert_eq!(
        (&plain.hits, plain.total),
        (&with.hits, with.total),
        "{q:?} {opts:?}"
    );
    with
}

pub(super) fn filtered(f: Filters) -> SearchOptions {
    SearchOptions {
        filters: f,
        ..SearchOptions::default()
    }
}

fn set(ix: &TextIndex, f: Filters) -> BTreeSet<i64> {
    let r = run(ix, "", filtered(f));
    assert_eq!(r.total, r.hits.len());
    r.hits.iter().map(|h| h.file_id).collect()
}

fn order(ix: &TextIndex, q: &str, sort: Sort) -> Vec<i64> {
    let opts = SearchOptions {
        sort,
        ..SearchOptions::default()
    };
    run(ix, q, opts).hits.iter().map(|h| h.file_id).collect()
}

pub(super) fn ids(v: &[i64]) -> BTreeSet<i64> {
    v.iter().copied().collect()
}

pub(super) fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

pub(super) fn keys(v: &[&str]) -> Vec<crate::text::Key> {
    v.iter().filter_map(|k| parse_key(k)).collect()
}

pub(super) fn bpm(min: Option<f64>, max: Option<f64>) -> Filters {
    Filters {
        bpm: Some(ValueRange::new(min, max)),
        ..Filters::default()
    }
}

#[test]
fn set_filters_are_any_of_and_case_insensitive() {
    let ix = corpus();
    let cat = |c: &[&str]| Filters {
        categories: strs(c),
        ..Filters::default()
    };
    assert_eq!(
        set(&ix, cat(&["kick"])),
        ids(&[1, 2]),
        "folded: Kick = kick"
    );
    assert_eq!(set(&ix, cat(&["KICK", "bass"])), ids(&[1, 2, 3, 4]));
    assert_eq!(set(&ix, cat(&["nothing"])), ids(&[]));
    let kind = |k: &[&str]| Filters {
        kinds: strs(k),
        ..Filters::default()
    };
    assert_eq!(set(&ix, kind(&["loop"])), ids(&[3, 5, 6, 9]));
    assert_eq!(
        set(&ix, kind(&["one-shot"])),
        ids(&[1, 2, 4]),
        "7 is hidden"
    );
    let libs = Filters {
        libraries: vec![2],
        ..Filters::default()
    };
    assert_eq!(set(&ix, libs), ids(&[4, 5, 6, 9]));
    let libs = Filters {
        libraries: vec![-1, 99],
        ..Filters::default()
    };
    assert_eq!(set(&ix, libs), ids(&[]));
}

#[test]
fn ranges_are_inclusive_and_skip_unknown_values() {
    let ix = corpus();
    assert_eq!(set(&ix, bpm(Some(120.0), Some(128.0))), ids(&[3, 5, 9]));
    assert_eq!(set(&ix, bpm(Some(128.0), None)), ids(&[3, 8, 9]));
    assert_eq!(set(&ix, bpm(None, Some(90.0))), ids(&[6]));
    assert_eq!(set(&ix, bpm(Some(128.0), Some(128.0))), ids(&[3, 9]));
    assert_eq!(
        set(&ix, bpm(Some(129.0), Some(120.0))),
        ids(&[]),
        "min > max"
    );
    assert_eq!(set(&ix, bpm(None, Some(0.0))), ids(&[]));
    assert_eq!(
        set(&ix, bpm(None, None)),
        ids(&[3, 5, 6, 8, 9]),
        "known BPM"
    );
    let dur = |min, max| Filters {
        duration_ms: Some(ValueRange::new(min, max)),
        ..Filters::default()
    };
    assert_eq!(set(&ix, dur(Some(300), Some(1200))), ids(&[1, 2, 4]));
    assert_eq!(set(&ix, dur(Some(5000), None)), ids(&[3, 8]));
    assert_eq!(
        set(&ix, dur(None, None)).len(),
        6,
        "6 and 9 unknown, 7 hidden"
    );
}

#[test]
fn keys_exact_and_compatible() {
    let ix = corpus();
    let mut f = Filters {
        keys: keys(&["Am"]),
        ..Filters::default()
    };
    assert_eq!(set(&ix, f.clone()), ids(&[3]));
    f.compatible_keys = true;
    // Am → Am, Dm, Em, C (Camelot 8A: 7A, 9A, 8B).
    assert_eq!(set(&ix, f), ids(&[3, 4, 5, 8]));
    let f = Filters {
        keys: keys(&["C"]),
        compatible_keys: true,
        ..Filters::default()
    };
    // C → C, F, G, Am.
    assert_eq!(set(&ix, f), ids(&[3, 4, 9]));
}

#[test]
fn tags_are_all_of_favorites_and_hidden() {
    let ix = corpus();
    let tags = |t: &[&str]| Filters {
        tags: strs(t),
        ..Filters::default()
    };
    assert_eq!(set(&ix, tags(&["techno"])), ids(&[3, 4, 9]));
    assert_eq!(set(&ix, tags(&["DARK"])), ids(&[1, 3]));
    assert_eq!(set(&ix, tags(&["dark", "techno"])), ids(&[3]));
    let fav = Filters {
        favorites_only: true,
        ..Filters::default()
    };
    assert_eq!(set(&ix, fav), ids(&[1, 3, 6]));
    let snare = Filters {
        categories: strs(&["snare"]),
        ..Filters::default()
    };
    assert_eq!(set(&ix, snare.clone()), ids(&[]), "hidden by default");
    let shown = Filters {
        include_hidden: true,
        ..snare
    };
    assert_eq!(set(&ix, shown), ids(&[7]));
    assert_eq!(set(&ix, Filters::default()).len(), 8);
}

#[test]
fn filters_combine_with_each_other_and_text() {
    let ix = corpus();
    let f = Filters {
        categories: strs(&["bass", "synth"]),
        bpm: Some(ValueRange::new(Some(100.0), None)),
        ..Filters::default()
    };
    assert_eq!(set(&ix, f), ids(&[3, 9]));
    let f = Filters {
        libraries: vec![2],
        kinds: strs(&["loop"]),
        favorites_only: true,
        ..Filters::default()
    };
    assert_eq!(set(&ix, f), ids(&[6]));
    // Text narrows too: "loop" (name or kind) in library 2.
    let r = run(
        &ix,
        "loop",
        filtered(Filters {
            libraries: vec![2],
            ..Filters::default()
        }),
    );
    let got: BTreeSet<i64> = r.hits.iter().map(|h| h.file_id).collect();
    assert_eq!(got, ids(&[5, 6, 9]));
    // Filters add no score: same order and scores as without the filter.
    let all = run(&ix, "loop", SearchOptions::default());
    let in_lib2: Vec<Hit> = all
        .hits
        .into_iter()
        .filter(|h| got.contains(&h.file_id))
        .collect();
    assert_eq!(r.hits, in_lib2);
}

#[test]
fn sort_orders_put_unknown_values_last() {
    let ix = corpus();
    use SortField as F;
    assert_eq!(order(&ix, "", Sort::by(F::Bpm)), [6, 5, 3, 9, 8, 1, 2, 4]);
    assert_eq!(order(&ix, "", Sort::desc(F::Bpm)), [8, 3, 9, 5, 6, 1, 2, 4]);
    // Names merge across segments.
    let by_name = [9, 3, 4, 5, 1, 2, 8, 6];
    assert_eq!(order(&ix, "", Sort::by(F::Name)), by_name);
    assert_eq!(order(&ix, "", Sort::default()), by_name, "browse = by name");
    let mut rev = by_name;
    rev.reverse();
    assert_eq!(order(&ix, "", Sort::desc(F::Name)), rev);
    // C(4) Dm(8) Em(5) F#m(6) G(9) Am(3), unknown 1, 2.
    assert_eq!(order(&ix, "", Sort::by(F::Key)), [4, 8, 5, 6, 9, 3, 1, 2]);
    assert_eq!(
        order(&ix, "", Sort::by(F::Category)),
        [3, 4, 5, 1, 2, 6, 9, 8]
    );
    assert_eq!(
        order(&ix, "", Sort::desc(F::Category)),
        [9, 6, 1, 2, 5, 3, 4, 8]
    );
    assert_eq!(
        order(&ix, "", Sort::desc(F::Duration)),
        [3, 8, 5, 4, 2, 1, 6, 9]
    );
    assert_eq!(order(&ix, "", Sort::by(F::Kind)), [3, 5, 6, 9, 1, 2, 4, 8]);
    assert_eq!(
        order(&ix, "", Sort::desc(F::Library)),
        [4, 5, 6, 9, 1, 2, 3, 8]
    );
    // A text query sorted by a field: only the matches, in field order.
    assert_eq!(order(&ix, "loop", Sort::desc(F::Bpm)), [3, 9, 5, 6]);
    // Limit keeps the first ones of the order, total counts all.
    let opts = SearchOptions {
        limit: 2,
        sort: Sort::by(F::Bpm),
        ..SearchOptions::default()
    };
    let r = search(&ix, &Synonyms::builtin(), "", &opts).expect("search");
    let got: Vec<i64> = r.hits.iter().map(|h| h.file_id).collect();
    assert_eq!((got, r.total), (vec![6, 5], 8));
}

#[test]
fn relevance_with_text_is_by_score() {
    let ix = corpus();
    let r = run(&ix, "bass", SearchOptions::default());
    assert!(r.hits.windows(2).all(|w| w[0].score >= w[1].score));
    assert!(r.hits.iter().all(|h| h.score > 0.0));
}

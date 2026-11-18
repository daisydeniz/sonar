//! Key filter honesty (T10.5, ADR-041): the key filter and facet use only
//! medium + high tier keys (and user overrides) unless uncertain keys are
//! asked for; a mode-less key matches both modes; sorting by key puts the
//! low tier after the others. End to end: store rows → index → search.

use std::collections::BTreeSet;
use std::fs;

use rusqlite::params;

use super::filter_tests::{filtered, ids, keys, run};
use super::*;
use crate::key::{KEY_TIER_HIGH, KEY_TIER_MEDIUM};
use crate::Store;

/// Files of the tier corpus, in insertion (= id) order.
struct Tiers {
    ix: TextIndex,
    high: i64,
    medium: i64,
    low: i64,
    no_conf: i64,
    overridden: i64,
    name_only: i64,
    modeless_high: i64,
    modeless_low: i64,
    major: i64,
}

fn tiers() -> Tiers {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("Lib");
    fs::create_dir(&root).expect("mkdir");
    let store = Store::open_in_memory().expect("store");
    let lib = store.add_library(&root).expect("library").id;
    let c = store.conn();
    let below = f64::from(KEY_TIER_MEDIUM) - 0.01;
    let add = |rel: &str, key: Option<&str>, conf: Option<f64>| {
        c.execute(
            "INSERT INTO files (library_id, rel_path, size, mtime) VALUES (?1, ?2, 1, 1)",
            params![lib, rel],
        )
        .expect("file");
        let id = c.last_insert_rowid();
        if key.is_some() {
            c.execute(
                "INSERT INTO analysis (file_id, key, key_conf, key_source) \
                 VALUES (?1, ?2, ?3, 'detected')",
                params![id, key, conf],
            )
            .expect("analysis");
        }
        id
    };
    let high = add("Pad High.wav", Some("Am"), Some(f64::from(KEY_TIER_HIGH)));
    let medium = add(
        "Pad Medium.wav",
        Some("Am"),
        Some(f64::from(KEY_TIER_MEDIUM)),
    );
    let low = add("HiHat.wav", Some("Am"), Some(below));
    let no_conf = add("Kick.wav", Some("Am"), None);
    let overridden = add("Hat Fixed.wav", Some("Am"), Some(0.0));
    // Not analysed yet: the name alone gives the key.
    let name_only = add("Bass_Loop_Am.wav", None, None);
    let modeless_high = add("Sub 808.wav", Some("A"), Some(f64::from(KEY_TIER_HIGH)));
    let modeless_low = add("Tom Loop.wav", Some("A"), Some(below));
    let major = add("Keys.wav", Some("Amaj"), Some(f64::from(KEY_TIER_HIGH)));
    c.execute(
        "INSERT INTO overrides (file_id, field, value) VALUES (?1, 'key', 'Cm')",
        [overridden],
    )
    .expect("override");
    let ix = TextIndex::in_ram().expect("index");
    ix.rebuild(&store).expect("rebuild");
    Tiers {
        ix,
        high,
        medium,
        low,
        no_conf,
        overridden,
        name_only,
        modeless_high,
        modeless_low,
        major,
    }
}

fn key_filter(k: &[&str], uncertain: bool) -> Filters {
    Filters {
        keys: keys(k),
        include_uncertain_keys: uncertain,
        ..Filters::default()
    }
}

fn hits(t: &Tiers, f: Filters) -> BTreeSet<i64> {
    run(&t.ix, "", filtered(f))
        .hits
        .iter()
        .map(|h| h.file_id)
        .collect()
}

fn key_counts(t: &Tiers, f: Filters) -> Vec<(String, usize)> {
    let r = run(&t.ix, "", filtered(f));
    let facets = r.facets.expect("facets");
    facets
        .keys
        .into_iter()
        .map(|c| (c.value, c.count))
        .collect()
}

fn owned(v: &[(&str, usize)]) -> Vec<(String, usize)> {
    v.iter().map(|(s, n)| ((*s).to_owned(), *n)).collect()
}

#[test]
fn key_filter_uses_medium_and_high_by_default() {
    let t = tiers();
    assert_eq!(
        hits(&t, key_filter(&["Am"], false)),
        ids(&[t.high, t.medium, t.modeless_high]),
        "low, missing confidence and name-only keys are left out"
    );
    assert_eq!(
        hits(&t, key_filter(&["Am"], true)),
        ids(&[
            t.high,
            t.medium,
            t.low,
            t.no_conf,
            t.name_only,
            t.modeless_high,
            t.modeless_low
        ]),
        "include uncertain adds the low tier"
    );
    // A user override is certain whatever the analysis said.
    assert_eq!(hits(&t, key_filter(&["Cm"], false)), ids(&[t.overridden]));
    assert!(!hits(&t, key_filter(&["Am"], true)).contains(&t.overridden));
}

#[test]
fn mode_less_keys_match_both_modes() {
    let t = tiers();
    // `A` typed = A major: the major file and the mode-less one.
    assert_eq!(
        hits(&t, key_filter(&["A"], false)),
        ids(&[t.modeless_high, t.major])
    );
    assert_eq!(
        hits(&t, key_filter(&["A"], true)),
        ids(&[t.modeless_high, t.modeless_low, t.major])
    );
    assert!(hits(&t, key_filter(&["Am"], false)).contains(&t.modeless_high));
    assert!(!hits(&t, key_filter(&["Am"], true)).contains(&t.major));
    // Compatible keys of Em include Am: the mode-less A answers too.
    let em = Filters {
        compatible_keys: true,
        ..key_filter(&["Em"], false)
    };
    assert!(hits(&t, em).contains(&t.modeless_high));
}

#[test]
fn key_facet_counts_the_same_tiers_as_the_filter() {
    let t = tiers();
    // Certain: Am = high, medium, mode-less; A = major, mode-less.
    let certain = owned(&[("Am", 3), ("A", 2), ("Cm", 1)]);
    assert_eq!(key_counts(&t, Filters::default()), certain);
    // Drill-sideways: the key chips ignore the key filter, not the tiers.
    assert_eq!(key_counts(&t, key_filter(&["Am"], false)), certain);
    let all = owned(&[("Am", 7), ("A", 3), ("Cm", 1)]);
    let uncertain = Filters {
        include_uncertain_keys: true,
        ..Filters::default()
    };
    assert_eq!(key_counts(&t, uncertain), all);
    assert_eq!(key_counts(&t, key_filter(&["Cm"], true)), all);
}

#[test]
fn key_sort_puts_the_low_tier_last_both_ways() {
    let t = tiers();
    let order = |descending| -> Vec<i64> {
        let opts = SearchOptions {
            sort: Sort {
                field: SortField::Key,
                descending,
            },
            ..SearchOptions::default()
        };
        run(&t.ix, "", opts)
            .hits
            .iter()
            .map(|h| h.file_id)
            .collect()
    };
    // Cm, A, A (mode-less), Am, Am | low: A (mode-less), Am, Am, Am.
    assert_eq!(
        order(false),
        [
            t.overridden,
            t.major,
            t.modeless_high,
            t.high,
            t.medium,
            t.modeless_low,
            t.low,
            t.no_conf,
            t.name_only
        ]
    );
    assert_eq!(
        order(true),
        [
            t.high,
            t.medium,
            t.modeless_high,
            t.major,
            t.overridden,
            t.low,
            t.no_conf,
            t.name_only,
            t.modeless_low
        ]
    );
}

//! T5.5a acceptance: with the engine alone (no manual index rebuild), a
//! loop dropped into a watched, indexed library is analyzed automatically
//! and found by name and by a BPM range that excludes the other loops.
//! T5.5b acceptance: user data set through the engine is in the next search.
//! Every wait polls with a generous timeout; no fixed sleeps are asserted on.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use sonar_core::engine::{Engine, EngineEvent, SearchRequest};
use sonar_core::fixtures::{self, FixtureOptions};
use sonar_core::jobs::JobOptions;
use sonar_core::search::keys::parse_key;
use sonar_core::search::{Filters, SearchMode, ValueRange};
use sonar_core::store::OverrideField;
use sonar_core::watch::WatchOptions;

const STEP_TIMEOUT: Duration = Duration::from_secs(120);

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < STEP_TIMEOUT {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out after {STEP_TIMEOUT:?} waiting for: {what}");
}

/// File names of the text-mode hits of `query` under `filters`.
fn hits(engine: &Engine, query: &str, filters: Filters) -> Vec<String> {
    let req = SearchRequest {
        query: query.into(),
        mode: SearchMode::Text,
        filters,
        ..SearchRequest::default()
    };
    let mut names: Vec<String> = engine
        .search(&req)
        .expect("search")
        .hits
        .iter()
        .filter_map(|h| engine.file_path(h.file_id).expect("path"))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

fn bpm_between(min: f64, max: f64) -> Filters {
    Filters {
        bpm: Some(ValueRange::new(Some(min), Some(max))),
        ..Filters::default()
    }
}

/// Copies the fixture drum loop with this exact tempo to `dest`.
fn copy_loop(fx: &fixtures::Manifest, root: &Path, bpm: f64, dest: &Path) {
    let f = fx
        .fixtures
        .iter()
        .find(|f| f.path.starts_with("loops/") && f.truth.bpm == Some(bpm))
        .unwrap_or_else(|| panic!("no fixture loop at {bpm} BPM"));
    fs::copy(root.join(&f.path), dest).expect("copy loop");
}

#[test]
fn watched_new_loop_is_analyzed_and_searchable_without_rebuild() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fx_dir = tmp.path().join("fixtures");
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    let fx = fixtures::generate(&fx_dir, &opts).expect("fixtures");
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).expect("mkdir lib");
    // Two loops at 85 and 100 BPM (neutral names: tempo comes from audio).
    copy_loop(&fx, &fx_dir, 85.0, &lib.join("old_groove.wav"));
    copy_loop(&fx, &fx_dir, 100.0, &lib.join("older_groove.wav"));

    let engine = Engine::open(tmp.path().join("home")).expect("open engine");
    let (_, report) = engine.add_library(&lib).expect("add library");
    assert_eq!(report.counts.added, 2);
    assert!(engine.start_indexing(JobOptions::default()).expect("start"));
    let summary = engine
        .wait_indexing(Some(STEP_TIMEOUT))
        .expect("indexing")
        .expect("indexing finished in time");
    assert!(!summary.cancelled);
    // Analysis results are filterable now, still without a rebuild.
    assert_eq!(
        hits(&engine, "", bpm_between(80.0, 105.0)),
        ["old_groove.wav", "older_groove.wav"]
    );
    assert!(hits(&engine, "", bpm_between(120.0, 135.0)).is_empty());
    assert!(hits(&engine, "fresh", Filters::default()).is_empty());

    let watch = WatchOptions {
        debounce: Duration::from_millis(50),
        max_delay: Duration::from_millis(500),
        root_poll: Duration::from_millis(200),
        error_retry: Duration::from_millis(200),
    };
    engine.start_watching(watch).expect("start watching");
    assert!(engine.is_watching());
    wait_until("the watcher's start-up scan", || {
        engine
            .poll_events()
            .iter()
            .any(|e| matches!(e, EngineEvent::Watch(_)))
    });

    // A new 128 BPM loop appears.
    copy_loop(&fx, &fx_dir, 128.0, &lib.join("fresh_groove.wav"));
    wait_until("the new loop in text search", || {
        hits(&engine, "fresh groove", Filters::default()) == ["fresh_groove.wav"]
    });
    // The engine queued analysis itself ("idle-but-enabled").
    wait_until("the new loop analyzed (BPM filter)", || {
        hits(&engine, "", bpm_between(120.0, 135.0)) == ["fresh_groove.wav"]
    });
    assert_eq!(
        hits(&engine, "", bpm_between(80.0, 105.0)),
        ["old_groove.wav", "older_groove.wav"],
        "the other loops stay where they were"
    );

    // Deleting it removes it from the index again.
    fs::remove_file(lib.join("fresh_groove.wav")).expect("remove");
    wait_until("the removed loop gone from search", || {
        hits(&engine, "fresh groove", Filters::default()).is_empty()
    });

    engine.stop_watching();
    assert!(!engine.is_watching());
    engine.wait_indexing(Some(STEP_TIMEOUT)).expect("idle");
    assert!(
        engine.startup_rebuild().is_some() && engine.text_index_docs() == 2,
        "the index was built once at open and then only refreshed"
    );
}

/// T5.5b acceptance: user data set through the engine is visible in the very
/// next search (favourites, hidden, tags, overrides), bad values are errors
/// that change nothing, clearing reverts.
#[test]
fn user_data_set_through_the_engine_is_in_the_next_search() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fx_dir = tmp.path().join("fixtures");
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    let fx = fixtures::generate(&fx_dir, &opts).expect("fixtures");
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).expect("mkdir lib");
    copy_loop(&fx, &fx_dir, 85.0, &lib.join("slow_groove.wav"));
    copy_loop(&fx, &fx_dir, 100.0, &lib.join("mid_groove.wav"));

    let engine = Engine::open(tmp.path().join("home")).expect("open engine");
    engine.add_library(&lib).expect("add library");
    assert!(engine.start_indexing(JobOptions::default()).expect("start"));
    engine
        .wait_indexing(Some(STEP_TIMEOUT))
        .expect("indexing")
        .expect("indexing finished in time");
    let both = ["mid_groove.wav", "slow_groove.wav"];
    assert_eq!(hits(&engine, "", Filters::default()), both);
    let id_of = |name: &str| {
        let req = SearchRequest {
            query: name.into(),
            mode: SearchMode::Text,
            ..SearchRequest::default()
        };
        engine.search(&req).expect("search").hits[0].file_id
    };
    let slow = id_of("slow groove");
    let mid = id_of("mid groove");
    let fav = Filters {
        favorites_only: true,
        ..Filters::default()
    };

    // Favourite: absent, then present in the next search, then gone again.
    assert!(hits(&engine, "", fav.clone()).is_empty());
    assert_eq!(engine.set_favorite(&[slow], true).expect("fav"), 1);
    assert_eq!(hits(&engine, "", fav.clone()), ["slow_groove.wav"]);
    assert_eq!(engine.set_favorite(&[slow], true).expect("again"), 0);
    engine.set_favorite(&[slow], false).expect("unfav");
    let fav_after = fav.clone();
    assert!(hits(&engine, "", fav).is_empty());

    // BPM override: found at the overridden tempo, no longer at the detected.
    assert_eq!(
        hits(&engine, "", bpm_between(80.0, 90.0)),
        ["slow_groove.wav"]
    );
    engine
        .set_override(&[slow], OverrideField::Bpm, "128")
        .expect("override");
    assert_eq!(
        hits(&engine, "", bpm_between(120.0, 135.0)),
        ["slow_groove.wav"]
    );
    assert!(hits(&engine, "", bpm_between(80.0, 90.0)).is_empty());
    // A non-numeric value is an error and changes nothing.
    assert!(engine
        .set_override(&[slow], OverrideField::Bpm, "140 bpm")
        .is_err());
    assert_eq!(
        hits(&engine, "", bpm_between(120.0, 135.0)),
        ["slow_groove.wav"]
    );
    engine
        .clear_override(&[slow], OverrideField::Bpm)
        .expect("clear");
    assert_eq!(
        hits(&engine, "", bpm_between(80.0, 90.0)),
        ["slow_groove.wav"]
    );

    // Category and kind overrides feed the filters too.
    engine
        .set_override(&[mid], OverrideField::Category, "Hi-Hat")
        .expect("category");
    engine
        .set_override(&[mid], OverrideField::Kind, "one-shot")
        .expect("kind");
    let hat = Filters {
        categories: vec!["hat".into()],
        kinds: vec!["one_shot".into()],
        ..Filters::default()
    };
    assert_eq!(hits(&engine, "", hat), ["mid_groove.wav"]);
    let f_sharp_minor = Filters {
        keys: vec![parse_key("F# minor").expect("key")],
        ..Filters::default()
    };
    engine
        .set_override(&[mid], OverrideField::Key, "f#m")
        .expect("key");
    assert_eq!(hits(&engine, "", f_sharp_minor), ["mid_groove.wav"]);

    // Tags (user spelling kept in the tag list, filter ignores case).
    let tagged = Filters {
        tags: vec!["dark vibe".into()],
        ..Filters::default()
    };
    assert!(hits(&engine, "", tagged.clone()).is_empty());
    engine
        .add_tags(&[slow, mid], &["Dark Vibe".into()])
        .expect("tag");
    assert_eq!(hits(&engine, "", tagged.clone()), both);
    assert_eq!(engine.tags().expect("tags")[0].name, "Dark Vibe");
    engine
        .remove_tags(&[mid], &["dark vibe".into()])
        .expect("untag");
    assert_eq!(hits(&engine, "", tagged), ["slow_groove.wav"]);

    // Hidden: gone from search unless asked for.
    assert_eq!(engine.set_hidden(&[slow], true).expect("hide"), 1);
    assert_eq!(hits(&engine, "", Filters::default()), ["mid_groove.wav"]);
    let with_hidden = Filters {
        include_hidden: true,
        ..Filters::default()
    };
    assert_eq!(hits(&engine, "", with_hidden), both);
    engine.set_hidden(&[slow], false).expect("unhide");
    assert_eq!(hits(&engine, "", Filters::default()), both);

    // Unknown ids fail without side effects; collections need no refresh.
    assert!(engine.set_favorite(&[slow, 9_999], true).is_err());
    assert!(hits(&engine, "", fav_after).is_empty());
    let c = engine.create_collection("Session").expect("collection");
    assert_eq!(engine.add_to_collection(c, &[mid, slow]).expect("add"), 2);
    assert_eq!(engine.collection_items(c).expect("items"), [mid, slow]);
    assert_eq!(engine.collections().expect("list")[0].files, 2);
    assert_eq!(engine.text_index_docs(), 2, "never rebuilt or duplicated");
}

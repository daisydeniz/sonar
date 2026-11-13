//! T5.5a acceptance: with the engine alone (no manual index rebuild), a
//! loop dropped into a watched, indexed library is analyzed automatically
//! and found by name and by a BPM range that excludes the other loops.
//! Every wait polls with a generous timeout; no fixed sleeps are asserted on.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use sonar_core::engine::{Engine, EngineEvent, SearchRequest};
use sonar_core::fixtures::{self, FixtureOptions};
use sonar_core::jobs::JobOptions;
use sonar_core::search::{Filters, SearchMode, ValueRange};
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

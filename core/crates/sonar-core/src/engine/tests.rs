//! Engine unit tests: index sync without rebuilds, library removal,
//! latest-query-wins, indexing control. The end-to-end watcher test is in
//! `tests/engine.rs`.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use super::*;
use crate::fixtures::wav::{encode, SampleFormat, WavSpec};
use crate::jobs::JobOptions;
use crate::search::{Filters, SearchMode, ValueRange};

const WAIT: Duration = Duration::from_secs(60);

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    lib: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let lib = tmp.path().join("lib");
        fs::create_dir(&lib).expect("mkdir lib");
        Self {
            _tmp: tmp,
            home,
            lib,
        }
    }

    fn wav(&self, name: &str, seed: u32) {
        let step = 0.02 + seed as f32 * 0.003;
        let tone: Vec<f32> = (0..800).map(|i| 0.4 * (i as f32 * step).sin()).collect();
        let bytes = encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes;
        fs::write(self.lib.join(name), bytes).expect("write wav");
    }
}

fn names(engine: &Engine, query: &str) -> Vec<String> {
    let req = SearchRequest {
        query: query.into(),
        mode: SearchMode::Text,
        ..SearchRequest::default()
    };
    let mut out: Vec<String> = engine
        .search(&req)
        .expect("search")
        .hits
        .iter()
        .filter_map(|h| engine.file_path(h.file_id).expect("path"))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    out.sort();
    out
}

#[test]
fn open_builds_the_index_once() {
    let env = Env::new();
    {
        let engine = Engine::open(&env.home).expect("open");
        let (reason, report) = engine.startup_rebuild().expect("fresh home rebuilds");
        assert_eq!(*reason, RebuildReason::New);
        assert_eq!(report.docs, 0);
        assert!(engine.text_index_ready());
    }
    let engine = Engine::open(&env.home).expect("reopen");
    assert!(engine.startup_rebuild().is_none());
}

#[test]
fn scans_refresh_only_touched_files() {
    let env = Env::new();
    env.wav("kick_one.wav", 1);
    env.wav("snare_two.wav", 2);
    let engine = Engine::open(&env.home).expect("open");
    let (lib, report) = engine.add_library(&env.lib).expect("add");
    assert_eq!(report.counts.added, 2);
    assert_eq!(report.touched.len(), 2);
    assert_eq!(engine.libraries().expect("list"), vec![lib]);
    assert_eq!(names(&engine, "kick"), ["kick_one.wav"]);

    env.wav("kick_three.wav", 3);
    fs::rename(env.lib.join("snare_two.wav"), env.lib.join("clap_two.wav")).expect("rename");
    let again = engine.scan_library(1).expect("rescan");
    assert_eq!(
        (
            again.counts.added,
            again.counts.moved,
            again.counts.unchanged
        ),
        (1, 1, 1)
    );
    assert_eq!(again.touched.len(), 2, "only the changed rows are touched");
    assert_eq!(names(&engine, "kick"), ["kick_one.wav", "kick_three.wav"]);
    assert_eq!(names(&engine, "clap"), ["clap_two.wav"]);
    assert!(names(&engine, "snare").is_empty());

    fs::remove_file(env.lib.join("kick_one.wav")).expect("rm");
    engine.scan_library(1).expect("rescan");
    assert_eq!(names(&engine, "kick"), ["kick_three.wav"]);
    assert!(engine.startup_rebuild().is_some(), "only the open built it");
}

#[test]
fn removing_a_library_empties_its_results() {
    let env = Env::new();
    env.wav("kick_one.wav", 1);
    let engine = Engine::open(&env.home).expect("open");
    let (lib, _) = engine.add_library(&env.lib).expect("add");
    assert_eq!(engine.text_index_docs(), 1);
    assert!(engine.remove_library(lib.id).expect("remove"));
    assert!(!engine.remove_library(lib.id).expect("again"));
    assert!(engine.libraries().expect("list").is_empty());
    assert!(names(&engine, "kick").is_empty());
    assert_eq!(engine.text_index_docs(), 0);
}

#[test]
fn newest_search_wins() {
    let env = Env::new();
    env.wav("kick_one.wav", 1);
    let engine = Engine::open(&env.home).expect("open");
    engine.add_library(&env.lib).expect("add");
    let req = SearchRequest {
        query: "kick".into(),
        mode: SearchMode::Text,
        ..SearchRequest::default()
    };
    let old = engine.begin_search();
    let new = engine.begin_search();
    assert!(matches!(
        engine.run_search(&req, &old),
        Err(Error::Cancelled)
    ));
    assert_eq!(engine.run_search(&req, &new).expect("newest").hits.len(), 1);
    // cancel_search stops the newest; the next search is unaffected.
    engine.cancel_search();
    assert!(matches!(
        engine.run_search(&req, &new),
        Err(Error::Cancelled)
    ));
    assert_eq!(engine.search(&req).expect("fresh").hits.len(), 1);
}

#[test]
fn paging_skips_hits_and_keeps_order() {
    let env = Env::new();
    for i in 0..5 {
        env.wav(&format!("kick_{i}.wav"), i);
    }
    let engine = Engine::open(&env.home).expect("open");
    engine.add_library(&env.lib).expect("add");
    let page = |offset, limit| {
        let req = SearchRequest {
            query: "kick".into(),
            mode: SearchMode::Text,
            offset,
            limit,
            ..SearchRequest::default()
        };
        engine.search(&req).expect("search")
    };
    let all: Vec<i64> = page(0, 10).hits.iter().map(|h| h.file_id).collect();
    assert_eq!(all.len(), 5);
    let second: Vec<i64> = page(2, 2).hits.iter().map(|h| h.file_id).collect();
    assert_eq!(second, all[2..4]);
    assert!(page(9, 3).hits.is_empty());
}

#[test]
fn leaving_the_index_unbuilt_then_searching_builds_it() {
    let env = Env::new();
    env.wav("kick_one.wav", 1);
    let options = EngineOptions {
        rebuild_index: false,
        ..EngineOptions::default()
    };
    let engine = Engine::open_with(&env.home, options).expect("open");
    assert!(!engine.text_index_ready());
    engine.add_library(&env.lib).expect("add");
    assert_eq!(engine.text_index_docs(), 0, "no sync into an unbuilt index");
    assert_eq!(names(&engine, "kick"), ["kick_one.wav"]);
    assert!(engine.text_index_ready());
}

#[test]
fn indexing_runs_stages_and_keeps_the_index_in_step() {
    let env = Env::new();
    env.wav("kick_one.wav", 1);
    env.wav("kick_two.wav", 2);
    let engine = Engine::open(&env.home).expect("open");
    engine.add_library(&env.lib).expect("add");
    // Before analysis no file has a duration, so a duration filter is empty.
    let req = SearchRequest {
        filters: Filters {
            duration_ms: Some(ValueRange::new(Some(40), Some(60))),
            ..Filters::default()
        },
        mode: SearchMode::Text,
        ..SearchRequest::default()
    };
    assert!(engine.search(&req).expect("browse").hits.is_empty());
    assert!(!engine.indexing_status().running);
    assert!(engine.start_indexing(JobOptions::default()).expect("start"));
    let summary = engine
        .wait_indexing(Some(WAIT))
        .expect("wait")
        .expect("finished in time");
    assert!(!summary.cancelled);
    assert_eq!(summary.stages.len(), 2, "meta and dsp ran");
    let status = engine.indexing_status();
    assert!(status.enabled && !status.running);
    // The meta stage made duration filterable; only touched ids were
    // re-read (the index was never rebuilt after open).
    assert_eq!(engine.search(&req).expect("browse").hits.len(), 2);
    assert!(engine.poll_events().iter().any(|e| matches!(
        e,
        EngineEvent::IndexingFinished {
            cancelled: false,
            error: None
        }
    )));
    assert!(engine.startup_rebuild().is_some());
}

#[test]
fn cancel_disables_automatic_indexing() {
    let env = Env::new();
    env.wav("kick_one.wav", 1);
    let engine = Engine::open(&env.home).expect("open");
    engine.start_indexing(JobOptions::default()).expect("start");
    engine.wait_indexing(Some(WAIT)).expect("wait");
    engine.cancel_indexing();
    assert!(!engine.indexing_status().enabled);
    // A scan that finds files does not start a job now.
    engine.add_library(&env.lib).expect("add");
    assert!(!engine.indexing_status().running);
    // Enabled again, the next scan does.
    engine.start_indexing(JobOptions::default()).expect("start");
    engine.wait_indexing(Some(WAIT)).expect("wait");
    env.wav("kick_two.wav", 2);
    engine.scan_library(1).expect("rescan");
    engine
        .wait_indexing(Some(WAIT))
        .expect("wait")
        .expect("a job ran");
    let store = Store::open(env.home.join(paths::DB_FILE_NAME)).expect("store");
    let analyzed: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM files WHERE status = 'analyzed'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(analyzed, 2);
}

#[test]
fn large_refreshes_rebuild_small_ones_do_not() {
    let env = Env::new();
    let engine = Engine::open(&env.home).expect("open");
    let inner = &engine.inner;
    // Empty index: a handful of ids is a refresh, a thousand is a rebuild.
    assert!(!inner.prefers_rebuild(10));
    assert!(inner.prefers_rebuild(REBUILD_MIN_IDS));
    assert!(inner.prefers_rebuild(REBUILD_ALWAYS_IDS + 1));
}

/// One file's setter + synchronous index refresh on the 500k bench corpus
/// (ADR-031). Not part of the gate.
#[test]
#[ignore = "benchmark: cargo test --release -p sonar-core --lib setter_latency -- --ignored --nocapture"]
fn setter_latency_on_500k_files() {
    use crate::bench::search_corpus::generate_store;
    use crate::store::OverrideField;
    let env = Env::new();
    fs::create_dir_all(&env.home).expect("home");
    {
        let store = Store::open(env.home.join(paths::DB_FILE_NAME)).expect("store");
        generate_store(&store, &env.lib, 500_000, 1_592_610_986).expect("corpus");
    }
    let engine = Engine::open(&env.home).expect("open (builds the index)");
    let mut times: Vec<(&str, Vec<f64>)> = vec![
        ("favorite", vec![]),
        ("hidden", vec![]),
        ("bpm override", vec![]),
        ("tag", vec![]),
    ];
    for i in 0..30_i64 {
        let id = [1000 + i];
        let t = Instant::now();
        engine.set_favorite(&id, true).expect("fav");
        times[0].1.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        engine.set_hidden(&id, true).expect("hide");
        times[1].1.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        engine
            .set_override(&id, OverrideField::Bpm, "128")
            .expect("bpm");
        times[2].1.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        engine.add_tags(&id, &["bench".into()]).expect("tag");
        times[3].1.push(t.elapsed().as_secs_f64() * 1e3);
    }
    for (name, v) in &mut times {
        v.sort_by(f64::total_cmp);
        println!(
            "{name:>13}: p50 {:.2} ms, max {:.2} ms",
            v[v.len() / 2],
            v[v.len() - 1]
        );
    }
}

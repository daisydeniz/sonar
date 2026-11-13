//! Tests of the FFI wrapper as Swift would drive it: add -> index -> search ->
//! favourite -> search -> peaks, plus error mapping.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};

use crate::*;

fn write_wavs(dir: &Path, names: &[&str]) {
    fs::create_dir_all(dir).expect("mkdir");
    for (i, name) in names.iter().enumerate() {
        let step = 0.02 + i as f32 * 0.004;
        let tone: Vec<f32> = (0..4000).map(|n| 0.5 * (n as f32 * step).sin()).collect();
        let bytes = encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes;
        fs::write(dir.join(name), bytes).expect("write");
    }
}

fn wait_idle(engine: &SonarEngine) -> IndexingProgress {
    let start = Instant::now();
    loop {
        let p = engine.indexing_progress();
        if !p.running {
            return p;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "indexing hangs");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn text(q: &str) -> SearchQuery {
    SearchQuery {
        text: q.into(),
        mode: Some(SearchMode::Text),
        filters: None,
        sort: None,
        descending: false,
        offset: 0,
        limit: 50,
        prefix: true,
        facets: false,
    }
}

#[test]
fn core_version_matches_sonar_core() {
    assert_eq!(core_version(), sonar_core::version());
    assert!(!core_version().is_empty());
}

#[test]
fn add_index_search_favourite_peaks() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lib = tmp.path().join("lib");
    write_wavs(
        &lib,
        &["zorg_a.wav", "zorg_b.wav", "quux_c.wav", "blip_d.wav"],
    );
    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");

    let added = engine
        .add_library(lib.display().to_string())
        .expect("add library");
    assert_eq!(added.scan.added, 4);
    assert_eq!(engine.libraries().expect("libraries").len(), 1);

    assert!(engine.start_indexing(false).expect("start"));
    let done = wait_idle(&engine);
    assert_eq!(done.errors, 0);
    assert!(engine
        .poll_events()
        .iter()
        .any(|e| matches!(e, EngineEvent::IndexingStarted)));

    let page = engine.search(text("zorg")).expect("search");
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.total, 2);
    assert!(!page.has_more);
    assert_eq!(page.mode, SearchMode::Text);
    let hit = &page.rows[0];
    assert!(hit.name.starts_with("zorg_"));
    assert!(hit.duration_ms.is_some(), "indexed: duration is known");
    assert!(!hit.favorite);

    assert_eq!(
        engine.set_favorite(vec![hit.file_id], true).expect("fav"),
        1
    );
    assert_eq!(
        engine.set_favorite(vec![hit.file_id], true).expect("fav"),
        0
    );
    let favs = engine
        .search(SearchQuery {
            filters: Some(SearchFilters {
                favorites_only: true,
                ..SearchFilters::default()
            }),
            ..text("")
        })
        .expect("favourites");
    assert_eq!(favs.rows.len(), 1);
    assert_eq!(favs.rows[0].file_id, hit.file_id);
    assert!(favs.rows[0].favorite);

    // Hybrid (the default) must work without CLAP model files.
    let hybrid = engine
        .search(SearchQuery {
            mode: None,
            ..text("quux")
        })
        .expect("hybrid");
    assert_eq!(hybrid.rows.len(), 1);

    let peaks = engine.waveform_peaks(hit.file_id, 200).expect("peaks");
    assert_eq!(peaks.len(), 200);
    assert!(peaks.iter().all(|p| p.min <= p.max && p.max <= 1.0));
    assert!(peaks.iter().any(|p| p.max > 0.3));

    let pcm = engine.preview_pcm(hit.file_id).expect("pcm");
    assert_eq!((pcm.sample_rate, pcm.channels), (16_000, 1));
    assert_eq!(pcm.samples.len(), 4000);

    assert!(engine.remove_library(added.library.id).expect("remove"));
    assert!(engine.search(text("zorg")).expect("search").rows.is_empty());
}

#[test]
fn user_data_and_paging() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lib = tmp.path().join("lib");
    write_wavs(&lib, &["a_one.wav", "a_two.wav", "a_three.wav"]);
    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");
    engine.add_library(lib.display().to_string()).expect("add");

    let all = engine.search(text("a")).expect("search");
    let ids: Vec<i64> = all.rows.iter().map(|r| r.file_id).collect();
    assert_eq!(ids.len(), 3);
    let page = engine
        .search(SearchQuery {
            limit: 2,
            ..text("a")
        })
        .expect("page");
    assert!(page.has_more && page.rows.len() == 2);
    let rest = engine
        .search(SearchQuery {
            offset: 2,
            limit: 2,
            ..text("a")
        })
        .expect("rest");
    assert_eq!(rest.rows.len(), 1);
    assert_eq!(rest.rows[0].file_id, ids[2]);

    engine
        .add_tags(vec![ids[0]], vec!["Dark".into()])
        .expect("tag");
    assert_eq!(engine.tags().expect("tags")[0].files, 1);
    engine
        .set_override(vec![ids[1]], OverrideField::Bpm, "128".into())
        .expect("override");
    assert_eq!(
        engine.row(ids[1]).expect("row").expect("some").bpm,
        Some(128.0)
    );
    let c = engine
        .create_collection("Picks".into())
        .expect("collection");
    engine
        .add_to_collection(c, vec![ids[2], ids[0]])
        .expect("add");
    let items = engine.collection_rows(c).expect("items");
    assert_eq!(
        items.iter().map(|r| r.file_id).collect::<Vec<_>>(),
        vec![ids[2], ids[0]]
    );
    assert_eq!(engine.collections().expect("list")[0].files, 2);
    engine.set_hidden(vec![ids[0]], true).expect("hide");
    assert_eq!(engine.search(text("a")).expect("search").rows.len(), 2);
}

#[test]
fn errors_are_values_not_panics() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");
    assert!(matches!(
        engine.set_favorite(vec![999], true),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        engine.set_override(vec![], OverrideField::Bpm, "fast".into()),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        engine.search(SearchQuery {
            filters: Some(SearchFilters {
                keys: vec!["not a key".into()],
                ..SearchFilters::default()
            }),
            ..text("x")
        }),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        engine.waveform_peaks(1, 10),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        engine.preview_pcm(1),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        engine.add_library(tmp.path().join("nope").display().to_string()),
        Err(FfiError::Io { .. } | FfiError::InvalidInput { .. } | FfiError::NotFound { .. })
    ));
    assert!(matches!(
        engine.similar(1, 5),
        Err(FfiError::NotFound { .. } | FfiError::Internal { .. })
    ));
    assert!(engine.row(5).expect("row").is_none());
}

#[test]
fn cancel_with_nothing_in_flight_keeps_search_working() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");
    // Nothing in flight: cancelling must not poison the next search.
    engine.cancel_search();
    assert!(engine.search(text("kick")).expect("search").rows.is_empty());
}

/// Facets: opt-in, consistent with the unfiltered result, drill-sideways.
#[test]
fn facets_are_opt_in_and_drill_sideways() {
    use sonar_core::fixtures::{generate, FixtureOptions};
    let tmp = tempfile::tempdir().expect("tempdir");
    let fx = tmp.path().join("fx");
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    generate(&fx, &opts).expect("fixtures");
    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");
    engine.add_library(fx.display().to_string()).expect("add");
    engine.start_indexing(false).expect("start");
    wait_idle(&engine);

    let off = engine.search(text("")).expect("no facets");
    assert!(off.facets.is_none());

    let on = engine
        .search(SearchQuery {
            facets: true,
            ..text("")
        })
        .expect("facets");
    let f = on.facets.expect("requested facets");
    assert!(!f.categories.is_empty() && !f.kinds.is_empty());
    assert!(f.categories.windows(2).all(|w| w[0].count >= w[1].count));
    // Each dimension counts a file at most once: sums stay within the total.
    let sum = |v: &[FacetCount]| v.iter().map(|c| c.count).sum::<u64>();
    assert!(sum(&f.categories) <= on.total && sum(&f.kinds) <= on.total);
    assert!(f.libraries.iter().map(|l| l.count).sum::<u64>() == on.total);

    // Drill-sideways: filtering on one category keeps the others listed.
    let top = f.categories[0].value.clone();
    let narrowed = engine
        .search(SearchQuery {
            facets: true,
            filters: Some(SearchFilters {
                categories: vec![top.clone()],
                ..SearchFilters::default()
            }),
            ..text("")
        })
        .expect("narrowed");
    assert!(narrowed.total < on.total);
    let nf = narrowed.facets.expect("facets");
    assert_eq!(nf.categories.len(), f.categories.len());
    assert!(nf.categories.iter().any(|c| c.value != top));
    assert_eq!(sum(&nf.categories), sum(&f.categories));
}

/// The same flow as `app/SonarTests/EngineBridgeTests.swift`, on the real
/// `sonar fixtures` set (including a damaged-file folder and Ogg Vorbis).
#[test]
fn fixtures_library_like_the_xctest() {
    use sonar_core::fixtures::{generate, FixtureOptions};
    let tmp = tempfile::tempdir().expect("tempdir");
    let fx = tmp.path().join("fx");
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    generate(&fx, &opts).expect("fixtures");
    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");
    let added = engine.add_library(fx.display().to_string()).expect("add");
    assert!(added.scan.added > 100);
    let started = Instant::now();
    assert!(engine.start_indexing(false).expect("start"));
    wait_idle(&engine);
    eprintln!(
        "fixtures indexed in {:.2} s",
        started.elapsed().as_secs_f64()
    );

    let hits = engine.search(text("hit")).expect("search");
    assert!(hits.rows.len() >= 12, "{} hits", hits.rows.len());
    let hit = hits.rows[0].clone();
    assert!(hit.duration_ms.is_some());
    engine.set_favorite(vec![hit.file_id], true).expect("fav");
    let favs = engine
        .search(SearchQuery {
            filters: Some(SearchFilters {
                favorites_only: true,
                ..SearchFilters::default()
            }),
            ..text("hit")
        })
        .expect("favs");
    assert_eq!(favs.rows.len(), 1);
    let peaks = engine.waveform_peaks(hit.file_id, 200).expect("peaks");
    assert_eq!(peaks.len(), 200);
    assert!(peaks.iter().any(|p| p.max - p.min > 0.01));

    let clips = engine
        .search(SearchQuery {
            limit: 100,
            ..text("clip")
        })
        .expect("clips");
    let ogg = clips
        .rows
        .iter()
        .find(|r| r.path.ends_with(".ogg"))
        .expect("an ogg fixture");
    let pcm = engine.preview_pcm(ogg.file_id).expect("ogg pcm");
    assert!(pcm.sample_rate > 0 && pcm.channels == 1 && !pcm.samples.is_empty());
}

//! FFI similarity query (ADR-035) on the `sonar fixtures` library.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sonar_core::fixtures::{generate, FixtureOptions};

use crate::*;

fn query(seed: SimilarSeed) -> SimilarQuery {
    SimilarQuery {
        seed,
        weights: None,
        filters: None,
        limit: 100,
        sort: None,
        descending: false,
        region: None,
    }
}

fn region(seed: SimilarSeed, start_s: f64, end_s: f64) -> SimilarQuery {
    SimilarQuery {
        region: Some(SimilarRegion { start_s, end_s }),
        ..query(seed)
    }
}

/// Region seeds (ADR-040), library and external: the first 2 s of a loop
/// find that loop (not excluded), its first kick ranks a kick one-shot
/// above unrelated sounds, bad ranges are `InvalidInput`.
#[test]
fn region_seeds_find_the_loop_and_its_kick() {
    let (tmp, engine) = analyzed();
    let rel = "loop_002.wav"; // 2 bars of a 90 BPM backbeat, 5.33 s
    let path = tmp.path().join("fx/loops").join(rel);
    let file_id = browse(&engine, "loop")
        .iter()
        .find(|r| r.path.ends_with(rel))
        .expect("loop in the library")
        .file_id;
    let ext = || SimilarSeed::External {
        path: path.display().to_string(),
    };
    let name = |r: &FileRow| r.path.rsplit('/').take(2).collect::<Vec<_>>().join("<");
    let kick = |r: &FileRow| {
        ["hit_000.wav", "hit_001.wav", "hit_002.wav", "hit_003.wav"]
            .iter()
            .any(|k| r.path.ends_with(&format!("oneshots/{k}")))
    };
    for seed in [SimilarSeed::File { file_id }, ext()] {
        let page = engine
            .similar_to(region(seed.clone(), 0.0, 2.0))
            .expect("2 s region");
        let top3: Vec<String> = page.rows.iter().take(3).map(name).collect();
        assert!(
            page.rows.iter().take(3).any(|r| r.file_id == file_id),
            "{top3:?}"
        );
        // First kick: two sixteenths at 90 BPM.
        let page = engine
            .similar_to(region(seed.clone(), 0.0, 0.333))
            .expect("kick region");
        assert!(kick(&page.rows[0]), "{}", name(&page.rows[0]));
        let unrelated = page
            .rows
            .iter()
            .position(|r| !kick(r) && !r.path.contains("/loops/"));
        assert!(unrelated.is_some_and(|p| p >= 3), "{unrelated:?}");
        for (s, e) in [(1.0, 1.04), (5.0, 6.0), (-1.0, 1.0), (2.0, 1.0)] {
            assert!(
                matches!(
                    engine.similar_to(region(seed.clone(), s, e)),
                    Err(FfiError::InvalidInput { .. })
                ),
                "{s}–{e}"
            );
        }
    }
    assert!(matches!(
        engine.similar_to(region(SimilarSeed::File { file_id: 999_999 }, 0.0, 1.0)),
        Err(FfiError::NotFound { .. })
    ));
}

/// Fixtures library, fully analyzed; returns the tempdir guard too.
fn analyzed() -> (tempfile::TempDir, Arc<SonarEngine>) {
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
    let start = Instant::now();
    while engine.indexing_progress().running {
        assert!(start.elapsed() < Duration::from_secs(120), "indexing hangs");
        std::thread::sleep(Duration::from_millis(20));
    }
    (tmp, engine)
}

fn browse(engine: &SonarEngine, text: &str) -> Vec<FileRow> {
    engine
        .search(SearchQuery {
            text: text.into(),
            mode: Some(SearchMode::Text),
            filters: None,
            sort: None,
            descending: false,
            offset: 0,
            limit: 1000,
            prefix: true,
            facets: false,
        })
        .expect("search")
        .rows
}

#[test]
fn file_and_external_seeds_with_weights_and_filters() {
    let (tmp, engine) = analyzed();
    let rows = browse(&engine, "hit");
    let seed = rows
        .iter()
        .find(|r| r.kind.is_some() && r.duration_ms.is_some())
        .expect("an analyzed hit")
        .clone();

    // Default weights: ordered, aligned, seed excluded, limit honoured.
    let page = engine
        .similar_to(SimilarQuery {
            limit: 10,
            ..query(SimilarSeed::File {
                file_id: seed.file_id,
            })
        })
        .expect("similar");
    assert!(!page.rows.is_empty() && page.rows.len() <= 10);
    assert_eq!(page.rows.len(), page.hits.len());
    // A field sort reorders the same nearest files; hits stay aligned.
    let sorted = engine
        .similar_to(SimilarQuery {
            limit: 10,
            sort: Some(SortKey::Duration),
            descending: true,
            ..query(SimilarSeed::File {
                file_id: seed.file_id,
            })
        })
        .expect("similar sorted");
    let ids = |p: &SimilarPage| p.rows.iter().map(|r| r.file_id).collect::<Vec<_>>();
    let mut expected = page.rows.clone();
    expected.sort_by_key(|r| std::cmp::Reverse(r.duration_ms)); // stable: ties nearest first
    assert_eq!(
        ids(&sorted),
        expected.iter().map(|r| r.file_id).collect::<Vec<_>>()
    );
    assert_ne!(ids(&sorted), ids(&page), "fixtures differ in duration");
    assert_eq!(
        ids(&sorted),
        sorted.hits.iter().map(|h| h.file_id).collect::<Vec<_>>()
    );
    assert!(page.candidates >= page.rows.len() as u64);
    assert!(page.rows.iter().all(|r| r.file_id != seed.file_id));
    for (r, h) in page.rows.iter().zip(&page.hits) {
        assert_eq!(r.file_id, h.file_id);
    }
    assert!(page.hits.windows(2).all(|w| w[0].distance <= w[1].distance));

    // Pitch-only weights reorder by the pitch distance.
    let weights = aspect_weights_from_sliders(0.0, 0.0, 0.0, 1.0, 0.0).expect("weights");
    let by_pitch = engine
        .similar_to(SimilarQuery {
            weights: Some(weights),
            ..query(SimilarSeed::File {
                file_id: seed.file_id,
            })
        })
        .expect("pitch");
    assert!(by_pitch.hits.windows(2).all(|w| w[0].pitch <= w[1].pitch));
    assert!(by_pitch
        .hits
        .iter()
        .all(|h| (h.distance - h.pitch).abs() < 1e-4));

    // A kind filter restricts the candidates; the default (nil) keeps all.
    let all = engine
        .similar_to(query(SimilarSeed::File {
            file_id: seed.file_id,
        }))
        .expect("all");
    let kind = all.rows[0].kind.clone().expect("kind");
    let filtered = engine
        .similar_to(SimilarQuery {
            filters: Some(SearchFilters {
                kinds: vec![kind.clone()],
                ..SearchFilters::default()
            }),
            ..query(SimilarSeed::File {
                file_id: seed.file_id,
            })
        })
        .expect("filtered");
    assert!(!filtered.rows.is_empty() && filtered.candidates <= all.candidates);
    assert!(filtered
        .rows
        .iter()
        .all(|r| r.kind.as_deref() == Some(&kind)));

    // Hidden files drop out as in search.
    let top = all.rows[0].file_id;
    engine.set_hidden(vec![top], true).expect("hide");
    let after = engine
        .similar_to(query(SimilarSeed::File {
            file_id: seed.file_id,
        }))
        .expect("after hide");
    assert!(after.rows.iter().all(|r| r.file_id != top));

    // External seed: a copy of the seed file finds the original first.
    let copy = tmp.path().join("dropped.wav");
    fs::copy(&seed.path, &copy).expect("copy");
    let ext = engine
        .similar_to(query(SimilarSeed::External {
            path: copy.display().to_string(),
        }))
        .expect("external");
    assert_eq!(ext.rows[0].file_id, seed.file_id);
    assert!(ext.hits[0].distance < 0.05, "{}", ext.hits[0].distance);
}

#[test]
fn bad_seeds_and_weights_are_errors() {
    let (tmp, engine) = analyzed();
    let rows = browse(&engine, "hit");
    let id = rows[0].file_id;

    let corrupt = tmp.path().join("corrupt.wav");
    fs::write(&corrupt, b"RIFF not really a wav file at all").expect("write");
    assert!(matches!(
        engine.similar_to(query(SimilarSeed::External {
            path: corrupt.display().to_string()
        })),
        Err(FfiError::Audio { .. } | FfiError::Io { .. } | FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        engine.similar_to(query(SimilarSeed::External {
            path: tmp.path().join("missing.wav").display().to_string()
        })),
        Err(FfiError::Audio { .. } | FfiError::Io { .. } | FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        engine.similar_to(query(SimilarSeed::File { file_id: 999_999 })),
        Err(FfiError::NotFound { .. })
    ));

    for bad in [
        [0.0, 0.0, 0.0, 0.0],
        [-1.0, 1.0, 1.0, 1.0],
        [f32::NAN, 1.0, 1.0, 1.0],
        [f32::INFINITY, 1.0, 1.0, 1.0],
    ] {
        let w = AspectWeights {
            spectrum: bad[0],
            timbre: bad[1],
            pitch: bad[2],
            amplitude: bad[3],
        };
        assert!(matches!(
            engine.similar_to(SimilarQuery {
                weights: Some(w),
                ..query(SimilarSeed::File { file_id: id })
            }),
            Err(FfiError::InvalidInput { .. })
        ));
    }
    assert!(matches!(
        aspect_weights_from_sliders(0.0, 0.0, 0.0, 0.0, 0.0),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        aspect_weights_from_sliders(1.0, -0.5, 0.0, 0.0, 0.0),
        Err(FfiError::InvalidInput { .. })
    ));
    let plain = aspect_weights_from_sliders(1.0, 0.0, 0.0, 0.0, 0.0).expect("overall");
    assert_eq!(
        (plain.spectrum, plain.timbre, plain.pitch, plain.amplitude),
        (1.0, 1.0, 1.0, 1.0)
    );
}

#[test]
fn nothing_analyzed_and_cancel() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");
    let err = engine
        .similar_to(query(SimilarSeed::File { file_id: 1 }))
        .expect_err("no vectors yet");
    assert!(matches!(
        err,
        FfiError::NotFound { .. } | FfiError::Internal { .. }
    ));
    // A cancel with nothing in flight does not poison the next call.
    engine.cancel_search();
    assert!(engine
        .similar_to(query(SimilarSeed::File { file_id: 1 }))
        .is_err_and(|e| !matches!(e, FfiError::Cancelled)));
}

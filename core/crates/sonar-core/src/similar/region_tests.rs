//! Region seeds (ADR-040) on the fixture library: a selection of a loop
//! finds that loop, its first hit finds the matching one-shot, bad ranges
//! are error values, and a 2 s region is not slower to analyze than the
//! whole file.

use std::time::{Duration, Instant};

use super::tests::{family, lib, opts, run};
use super::*;
use crate::fixtures::manifest::{Fixture, Group};

/// Drum loops (not the click loops) with their tempo truth.
fn drum_loops() -> Vec<(i64, &'static Fixture)> {
    let lib = lib();
    let mut loops: Vec<_> = lib
        .fixtures
        .iter()
        .filter(|(&id, f)| f.group == Group::Loops && family(lib.fixture(id)) == "drum loop")
        .map(|(&id, f)| (id, f))
        .collect();
    loops.sort_by(|a, b| a.1.path.cmp(&b.1.path));
    loops
}

fn region(rel: &str, start_s: f64, end_s: f64) -> Seed {
    Seed::ExternalRegion {
        path: lib().root.join(rel),
        range: TimeRange::new(start_s, end_s),
    }
}

/// The first 2 s of a drum loop find that loop in the top 3 for most
/// loops (measured 11/13: ranks 1–2, plus 5 and 7 for the 100/110 BPM
/// 4-bar loops whose first 2 s are a smaller share of the file). The
/// whole file is a candidate: a region seed excludes nothing.
#[test]
fn two_second_region_of_a_loop_finds_the_loop() {
    let loops = drum_loops();
    assert_eq!(loops.len(), 13);
    let mut top3 = Vec::new();
    for &(id, f) in &loops {
        let r = run(
            region(&f.path, 0.0, 2.0),
            &opts(3, AspectWeights::default()),
        );
        assert_eq!(r.seed_file_id, None);
        if r.hits.iter().any(|h| h.file_id == id) {
            top3.push(f.path.as_str());
        }
    }
    assert!(top3.len() >= 11, "{top3:?}");
}

/// The first kick of a drum loop (two sixteenths: kick, wrapped tails,
/// a hat at step 0 in some patterns) ranks a kick one-shot first for every
/// drum loop, and the nearest kick above every unrelated one-shot or note
/// (snares, hats, sines, plucks, noise; measured: nearest non-kick
/// one-shot at rank ≥ 4).
#[test]
fn first_hit_of_a_loop_ranks_the_kick_one_shot_first() {
    let lib = lib();
    for (_, f) in drum_loops() {
        let bpm = f.truth.bpm.expect("loop tempo");
        let r = run(
            region(&f.path, 0.0, 2.0 * 15.0 / bpm),
            &opts(200, AspectWeights::default()),
        );
        let fams: Vec<&str> = r
            .hits
            .iter()
            .map(|h| family(lib.fixture(h.file_id)))
            .collect();
        assert_eq!(fams.first(), Some(&"kick"), "{}: {fams:?}", f.path);
        let unrelated = fams.iter().position(|&x| {
            matches!(
                x,
                "snare" | "hat" | "sine" | "sine burst" | "pluck" | "noise"
            )
        });
        assert!(unrelated.is_some_and(|p| p >= 3), "{}: {fams:?}", f.path);
    }
}

/// Library regions resolve through the engine only; out-of-range, empty
/// and malformed regions are `InvalidQuery`, never a panic.
#[test]
fn bad_regions_are_error_values() {
    let lib = lib();
    let vs = lib.vs();
    let o = opts(3, AspectWeights::default());
    let rel = "loops/loop_000.wav"; // 2 bars at 70 BPM = 6.857 s
    let invalid = |seed: Seed| match similar(&vs, &lib.index, &seed, &o) {
        Err(Error::InvalidQuery { reason }) => reason,
        other => panic!("{seed:?}: {other:?}"),
    };
    assert!(invalid(region(rel, 1.0, 1.00001)).contains("shorter than one sample"));
    assert!(invalid(region(rel, 6.0, 7.5)).contains("past the end"));
    assert!(invalid(region(rel, 7.0, 8.0)).contains("past the end"));
    assert!(invalid(region(rel, -0.5, 1.0)).contains("negative"));
    assert!(invalid(region(rel, 2.0, 1.0)).contains("not after"));
    assert!(invalid(region(rel, f64::NAN, 1.0)).contains("finite"));
    // An end a few ms past the file (waveform rounding) is clamped.
    assert!(similar(&vs, &lib.index, &region(rel, 6.0, 6.865), &o).is_ok());
    // Clamping 6.83–6.865 to 6.857 leaves 27 ms: a (widened) seed.
    assert!(similar(&vs, &lib.index, &region(rel, 6.83, 6.865), &o).is_ok());
    // Clamping 6.86–6.865 to 6.857 leaves nothing.
    assert!(invalid(region(rel, 6.86, 6.865)).contains("not after"));
    let file_region = Seed::FileRegion {
        file_id: lib.id_of(rel),
        range: TimeRange::new(0.0, 1.0),
    };
    assert!(invalid(file_region).contains("resolve"));
    let missing = region("loops/nope.wav", 0.0, 1.0);
    assert!(matches!(
        similar(&vs, &lib.index, &missing, &o),
        Err(Error::Io { .. } | Error::Decode { .. })
    ));
}

/// Median wall time of `n` runs of `f`.
fn median(n: usize, mut f: impl FnMut()) -> Duration {
    let mut t: Vec<Duration> = (0..n)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    t.sort_unstable();
    t[n / 2]
}

/// Seed analysis of a 2 s region (4–6 s of a 9.6 s loop) is not slower
/// than the whole-file external seed it replaces (no separate budget
/// exists; ADR-024's external seed is the reference). Measured (debug
/// build, M4): whole file 390 ms, 2 s region 158 ms; release 6.7 ms vs
/// 2.4 ms (decoding still covers 0–6 s; the analysis covers 2 s).
#[test]
fn two_second_region_analysis_is_not_slower_than_the_whole_file() {
    let path = lib().root.join("loops/loop_003.wav"); // 4 bars at 100 BPM
    let range = TimeRange::new(4.0, 6.0);
    let whole = median(5, || {
        seed::analyze_file(&path, None).expect("whole file");
    });
    let part = median(5, || {
        region::analyze_region(&path, range, None).expect("region");
    });
    println!("seed analysis: whole 9.6 s file {whole:?}, 2 s region {part:?}");
    // 1.5× slack: parallel tests can load one batch more than the other
    // (measured margin is ~2.5× the other way).
    assert!(part <= whole * 3 / 2, "region {part:?} vs whole {whole:?}");
}

/// A 1 ms and a 10 ms selection (T13.1) are seeds, at the start and in
/// the middle of a loop: each returns a full page of finite distances
/// (how good they are is measured by `sonar eval --short-regions`).
#[test]
fn one_and_ten_ms_regions_return_results() {
    let o = opts(10, AspectWeights::default());
    let rel = "loops/loop_000.wav";
    for (start, end) in [(0.0, 0.001), (0.0, 0.01), (3.0, 3.001), (3.0, 3.01)] {
        let r = run(region(rel, start, end), &o);
        assert_eq!(r.hits.len(), 10, "{rel} {start}..{end}");
        assert!(r.hits.iter().all(|h| h.distance.is_finite()));
    }
}

/// Aspects of the analysis-rate samples `lo..hi` of `path`, analysed as
/// is.
fn span_aspects(path: &std::path::Path, lo: usize, hi: Option<usize>) -> AspectVectors {
    crate::jobs::work::with_workers(|w| {
        let mut d = w
            .decoder
            .decode(path, &crate::decode::DecodeOptions::default())
            .expect("decode");
        let hi = hi.unwrap_or(d.samples.len());
        d.samples = d.samples[lo..hi].to_vec();
        crate::jobs::work::window_aspects(w, &d).expect("aspects")
    })
}

/// Strategy A ("context") widens a region at the very start of a file to
/// the right only and one at the very end to the left only: the analysed
/// samples are exactly the first / last `W`.
#[test]
fn context_widens_inward_at_the_file_edges() {
    use super::widen::{widen_samples, Widen};
    let path = lib().root.join("loops/loop_000.wav");
    let w = widen_samples(crate::decode::ANALYSIS_RATE);
    let at_start =
        region::analyze_region_with(&path, TimeRange::new(0.0, 0.01), None, Widen::Context)
            .expect("start region");
    assert_eq!(at_start, span_aspects(&path, 0, Some(w)));
    let n = crate::decode::decode_file(&path, &crate::decode::DecodeOptions::default())
        .expect("decode")
        .samples
        .len();
    let dur = n as f64 / f64::from(crate::decode::ANALYSIS_RATE);
    let at_end =
        region::analyze_region_with(&path, TimeRange::new(dur - 0.01, dur), None, Widen::Context)
            .expect("end region");
    assert_eq!(at_end, span_aspects(&path, n - w, None));
    // The other strategies analyse something else.
    let none = region::analyze_region_with(&path, TimeRange::new(0.0, 0.01), None, Widen::None)
        .expect("none");
    assert_ne!(none, at_start);
}

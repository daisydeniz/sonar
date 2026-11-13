//! FFI duplicate finder (ADR-036): exact copies, a format variant, errors.

use std::fs;
use std::time::{Duration, Instant};

use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};
use sonar_core::fixtures::{generate, FixtureOptions};

use crate::*;

/// A decaying 440 Hz tone (`secs` long) at `rate` Hz, `gain` linear.
fn tone(rate: u32, secs: f32, gain: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            gain * (-t * 3.0).exp() * (std::f32::consts::TAU * 440.0 * t).sin()
        })
        .collect()
}

/// Near distances are z-scored with library stats, so the library is the
/// fixture set (varied sounds) plus the test files in `mine/`.
#[test]
fn exact_and_near_groups_and_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("lib");
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    generate(&root.join("fx"), &opts).expect("fixtures");
    let lib = root.join("mine");
    fs::create_dir(&lib).expect("mkdir");
    let a = encode(
        &WavSpec::new(44_100, 1, SampleFormat::Int16),
        &tone(44_100, 1.0, 0.8),
    )
    .bytes;
    fs::write(lib.join("a.wav"), &a).expect("write");
    fs::write(lib.join("a copy.wav"), &a).expect("write");
    let b = tone(48_000, 1.0, 0.4);
    let b = encode(&WavSpec::new(48_000, 1, SampleFormat::Float32), &b).bytes;
    fs::write(lib.join("a 48k.wav"), b).expect("write");
    let c = encode(
        &WavSpec::new(44_100, 1, SampleFormat::Int16),
        &tone(44_100, 2.5, 0.8),
    )
    .bytes;
    fs::write(lib.join("other.wav"), c).expect("write");

    let engine = SonarEngine::open(tmp.path().join("home").display().to_string()).expect("open");
    engine.add_library(root.display().to_string()).expect("add");
    let all = DuplicatesQuery {
        exact: true,
        near: true,
        threshold: None,
    };
    let mine = |g: &DuplicateGroup| {
        let rows = engine.rows(g.file_ids.clone()).expect("rows");
        let mut names: Vec<String> = rows
            .iter()
            .filter(|r| r.folders.ends_with("mine"))
            .map(|r| r.name.clone())
            .collect();
        names.sort_unstable();
        names
    };
    // Before analysis: exact groups only.
    let r = engine.duplicates(all).expect("dupes");
    assert_eq!(r.near_files, 0);
    assert!(r.groups.iter().all(|g| g.kind == DuplicateKind::Exact));
    let copies = r
        .groups
        .iter()
        .find(|g| !mine(g).is_empty())
        .expect("copies");
    assert_eq!(mine(copies), ["a", "a copy"]);
    assert_eq!(copies.wasted_bytes, a.len() as u64);

    engine.start_indexing(false).expect("start");
    let start = Instant::now();
    while engine.indexing_progress().running {
        assert!(start.elapsed() < Duration::from_secs(120), "indexing hangs");
        std::thread::sleep(Duration::from_millis(20));
    }
    let r = engine.duplicates(all).expect("dupes");
    let near: Vec<&DuplicateGroup> = r
        .groups
        .iter()
        .filter(|g| g.kind == DuplicateKind::Near && !mine(g).is_empty())
        .collect();
    assert_eq!(near.len(), 1, "{r:?}");
    // One of the two copies stands for both; "other" (longer) is not in.
    let names = mine(near[0]);
    assert_eq!(names.len(), 2, "{names:?}");
    assert_eq!(names[0], "a 48k");
    assert!(near[0].max_distance <= r.threshold);
    assert!(r.near_files > 100);
    assert_eq!(r.hashed, 0, "hashes cached by the first run");

    let bad = DuplicatesQuery {
        threshold: Some(2.0),
        ..all
    };
    assert!(matches!(
        engine.duplicates(bad),
        Err(FfiError::InvalidInput { .. })
    ));
    engine.cancel_duplicates();
    assert!(engine.duplicates(all).is_ok(), "a new call has a new flag");
}

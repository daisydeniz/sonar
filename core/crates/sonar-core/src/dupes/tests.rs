//! Duplicate finder tests: the analyzed fixture library with test-time
//! variants ([`super::test_lib`]) and a small unanalyzed library for file
//! states (missing, offline, unreadable).

use std::fs;

use super::test_lib::{lib, sounds, VARIANTS};
use super::*;
use crate::index::vectors::dir_for_db;
use crate::scan::scan_library;

fn run(opts: &DupesOptions) -> DupesReport {
    let lib = lib();
    let store = Store::open(&lib.db).expect("store");
    let vs = VectorStore::open_read(dir_for_db(&lib.db)).expect("vectors");
    find_duplicates(&store, Some(&vs), opts).expect("dupes")
}

fn variant_ids(sound: &str) -> Vec<i64> {
    let lib = lib();
    let mut ids: Vec<i64> = VARIANTS
        .iter()
        .map(|v| match *v {
            "copy" => format!("dupes/{sound}/copy/orig.wav"),
            "flac16" => format!("dupes/{sound}/flac16.flac"),
            v => format!("dupes/{sound}/{v}.wav"),
        })
        .map(|p| lib.id_of(&p))
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn copies_are_exact_groups_and_hashes_are_cached() {
    let lib = lib();
    let r = run(&DupesOptions {
        near: false,
        ..DupesOptions::default()
    });
    assert!(r.groups.iter().all(|g| g.kind == DupeKind::Exact));
    for (sound, _) in sounds() {
        let pair = [
            lib.id_of(&format!("dupes/{sound}/orig.wav")),
            lib.id_of(&format!("dupes/{sound}/copy/orig.wav")),
        ];
        let g = r
            .groups
            .iter()
            .find(|g| g.file_ids.contains(&pair[0]))
            .unwrap_or_else(|| panic!("{sound}: no exact group"));
        let mut want = pair.to_vec();
        want.sort_unstable();
        assert_eq!(g.file_ids, want, "{sound}");
        assert_eq!(g.wasted_bytes, g.sizes[0]);
        assert_eq!(g.max_distance, 0.0);
    }
    // Cached: a second run hashes nothing and finds the same groups.
    let again = run(&DupesOptions {
        near: false,
        ..DupesOptions::default()
    });
    assert_eq!(again.hashed, 0);
    assert_eq!(again.groups, r.groups);
    let store = Store::open(&lib.db).expect("store");
    let cached: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM files WHERE length(content_hash) = 16",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert!(cached as usize >= r.exact_candidates - r.skipped);
}

#[test]
fn format_rate_gain_and_padding_variants_are_one_near_group() {
    let r = run(&DupesOptions::default());
    for (sound, _) in sounds() {
        let all = variant_ids(sound);
        let near: Vec<&DupeGroup> = r
            .groups
            .iter()
            .filter(|g| g.kind == DupeKind::Near && g.file_ids.iter().any(|id| all.contains(id)))
            .collect();
        assert_eq!(near.len(), 1, "{sound}: {near:?}");
        let g = near[0];
        // Every variant except the exact copy that the other copy stands for.
        assert_eq!(g.file_ids.len(), all.len() - 1, "{sound}: {g:?}");
        assert!(g.file_ids.iter().all(|id| all.contains(id)), "{sound}");
        assert!(g.max_distance <= DEFAULT_NEAR_THRESHOLD * 1.5, "{g:?}");
        assert!(g.mean_distance <= g.max_distance);
    }
    // The fixture set's own 1 s A4 sines (WAV matrix, AIFF, FLAC at
    // 11 kHz, ALAC, the -60 dBFS one) are one near group too.
    let lib = lib();
    let a4 = r
        .groups
        .iter()
        .filter(|g| g.file_ids.iter().all(|id| lib.labels[id] == "a4-sine-1s"))
        .max_by_key(|g| g.file_ids.len())
        .expect("A4 group");
    assert!(a4.file_ids.len() >= 22, "{}", a4.file_ids.len());
}

#[test]
fn different_sounds_never_share_a_group() {
    let lib = lib();
    let r = run(&DupesOptions::default());
    assert!(!r.groups.is_empty());
    for g in &r.groups {
        let first = &lib.labels[&g.file_ids[0]];
        for id in &g.file_ids {
            assert_eq!(&lib.labels[id], first, "{:?}: {}", g.kind, lib.paths[id]);
        }
    }
    // No file in two groups of the same kind, none in a silence group.
    for kind in [DupeKind::Exact, DupeKind::Near] {
        let mut ids: Vec<i64> = r
            .groups
            .iter()
            .filter(|g| g.kind == kind)
            .flat_map(|g| g.file_ids.iter().copied())
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "{kind:?}");
    }
    assert!(r
        .groups
        .iter()
        .filter(|g| g.kind == DupeKind::Near)
        .all(|g| lib.labels[&g.file_ids[0]] != "silence"));
    assert!(r.near_pairs > 0 && r.near_files > 100);
}

#[test]
fn order_is_deterministic_by_wasted_bytes() {
    let a = run(&DupesOptions::default());
    let b = run(&DupesOptions::default());
    assert_eq!(a.groups, b.groups);
    for w in a.groups.windows(2) {
        assert!(w[0].wasted_bytes >= w[1].wasted_bytes);
    }
    for g in &a.groups {
        assert!(g.file_ids.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(g.file_ids.len(), g.sizes.len());
    }
    let near_only = run(&DupesOptions {
        exact: false,
        ..DupesOptions::default()
    });
    assert!(near_only.groups.iter().all(|g| g.kind == DupeKind::Near));
    assert_eq!(near_only.exact_candidates, 0);
}

#[test]
fn bad_threshold_and_cancel_are_errors() {
    let lib = lib();
    let store = Store::open(&lib.db).expect("store");
    for t in [-0.1, f32::NAN, MAX_NEAR_THRESHOLD + 0.1] {
        let opts = DupesOptions {
            threshold: t,
            ..DupesOptions::default()
        };
        let err = find_duplicates(&store, None, &opts).expect_err("threshold");
        assert!(matches!(err, Error::InvalidQuery { .. }), "{t}");
    }
    let vs = VectorStore::open_read(dir_for_db(&lib.db)).expect("vectors");
    let opts = DupesOptions {
        cancel: Some(Arc::new(AtomicBool::new(true))),
        ..DupesOptions::default()
    };
    let err = find_duplicates(&store, Some(&vs), &opts).expect_err("cancelled");
    assert!(matches!(err, Error::Cancelled));
}

/// Three identical files and one other, scanned but not analyzed.
#[test]
fn missing_offline_and_unreadable_files_are_left_out() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("lib");
    fs::create_dir(&root).expect("mkdir");
    let bytes = vec![7u8; 5000];
    for name in ["a.wav", "b.wav", "c.wav"] {
        fs::write(root.join(name), &bytes).expect("write");
    }
    fs::write(root.join("d.wav"), vec![8u8; 5000]).expect("write");
    fs::write(root.join("empty1.wav"), b"").expect("write");
    fs::write(root.join("empty2.wav"), b"").expect("write");
    let mut store = Store::open(tmp.path().join("sonar.db")).expect("store");
    let library = store.add_library(&root).expect("library");
    scan_library(&mut store, library.id).expect("scan");
    let id = |name: &str| {
        store
            .file_id_by_path(&library.root_path.join(name))
            .expect("lookup")
            .expect("known")
    };
    let (a, b, c) = (id("a.wav"), id("b.wav"), id("c.wav"));
    let r = find_duplicates(&store, None, &DupesOptions::default()).expect("dupes");
    assert_eq!(r.files, 6);
    assert_eq!(r.groups.len(), 1, "zero-byte files are not grouped");
    assert_eq!(r.groups[0].file_ids, [a, b, c]);
    assert_eq!(r.groups[0].wasted_bytes, 10_000);
    assert_eq!((r.exact_candidates, r.hashed, r.near_files), (3, 3, 0));

    // A missing file is left out; a vanished file (not rescanned yet) is
    // skipped, not an error; cached hashes are not re-read.
    store
        .conn()
        .execute("UPDATE files SET status = 'missing' WHERE id = ?1", [a])
        .expect("mark missing");
    fs::remove_file(root.join("c.wav")).expect("rm");
    store
        .conn()
        .execute("UPDATE files SET content_hash = NULL WHERE id = ?1", [c])
        .expect("drop cache");
    let r = find_duplicates(&store, None, &DupesOptions::default()).expect("dupes");
    assert_eq!(r.files, 5);
    assert!(r.groups.is_empty(), "{:?}", r.groups);
    assert_eq!((r.exact_candidates, r.hashed, r.skipped), (2, 0, 1));

    store
        .conn()
        .execute("UPDATE libraries SET online = 0", [])
        .expect("offline");
    let r = find_duplicates(&store, None, &DupesOptions::default()).expect("dupes");
    assert_eq!((r.files, r.groups.len()), (0, 0));
}

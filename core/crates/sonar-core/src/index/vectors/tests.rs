//! Vector store tests on temp directories.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use super::*;
use crate::dsp::aspects::{OVERALL_DIM, PITCH_DIM, SPECTRUM_DIM};

/// Deterministic raw vectors with awkward values (−0, subnormal, huge).
fn vecs(seed: u32) -> AspectVectors {
    let make = |dim: usize, salt: u32| -> Vec<f32> {
        (0..dim as u32)
            .map(|i| match (seed + i + salt) % 7 {
                0 => -0.0,
                1 => f32::MIN_POSITIVE / 3.0,
                2 => 1.0e30,
                _ => (seed as f32 * 0.37 + i as f32 * 1.13 + salt as f32).sin() * 5.0,
            })
            .collect()
    };
    AspectVectors {
        spectrum: make(Aspect::Spectrum.dim(), 1),
        timbre: make(Aspect::Timbre.dim(), 2),
        pitch: make(Aspect::Pitch.dim(), 3),
        amplitude: make(Aspect::Amplitude.dim(), 4),
    }
}

fn bits(v: &AspectVectors) -> Vec<u32> {
    v.parts()
        .iter()
        .flat_map(|(_, x)| x.iter().map(|f| f.to_bits()))
        .collect()
}

fn writer(dir: &Path) -> (VectorStore, OpenState) {
    VectorStore::open(dir).expect("open writer")
}

fn append_ids(store: &mut VectorStore, ids: &[i64]) {
    let owned: Vec<(i64, AspectVectors)> = ids.iter().map(|&i| (i, vecs(i as u32))).collect();
    let batch: Vec<(i64, &AspectVectors)> = owned.iter().map(|(i, v)| (*i, v)).collect();
    store.append(&batch).expect("append");
}

fn gen_file(store: &VectorStore, name: &str) -> PathBuf {
    generation::gen_dir(store.dir(), store.generation).join(name)
}

/// Stats from pushing every vector of `ids` in order.
fn reference_norm(ids: &[i64]) -> LibraryNorm {
    let mut n = LibraryNorm::new();
    for &i in ids {
        n.push(&vecs(i as u32)).expect("push");
    }
    n
}

fn assert_norm_close(a: &LibraryNorm, b: &LibraryNorm) {
    assert_eq!(a.count(), b.count());
    let pairs = [
        (&a.spectrum, &b.spectrum),
        (&a.timbre, &b.timbre),
        (&a.pitch, &b.pitch),
        (&a.amplitude, &b.amplitude),
    ];
    for (x, y) in pairs {
        let close = |p: f64, q: f64| (p - q).abs() <= 1e-9 * p.abs().max(q.abs()).max(1.0);
        for (p, q) in x.mean().iter().zip(y.mean()) {
            assert!(close(*p, *q), "mean {p} vs {q}");
        }
        for (p, q) in x.std().into_iter().zip(y.std()) {
            assert!(close(p, q), "std {p} vs {q}");
        }
    }
}

#[test]
fn append_read_roundtrip_is_bitwise_and_survives_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("vectors");
    let (mut s, state) = writer(&dir);
    assert_eq!(state, OpenState::New);
    append_ids(&mut s, &[3, 1, 2]);
    append_ids(&mut s, &[10, 11]);
    assert_eq!((s.rows(), s.live_rows()), (5, 5));
    let check = |s: &VectorStore| {
        for id in [3, 1, 2, 10, 11] {
            let got = s.vectors(id).expect("vectors");
            assert_eq!(bits(&got), bits(&vecs(id as u32)), "file {id}");
        }
        assert_eq!(s.row_of(10), Some(3));
        assert_eq!(s.file_id_at(1), Some(1));
        assert_eq!(
            s.matrix(Aspect::Pitch).map(<[f32]>::len),
            Some(5 * PITCH_DIM)
        );
        assert_eq!(s.matrix(Aspect::Overall), None);
        assert_eq!(s.vectors(99), None);
        let z = s.normalized(2).expect("normalized");
        assert_eq!(z.overall.len(), OVERALL_DIM);
        assert!(z.overall.iter().all(|x| x.is_finite()));
    };
    check(&s);
    drop(s);
    let (s, state) = writer(&dir);
    assert_eq!(state, OpenState::Ready);
    check(&s);
    let r = VectorStore::open_read(&dir).expect("reader");
    check(&r);
    assert_eq!(r.stats().live, 5);
}

#[test]
fn reappend_supersedes_and_remove_tombstones() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _) = writer(tmp.path());
    append_ids(&mut s, &[5, 6]);
    let newer = vecs(77);
    s.append(&[(5, &newer), (7, &vecs(7)), (7, &vecs(8))])
        .expect("append");
    assert_eq!((s.rows(), s.live_rows()), (5, 3));
    assert_eq!(s.id_slots()[0], TOMBSTONE);
    assert_eq!(s.file_id_at(0), None);
    assert_eq!(s.row_of(7), Some(4));
    assert_eq!(bits(&s.vectors(5).unwrap()), bits(&newer));
    assert_eq!(bits(&s.vectors(7).unwrap()), bits(&vecs(8)));
    assert_eq!(s.remove([6, 42]).expect("remove"), 1);
    drop(s);
    let (s, _) = writer(tmp.path());
    assert_eq!((s.rows(), s.live_rows()), (5, 2));
    assert_eq!(s.vectors(6), None);
    assert_eq!(bits(&s.vectors(5).unwrap()), bits(&newer));
    assert_eq!(s.stats().dead, 3);
}

#[test]
fn compaction_keeps_live_rows_and_mapping_and_recomputes_stats() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _) = writer(tmp.path());
    let ids: Vec<i64> = (1..=40).collect();
    append_ids(&mut s, &ids);
    append_ids(&mut s, &[4, 8]); // supersede
    s.remove([1, 2, 3]).expect("remove");
    let keep: HashSet<i64> = (1..=30).collect();
    assert_eq!(s.retain(&keep).expect("retain"), 10);
    let old_gen = generation::gen_dir(s.dir(), s.generation);
    let live: Vec<i64> = (4..=30).collect();
    let report = s.compact().expect("compact");
    assert_eq!((report.rows_before, report.rows_after), (42, 27));
    assert_eq!((s.rows(), s.live_rows(), s.stats().dead), (27, 27, 0));
    assert!(!old_gen.exists());
    for id in 1..=40 {
        let got = s.vectors(id).map(|v| bits(&v));
        let want = live.contains(&id).then(|| bits(&vecs(id as u32)));
        assert_eq!(got, want, "file {id}");
    }
    // Rows keep their relative order; superseded 4 and 8 moved to the end.
    assert_eq!(s.row_of(5), Some(0));
    assert_eq!(s.row_of(8), Some(26));
    assert_norm_close(s.norm(), &reference_norm(&live));
    drop(s);
    let (mut s, state) = writer(tmp.path());
    assert_eq!((state, s.rows()), (OpenState::Ready, 27));
    assert_norm_close(s.norm(), &reference_norm(&live));
    append_ids(&mut s, &[99]);
    assert_eq!(s.row_of(99), Some(27));
}

#[test]
fn torn_appends_are_cut_back_to_the_committed_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _) = writer(tmp.path());
    append_ids(&mut s, &[1, 2, 3, 4, 5]);
    let ids = gen_file(&s, "ids.i64");
    let spectrum = gen_file(&s, "spectrum.f32");
    drop(s);
    // Rows written past the count, headers not bumped (kill -9 mid-append),
    // plus one header bumped before the others.
    let f = OpenOptions::new().write(true).open(&ids).unwrap();
    f.write_all_at(&[7u8; 8 * 3], 64 + 5 * 8).unwrap();
    f.write_all_at(&6u64.to_le_bytes(), 32).unwrap();
    let g = OpenOptions::new().write(true).open(&spectrum).unwrap();
    g.write_all_at(
        &[1u8; 4 * SPECTRUM_DIM + 6],
        64 + 5 * 4 * SPECTRUM_DIM as u64,
    )
    .unwrap();
    let r = VectorStore::open_read(tmp.path()).expect("reader");
    assert_eq!(r.rows(), 5);
    drop(r);
    let (mut s, _) = writer(tmp.path());
    assert_eq!((s.rows(), s.live_rows()), (5, 5));
    assert_eq!(fs::metadata(&ids).unwrap().len(), 64 + 5 * 8);
    assert_eq!(
        fs::metadata(&spectrum).unwrap().len(),
        64 + 5 * 4 * SPECTRUM_DIM as u64
    );
    append_ids(&mut s, &[6]);
    for id in 1..=6 {
        assert_eq!(bits(&s.vectors(id).unwrap()), bits(&vecs(id as u32)));
    }
    drop(s);
    // A file cut mid-row: the committed count drops to its whole rows.
    let f = OpenOptions::new().write(true).open(&spectrum).unwrap();
    f.set_len(64 + (3 * SPECTRUM_DIM as u64 + 5) * 4).unwrap();
    let (s, _) = writer(tmp.path());
    assert_eq!((s.rows(), s.live_rows()), (3, 3));
    assert_eq!(fs::metadata(&ids).unwrap().len(), 64 + 3 * 8);
    assert_norm_close(s.norm(), &reference_norm(&[1, 2, 3]));
}

#[test]
fn version_dims_or_magic_mismatch_is_an_error_not_a_panic() {
    // (offset, value): dims, aspects version, format version, magic.
    for (offset, value) in [(16u64, 999u32), (24, 12345), (8, 77), (0, 0)] {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = writer(tmp.path());
        append_ids(&mut s, &[1, 2]);
        let timbre = gen_file(&s, "timbre.f32");
        drop(s);
        let f = OpenOptions::new().write(true).open(&timbre).unwrap();
        f.write_all_at(&value.to_le_bytes(), offset).unwrap();
        let err = VectorStore::open_read(tmp.path()).expect_err("mismatch");
        assert!(matches!(err, Error::VectorStore { .. }), "{err:?}");
        let (s, state) = writer(tmp.path());
        assert!(matches!(state, OpenState::Reset { .. }), "{state:?}");
        assert_eq!(s.rows(), 0);
        assert_eq!(s.generation, 2);
    }
}

#[test]
fn stats_persist_and_catch_up_after_a_crash() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _) = writer(tmp.path());
    append_ids(&mut s, &[1, 2, 3]);
    let norm_path = gen_file(&s, "norm.json");
    let after_first = fs::read(&norm_path).unwrap();
    append_ids(&mut s, &[4, 5]);
    assert_norm_close(s.norm(), &reference_norm(&[1, 2, 3, 4, 5]));
    drop(s);
    let (s, _) = writer(tmp.path());
    assert_norm_close(s.norm(), &reference_norm(&[1, 2, 3, 4, 5]));
    drop(s);
    // Crash after the rows were committed, before norm.json was saved.
    fs::write(&norm_path, &after_first).unwrap();
    let (s, _) = writer(tmp.path());
    assert_norm_close(s.norm(), &reference_norm(&[1, 2, 3, 4, 5]));
    drop(s);
    fs::write(&norm_path, b"{ damaged").unwrap();
    let r = VectorStore::open_read(tmp.path()).expect("reader");
    assert_norm_close(r.norm(), &reference_norm(&[1, 2, 3, 4, 5]));
}

#[test]
fn bad_input_writes_nothing_and_writers_are_exclusive() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _) = writer(tmp.path());
    append_ids(&mut s, &[1]);
    let mut short = vecs(2);
    short.pitch.pop();
    assert!(s.append(&[(2, &vecs(2)), (3, &short)]).is_err());
    assert!(s.append(&[(0, &vecs(2))]).is_err());
    assert_eq!((s.rows(), s.norm().count()), (1, 1));
    let second = VectorStore::open(tmp.path()).expect_err("locked");
    assert!(matches!(second, Error::VectorStore { .. }), "{second:?}");
    let mut r = VectorStore::open_read(tmp.path()).expect("reader alongside writer");
    assert!(r.append(&[(4, &vecs(4))]).is_err());
    assert!(r.compact().is_err());
    drop(s);
    assert!(VectorStore::open(tmp.path()).is_ok());
}

#[test]
fn reader_without_store_and_stale_generations() {
    let tmp = tempfile::tempdir().unwrap();
    let err = VectorStore::open_read(tmp.path()).expect_err("no store");
    assert!(matches!(err, Error::VectorStore { .. }));
    let (s, _) = writer(tmp.path());
    drop(s);
    // Leftover of a compaction that crashed before switching CURRENT.
    let stale = generation::gen_dir(tmp.path(), 9);
    fs::create_dir(&stale).unwrap();
    fs::write(tmp.path().join("keep-me"), b"x").unwrap();
    let (s, state) = writer(tmp.path());
    assert_eq!((state, s.generation), (OpenState::Ready, 1));
    assert!(!stale.exists() && tmp.path().join("keep-me").exists());
    assert_eq!(
        dir_for_db(Path::new("/d/sonar.db")),
        Path::new("/d/index/vectors")
    );
}

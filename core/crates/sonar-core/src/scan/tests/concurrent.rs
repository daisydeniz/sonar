//! Concurrent scans of one library are serialized.

use super::*;

#[test]
fn concurrent_scans_of_one_library_both_succeed_without_duplicates() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("lib");
    fs::create_dir(&root).expect("mkdir");
    for i in 0..200usize {
        fs::write(root.join(format!("s{i}.wav")), vec![7u8; 64 + i]).expect("write");
    }
    let db = tmp.path().join("db.sqlite");
    let id = Store::open(&db)
        .expect("store")
        .add_library(&root)
        .expect("add")
        .id;

    let results: Vec<_> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    let mut store = Store::open(&db).expect("open");
                    scan_library(&mut store, id)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("join")).collect()
    });
    for r in &results {
        assert!(r.is_ok(), "scan failed: {r:?}");
    }
    let added: usize = results
        .iter()
        .map(|r| r.as_ref().expect("ok").counts.added)
        .sum();
    assert_eq!(added, 200, "exactly one scan adds the files");
    let store = Store::open(&db).expect("open");
    let n: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM files WHERE library_id = ?1",
            [id],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(n, 200);
}

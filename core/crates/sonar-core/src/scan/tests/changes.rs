//! Changed content, deletion and reappearance at the same path.

use super::*;

#[test]
fn size_change_resets_stage_data_but_keeps_user_data() {
    let mut lib = Lib::new();
    lib.write("loop.wav", &[1; 300]);
    lib.write("other.wav", &[2; 300]);
    lib.scan();
    let before = lib.must("loop.wav");
    lib.decorate(before.id);
    assert_eq!(lib.attached(before.id), [1, 1, 1, 1, 1]);

    lib.write("loop.wav", &[1; 400]);
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 1, 0, 0, 1));

    let after = lib.must("loop.wav");
    assert_eq!(after.id, before.id, "same row");
    assert_eq!(after.size, 400);
    assert_eq!(after.status, "new");
    assert_ne!(after.quick_hash, before.quick_hash);
    assert_eq!(
        lib.attached(after.id),
        [0, 0, 1, 1, 1],
        "meta/analysis dropped"
    );
    let version: i64 = lib
        .store
        .conn()
        .query_row(
            "SELECT analysis_version FROM files WHERE id = ?1",
            [after.id],
            |r| r.get(0),
        )
        .expect("version");
    assert_eq!(version, 0);
}

#[test]
fn same_size_new_mtime_is_changed() {
    let mut lib = Lib::new();
    lib.write("hat.wav", b"aaaa");
    lib.set_mtime("hat.wav", 1_000_000);
    lib.scan();
    let before = lib.must("hat.wav");
    lib.decorate(before.id);

    lib.write("hat.wav", b"bbbb");
    lib.set_mtime("hat.wav", 1_000_010);
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 1, 0, 0, 0));
    let after = lib.must("hat.wav");
    assert_eq!((after.id, after.size), (before.id, 4));
    assert_eq!(after.mtime, 1_000_010 * 1_000_000_000);
    assert_ne!(after.quick_hash, before.quick_hash);
    assert_eq!(lib.attached(after.id), [0, 0, 1, 1, 1]);

    // Touch only (same bytes, new mtime): still treated as changed, since
    // the quick hash does not cover the whole file.
    lib.set_mtime("hat.wav", 1_000_020);
    assert_eq!(lib.scan().counts, counts(0, 1, 0, 0, 0));
    assert_eq!(lib.must("hat.wav").quick_hash, after.quick_hash);
}

#[test]
fn deleted_file_goes_missing_and_keeps_everything() {
    let mut lib = Lib::new();
    lib.write("snare.wav", b"snare");
    lib.write("kick.wav", b"kick");
    lib.scan();
    let id = lib.must("snare.wav").id;
    lib.decorate(id);

    fs::remove_file(lib.path("snare.wav")).expect("rm");
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, 0, 1, 1));
    let row = lib.must("snare.wav");
    assert_eq!((row.id, row.status.as_str()), (id, "missing"));
    assert_eq!(lib.attached(id), [1; 5]);

    // Still gone: not counted again.
    assert_eq!(lib.scan().counts, counts(0, 0, 0, 0, 1));
}

#[test]
fn reappearing_file_is_restored_with_its_status() {
    let mut lib = Lib::new();
    lib.write("snare.wav", b"snare");
    lib.set_mtime("snare.wav", 2_000_000);
    lib.write("plain.wav", b"plain");
    lib.set_mtime("plain.wav", 2_000_000);
    lib.scan();
    let snare = lib.must("snare.wav").id;
    let plain = lib.must("plain.wav").id;
    lib.decorate(snare);

    let stash = lib.outside();
    fs::rename(lib.path("snare.wav"), stash.join("snare.wav")).expect("stash");
    fs::rename(lib.path("plain.wav"), stash.join("plain.wav")).expect("stash");
    assert_eq!(lib.scan().counts.removed, 2);

    fs::rename(stash.join("snare.wav"), lib.path("snare.wav")).expect("unstash");
    fs::rename(stash.join("plain.wav"), lib.path("plain.wav")).expect("unstash");
    let report = lib.scan();
    assert_eq!(report.counts.restored, 2);
    assert_eq!(report.counts.changed + report.counts.added, 0);
    let row = lib.must("snare.wav");
    assert_eq!((row.id, row.status.as_str()), (snare, "analyzed"));
    assert_eq!(lib.attached(snare), [1; 5], "nothing lost while missing");
    let row = lib.must("plain.wav");
    assert_eq!((row.id, row.status.as_str()), (plain, "new"));
}

#[test]
fn reappearing_with_new_content_is_changed() {
    let mut lib = Lib::new();
    lib.write("snare.wav", b"snare");
    lib.scan();
    let id = lib.must("snare.wav").id;
    lib.decorate(id);
    fs::remove_file(lib.path("snare.wav")).expect("rm");
    lib.scan();

    lib.write("snare.wav", b"a different snare");
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 1, 0, 0, 0));
    let row = lib.must("snare.wav");
    assert_eq!((row.id, row.status.as_str()), (id, "new"));
    assert_eq!(lib.attached(id), [0, 0, 1, 1, 1]);
}

#[test]
fn restoring_clears_stale_error_text() {
    let mut lib = Lib::new();
    lib.write("same-path.wav", b"one");
    lib.write("moved.wav", b"two");
    lib.scan();
    // Analysis failures recorded later (status error, hash kept).
    for rel in ["same-path.wav", "moved.wav"] {
        lib.store
            .conn()
            .execute(
                "UPDATE files SET status = 'error', error = 'decode failed' WHERE id = ?1",
                [lib.must(rel).id],
            )
            .expect("mark error");
    }
    let stash = lib.outside();
    for rel in ["same-path.wav", "moved.wav"] {
        fs::rename(lib.path(rel), stash.join(rel)).expect("stash");
    }
    assert_eq!(lib.scan().counts.removed, 2);
    assert_eq!(
        lib.must("moved.wav").error.as_deref(),
        Some("decode failed")
    );

    // Back at the same path (restore), and under a new name (move).
    fs::rename(stash.join("same-path.wav"), lib.path("same-path.wav")).expect("unstash");
    fs::rename(stash.join("moved.wav"), lib.path("renamed.wav")).expect("unstash");
    let report = lib.scan();
    assert_eq!((report.counts.restored, report.counts.moved), (1, 1));
    for rel in ["same-path.wav", "renamed.wav"] {
        let row = lib.must(rel);
        assert_eq!((row.status.as_str(), row.error), ("new", None), "{rel}");
    }
}

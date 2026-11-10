//! Unreadable files and directories, offline roots: never a panic, never
//! a false "missing".

use super::*;

#[test]
fn unreadable_file_is_an_error_row_and_retried() {
    let mut lib = Lib::new();
    lib.write("ok.wav", b"ok");
    lib.write("locked.wav", b"locked");
    let guard = ModeGuard::set(&lib.path("locked.wav"), 0o000, 0o644);
    if permissions_ignored(&lib.path("locked.wav")) {
        return;
    }
    let report = lib.scan();
    assert_eq!(report.counts.added, 1);
    assert_eq!(report.counts.errors, 1);
    assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
    assert_eq!(report.issues[0].kind, IssueKind::UnreadableFile);
    assert_eq!(report.issues[0].path, lib.path("locked.wav"));
    let row = lib.must("locked.wav");
    assert_eq!(row.status, "error");
    assert_eq!(row.quick_hash, None);
    assert!(row.error.as_deref().is_some_and(|e| !e.is_empty()));

    // Still locked: retried, still an error, row reused.
    let again = lib.scan();
    assert_eq!(again.counts.errors, 1);
    assert_eq!(lib.must("locked.wav").id, row.id);

    // Readable again: the next scan fixes the row.
    drop(guard);
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 1, 0, 0, 1));
    let fixed = lib.must("locked.wav");
    assert_eq!(fixed.id, row.id);
    assert_eq!(fixed.status, "new");
    assert!(fixed.quick_hash.is_some());
    assert_eq!(fixed.error, None);
}

#[test]
fn unreadable_file_is_not_a_move_target() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"content");
    lib.scan();
    let id = lib.must("a.wav").id;
    lib.rename("a.wav", "b.wav");
    let _guard = ModeGuard::set(&lib.path("b.wav"), 0o000, 0o644);
    if permissions_ignored(&lib.path("b.wav")) {
        return;
    }
    let report = lib.scan();
    assert_eq!(report.counts.moved, 0);
    assert_eq!(report.counts.removed, 1);
    assert_eq!(report.counts.errors, 1);
    assert_eq!(lib.must("a.wav").status, "missing");
    let b = lib.must("b.wav");
    assert_ne!(b.id, id);
    assert_eq!(b.status, "error");
}

#[test]
fn unreadable_dir_keeps_its_rows() {
    let mut lib = Lib::new();
    lib.write("top.wav", b"top");
    lib.write("locked/a.wav", b"a");
    lib.write("locked/deep/b.wav", b"b");
    lib.write("lockedness.wav", b"sibling with a shared name prefix");
    lib.scan();
    let before = lib.dump();

    let guard = ModeGuard::set(&lib.path("locked"), 0o000, 0o755);
    if permissions_ignored(&lib.path("locked")) {
        return;
    }
    fs::remove_file(lib.path("lockedness.wav")).expect("rm sibling");
    let report = lib.scan();
    drop(guard);
    assert_eq!(report.counts, counts(0, 0, 0, 1, 1));
    assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
    assert_eq!(report.issues[0].kind, IssueKind::UnreadableDir);
    assert_eq!(report.issues[0].path, lib.path("locked"));
    assert_eq!(lib.must("locked/a.wav").status, "new");
    assert_eq!(lib.must("locked/deep/b.wav").status, "new");
    assert_eq!(lib.must("lockedness.wav").status, "missing");

    // Readable again: everything is where it was.
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, 0, 0, 3));
    let after = lib.dump();
    assert_eq!(after.len(), before.len());
}

#[test]
fn missing_root_is_offline_and_changes_no_rows() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"a");
    lib.write("sub/b.wav", b"b");
    lib.scan();
    let before = lib.dump();

    let away = lib.outside().join("unmounted");
    fs::rename(&lib.root, &away).expect("unmount");
    let report = lib.scan();
    assert!(report.offline);
    assert_eq!(report.counts, ScanCounts::default());
    assert_eq!(report.issues.len(), 1);
    assert_eq!(lib.dump(), before, "no file row touched");
    let library = lib.store.library(lib.id).expect("get").expect("exists");
    assert!(!library.online);

    fs::rename(&away, &lib.root).expect("remount");
    let report = lib.scan();
    assert!(!report.offline);
    assert_eq!(report.counts, counts(0, 0, 0, 0, 2));
    assert!(
        lib.store
            .library(lib.id)
            .expect("get")
            .expect("exists")
            .online
    );
}

#[test]
fn empty_root_with_present_rows_is_a_stale_mount_point() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"a");
    lib.write("sub/b.wav", b"b");
    lib.scan();
    let before = lib.dump();

    // Unmounted volume: the mount point stays behind as an empty directory.
    let away = lib.outside().join("unmounted");
    fs::rename(&lib.root, &away).expect("unmount");
    fs::create_dir(&lib.root).expect("empty mount point");
    let report = lib.scan();
    assert!(report.offline);
    assert_eq!(report.counts, ScanCounts::default());
    assert_eq!(lib.dump(), before, "nothing marked missing");
    assert!(
        !lib.store
            .library(lib.id)
            .expect("get")
            .expect("some")
            .online
    );

    // Remounted: normal scan, back online.
    fs::remove_dir(&lib.root).expect("rmdir");
    fs::rename(&away, &lib.root).expect("remount");
    let report = lib.scan();
    assert!(!report.offline);
    assert_eq!(report.counts, counts(0, 0, 0, 0, 2));
    assert!(
        lib.store
            .library(lib.id)
            .expect("get")
            .expect("some")
            .online
    );
}

#[test]
fn root_with_only_non_audio_entries_marks_rows_missing() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"a");
    lib.scan();
    // Emptied in Finder: a `.DS_Store` (hidden) stays behind.
    fs::remove_file(lib.path("a.wav")).expect("rm");
    lib.write(".DS_Store", b"finder");
    let report = lib.scan();
    assert!(!report.offline, "a non-empty root is a real library");
    assert_eq!(report.counts.removed, 1);
    assert_eq!(lib.must("a.wav").status, "missing");
}

#[test]
fn empty_root_with_only_missing_rows_stays_online() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"a");
    lib.write("notes.txt", b"keeps the root non-empty");
    lib.scan();
    fs::remove_file(lib.path("a.wav")).expect("rm");
    lib.scan();
    assert_eq!(lib.must("a.wav").status, "missing");

    fs::remove_file(lib.path("notes.txt")).expect("rm notes");
    let report = lib.scan();
    assert!(!report.offline, "no present rows → no stale-mount guess");
    assert_eq!(report.counts, ScanCounts::default());
    assert_eq!(lib.must("a.wav").status, "missing");
}

#[test]
fn new_empty_root_is_online() {
    let mut lib = Lib::new();
    let report = lib.scan();
    assert!(!report.offline, "a new, empty library is fine");
}

#[test]
fn unlistable_root_is_offline() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"a");
    lib.scan();
    let before = lib.dump();
    let _guard = ModeGuard::set(&lib.root, 0o000, 0o755);
    if permissions_ignored(&lib.root) {
        return;
    }
    let report = lib.scan();
    assert!(report.offline);
    assert_eq!(lib.dump(), before);
}

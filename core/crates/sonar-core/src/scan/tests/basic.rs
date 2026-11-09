//! Discovery: filtering, rel paths, fast-path rescans, symlinks, API errors.

use super::*;
use std::sync::atomic::Ordering;

const AUDIO: &[&str] = &[
    "a.wav",
    "B.WAV",
    "c.Aif",
    "d.aiff",
    "e.flac",
    "f.mp3",
    "g.ogg",
    "h.M4A",
    "kits/808/kick.wav",
];

const IGNORED: &[&str] = &[
    "notes.txt",
    "a.wav.asd",
    "README",
    "video.mp4",
    ".hidden.wav",
    "._a.wav",
    ".git/x.wav",
    "kits/._kick.wav",
    "kits/.cache/y.wav",
];

#[test]
fn initial_scan_filters_and_records_rows() {
    let mut lib = Lib::new();
    for (i, rel) in AUDIO.iter().chain(IGNORED).enumerate() {
        lib.write(rel, &vec![i as u8; 100 + i]);
    }
    let report = lib.scan();
    assert_eq!(report.counts, counts(AUDIO.len(), 0, 0, 0, 0));
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert!(!report.offline);
    assert_eq!(report.root, lib.root);

    let mut expected: Vec<_> = AUDIO
        .iter()
        .map(|s| (s.to_string(), "new".into()))
        .collect();
    expected.sort();
    assert_eq!(lib.paths(), expected);

    let row = lib.must("kits/808/kick.wav");
    let meta = fs::metadata(lib.path("kits/808/kick.wav")).expect("stat");
    assert_eq!(row.size, meta.len() as i64);
    assert_eq!(row.mtime, identity::mtime_ns(&meta).expect("mtime"));
    let expected_hash = quick_hash(&lib.path("kits/808/kick.wav"), meta.len()).expect("hash");
    assert_eq!(row.quick_hash, Some(identity::hash_to_db(expected_hash)));
    assert_eq!(row.error, None);
}

#[test]
fn empty_library_scans_cleanly() {
    let mut lib = Lib::new();
    let report = lib.scan();
    assert_eq!(report.counts, ScanCounts::default());
    assert!(report.issues.is_empty());
}

#[test]
fn rescan_without_changes_reads_nothing() {
    let mut lib = Lib::new();
    for rel in AUDIO {
        lib.write(rel, rel.as_bytes());
    }
    lib.scan();
    let before = lib.dump();

    // Make every file unreadable: an unchanged rescan must not open any.
    let guards: Vec<_> = AUDIO
        .iter()
        .map(|rel| ModeGuard::set(&lib.path(rel), 0o000, 0o644))
        .collect();
    if permissions_ignored(&lib.path(AUDIO[0])) {
        return;
    }
    let report = lib.scan();
    drop(guards);
    assert_eq!(report.counts, counts(0, 0, 0, 0, AUDIO.len()));
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert_eq!(lib.dump(), before);
}

#[test]
fn symlinks_are_not_followed() {
    let mut lib = Lib::new();
    lib.write("real.wav", b"real");
    let outside = lib.outside().join("outside");
    fs::create_dir(&outside).expect("mkdir");
    fs::write(outside.join("x.wav"), b"outside").expect("write");
    let link = |target: &Path, rel: &str| {
        std::os::unix::fs::symlink(target, lib.path(rel)).expect("symlink");
    };
    link(&outside.join("x.wav"), "file-link.wav");
    link(&outside, "dir-link");
    link(&lib.root, "loop"); // would recurse forever if followed
    link(&lib.path("nope.wav"), "dangling.wav");

    let report = lib.scan();
    assert_eq!(report.counts, counts(1, 0, 0, 0, 0));
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert_eq!(lib.paths(), vec![("real.wav".into(), "new".into())]);
}

#[test]
fn non_utf8_names_are_reported_and_skipped() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let mut lib = Lib::new();
    lib.write("ok.wav", b"ok");
    let bad = lib.root.join(OsStr::from_bytes(b"bad\xff.wav"));
    if fs::write(&bad, b"x").is_err() {
        // APFS rejects non-UTF-8 names; the walk logic is still covered by
        // `walk::tests::non_utf8_extension_check`.
        eprintln!("skipping: filesystem rejects non-UTF-8 names");
        return;
    }
    let report = lib.scan();
    assert_eq!(report.counts.added, 1);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, IssueKind::NonUtf8Name);
}

#[test]
fn unknown_library_is_an_error() {
    let mut store = Store::open_in_memory().expect("store");
    assert!(matches!(
        scan_library(&mut store, 42),
        Err(Error::LibraryNotFound { id: 42 })
    ));
}

#[test]
fn cancelled_scan_writes_nothing() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"a");
    let opts = ScanOptions {
        cancel: Some(Arc::new(AtomicBool::new(true))),
    };
    let result = scan_library_with(&mut lib.store, lib.id, &opts);
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(lib.paths().is_empty());

    // Clearing the flag lets the next scan run normally.
    if let Some(flag) = &opts.cancel {
        flag.store(false, Ordering::Relaxed);
    }
    let report = scan_library_with(&mut lib.store, lib.id, &opts).expect("scan");
    assert_eq!(report.counts.added, 1);
}

#[test]
fn issue_display_is_readable() {
    let issue = ScanIssue {
        path: "/lib/x".into(),
        kind: IssueKind::UnreadableDir,
        message: "Permission denied".into(),
    };
    assert_eq!(
        issue.to_string(),
        "unreadable directory: /lib/x: Permission denied"
    );
}

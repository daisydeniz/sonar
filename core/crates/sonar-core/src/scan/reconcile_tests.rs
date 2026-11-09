//! Unit tests of the matching rules, with an injected hash function.

use super::*;
use std::sync::atomic::AtomicUsize;

fn row(id: i64, rel: &str, size: i64, mtime: i64, hash: Option<i64>, status: Status) -> DbFile {
    DbFile {
        id,
        rel_path: rel.into(),
        size,
        mtime,
        quick_hash: hash,
        status,
    }
}

fn disk(rel: &str, size: u64, mtime: i64) -> DiskFile {
    DiskFile {
        rel_path: rel.into(),
        size,
        mtime_ns: mtime,
    }
}

/// Fake hash: the size (so equal sizes "match"), unless the name contains
/// "bad" (read error). Counts calls.
fn run(db: Vec<DbFile>, files: Vec<DiskFile>, unknown: &Unknown, calls: &AtomicUsize) -> Plan {
    let never = AtomicBool::new(false);
    plan(
        db,
        files,
        unknown,
        Path::new("/lib"),
        &never,
        |rel, size, _buf| {
            calls.fetch_add(1, Ordering::Relaxed);
            if rel.contains("bad") {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            } else {
                Ok(size)
            }
        },
    )
    .expect("not cancelled")
}

#[test]
fn unchanged_files_are_never_hashed() {
    let db = vec![
        row(1, "a.wav", 10, 100, Some(10), Status::New),
        row(2, "b.wav", 20, 200, Some(20), Status::Analyzed),
    ];
    let files = vec![disk("a.wav", 10, 100), disk("b.wav", 20, 200)];
    let calls = AtomicUsize::new(0);
    let p = run(db, files, &Unknown::default(), &calls);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(p.ops.is_empty(), "{:?}", p.ops);
    assert_eq!(p.counts.unchanged, 2);
}

#[test]
fn size_or_mtime_change_rehashes() {
    let db = vec![
        row(1, "a.wav", 10, 100, Some(10), Status::Meta),
        row(2, "b.wav", 20, 200, Some(20), Status::Analyzed),
    ];
    let files = vec![disk("a.wav", 11, 100), disk("b.wav", 20, 201)];
    let calls = AtomicUsize::new(0);
    let p = run(db, files, &Unknown::default(), &calls);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(p.counts.changed, 2);
    assert_eq!(
        p.ops,
        vec![
            Op::Update {
                id: 1,
                size: 11,
                mtime: 100,
                identity: Identity::Hash(11)
            },
            Op::Update {
                id: 2,
                size: 20,
                mtime: 201,
                identity: Identity::Hash(20)
            },
        ]
    );
}

#[test]
fn ambiguous_moves_pair_by_sorted_path() {
    // Two identical files (same size → same fake hash) both renamed.
    let db = vec![
        row(2, "z/b.wav", 5, 1, Some(5), Status::New),
        row(1, "z/a.wav", 5, 1, Some(5), Status::New),
    ];
    let files = vec![disk("x.wav", 5, 1), disk("y.wav", 5, 1)];
    let calls = AtomicUsize::new(0);
    let p = run(db, files, &Unknown::default(), &calls);
    assert_eq!(p.counts.moved, 2);
    assert_eq!(p.counts.added + p.counts.removed, 0);
    let moves: Vec<_> = p
        .ops
        .iter()
        .map(|op| match op {
            Op::Move { id, rel_path, .. } => (*id, rel_path.as_str()),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(moves, vec![(1, "x.wav"), (2, "y.wav")]);
}

#[test]
fn surplus_sources_go_missing_and_surplus_targets_are_added() {
    let db = vec![
        row(1, "a.wav", 5, 1, Some(5), Status::New),
        row(2, "b.wav", 5, 1, Some(5), Status::New),
        row(3, "c.wav", 7, 1, Some(7), Status::New),
    ];
    let files = vec![disk("n.wav", 5, 1), disk("m.wav", 8, 1)];
    let calls = AtomicUsize::new(0);
    let p = run(db, files, &Unknown::default(), &calls);
    assert_eq!(
        (p.counts.moved, p.counts.added, p.counts.removed),
        (1, 1, 2)
    );
    assert!(p.ops.contains(&Op::Move {
        id: 1,
        rel_path: "n.wav".into(),
        mtime: 1,
        restore: false
    }));
    assert!(p.ops.contains(&Op::Missing { id: 2 }));
    assert!(p.ops.contains(&Op::Missing { id: 3 }));
}

#[test]
fn present_sources_win_over_missing_ones() {
    let db = vec![
        row(1, "a.wav", 5, 1, Some(5), Status::Missing),
        row(2, "b.wav", 5, 1, Some(5), Status::New),
    ];
    let p = run(
        db.clone(),
        vec![disk("c.wav", 5, 1)],
        &Unknown::default(),
        &AtomicUsize::new(0),
    );
    assert_eq!(
        p.ops,
        vec![Op::Move {
            id: 2,
            rel_path: "c.wav".into(),
            mtime: 1,
            restore: false
        }]
    );
    // With only the missing row left, it is revived by the move.
    let p = run(
        vec![db[0].clone()],
        vec![disk("c.wav", 5, 1)],
        &Unknown::default(),
        &AtomicUsize::new(0),
    );
    assert_eq!(p.counts.removed, 0, "already missing is not re-counted");
    assert!(matches!(
        p.ops[..],
        [Op::Move {
            id: 1,
            restore: true,
            ..
        }]
    ));
}

#[test]
fn unknown_paths_are_not_marked_missing() {
    let db = vec![
        row(1, "locked/a.wav", 5, 1, Some(5), Status::New),
        row(2, "stat-failed.wav", 6, 1, Some(6), Status::New),
        row(3, "gone.wav", 7, 1, None, Status::Error),
    ];
    let mut unknown = Unknown::default();
    unknown.add_dir("locked");
    unknown.add_file("stat-failed.wav");
    let p = run(db, vec![], &unknown, &AtomicUsize::new(0));
    assert_eq!(p.ops, vec![Op::Missing { id: 3 }]);
}

#[test]
fn unreadable_files_are_errors_and_never_move_targets() {
    let db = vec![row(1, "a.wav", 5, 1, Some(5), Status::New)];
    let files = vec![disk("bad.wav", 5, 1)];
    let p = run(db, files, &Unknown::default(), &AtomicUsize::new(0));
    assert_eq!(
        (p.counts.errors, p.counts.moved, p.counts.removed),
        (1, 0, 1)
    );
    assert!(matches!(
        &p.ops[0],
        Op::Insert { identity: Identity::Error(m), .. } if m.contains("denied")
    ));
    assert_eq!(p.issues.len(), 1);
    assert_eq!(p.issues[0].path, Path::new("/lib/bad.wav"));
    assert_eq!(p.issues[0].kind, IssueKind::UnreadableFile);
}

#[test]
fn scan_errors_are_retried_and_missing_rows_restored() {
    let db = vec![
        row(1, "err.wav", 5, 1, None, Status::Error),
        row(2, "analysis-failed.wav", 6, 1, Some(6), Status::Error),
        row(3, "back.wav", 7, 1, Some(7), Status::Missing),
        row(4, "back-changed.wav", 8, 1, Some(8), Status::Missing),
    ];
    let files = vec![
        disk("err.wav", 5, 1),
        disk("analysis-failed.wav", 6, 1),
        disk("back.wav", 7, 1),
        disk("back-changed.wav", 9, 1),
    ];
    let calls = AtomicUsize::new(0);
    let p = run(db, files, &Unknown::default(), &calls);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "err.wav + back-changed.wav"
    );
    assert_eq!(
        (p.counts.changed, p.counts.restored, p.counts.unchanged),
        (2, 1, 1)
    );
    assert!(p.ops.contains(&Op::Restore { id: 3 }));
}

#[test]
fn cancel_before_hashing() {
    let cancel = AtomicBool::new(true);
    let result = plan(
        vec![],
        vec![disk("a.wav", 1, 1)],
        &Unknown::default(),
        Path::new("/lib"),
        &cancel,
        |_, s, _| Ok(s),
    );
    assert!(result.is_err());
}

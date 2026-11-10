//! Renames and moves keep the row id and its data.

use super::*;

#[test]
fn rename_and_move_keep_id_and_user_data() {
    let mut lib = Lib::new();
    lib.write("drums/kick.wav", &[1; 200_000]); // > 128 KiB: head+tail hash
    lib.write("drums/snare.wav", b"snare");
    lib.write("keep.wav", b"keep");
    lib.scan();
    let kick = lib.must("drums/kick.wav");
    let snare = lib.must("drums/snare.wav");
    lib.decorate(kick.id);
    lib.decorate(snare.id);

    lib.rename("drums/kick.wav", "drums/kick 01.wav"); // same dir
    lib.rename("drums/snare.wav", "perc/new/snare.wav"); // new subdir
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, 2, 0, 1));

    let moved = lib.must("drums/kick 01.wav");
    assert_eq!(moved.id, kick.id);
    assert_eq!(moved.quick_hash, kick.quick_hash);
    assert_eq!(moved.status, "analyzed", "stage data still valid");
    assert_eq!(lib.attached(kick.id), [1; 5]);
    assert_eq!(lib.must("perc/new/snare.wav").id, snare.id);
    assert_eq!(lib.attached(snare.id), [1; 5]);
    assert!(lib.row("drums/kick.wav").is_none(), "no stale row left");
    assert_eq!(lib.paths().len(), 3);

    // And the next scan sees everything as unchanged.
    assert_eq!(lib.scan().counts, counts(0, 0, 0, 0, 3));
}

#[test]
fn case_only_rename_is_a_move() {
    // Works the same on case-insensitive (default APFS) and case-sensitive
    // volumes: the walk reports the new spelling, rel_path comparison is
    // exact, so the old row follows the file to `kick.wav`.
    let mut lib = Lib::new();
    lib.write("Kick.wav", b"boom");
    lib.scan();
    let kick = lib.must("Kick.wav");
    lib.decorate(kick.id);

    lib.rename("Kick.wav", "kick.wav");
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, 1, 0, 0));
    assert_eq!(
        lib.paths(),
        vec![("kick.wav".to_string(), "analyzed".to_string())]
    );
    assert_eq!(lib.must("kick.wav").id, kick.id);
    assert_eq!(lib.attached(kick.id), [1; 5]);
    assert_eq!(lib.scan().counts, counts(0, 0, 0, 0, 1));
}

#[test]
fn folder_rename_moves_every_file() {
    let mut lib = Lib::new();
    let old = [
        "Drums/kick.wav",
        "Drums/snare.wav",
        "Drums/hats/closed.wav",
        "Drums/hats/open.wav",
    ];
    for (i, rel) in old.iter().enumerate() {
        lib.write(rel, &vec![i as u8; 1000 + i]);
    }
    lib.write("bass.wav", b"bass");
    lib.scan();
    let ids: Vec<i64> = old.iter().map(|rel| lib.must(rel).id).collect();
    lib.decorate(ids[2]);

    fs::rename(lib.path("Drums"), lib.path("Percussion")).expect("rename dir");
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, old.len(), 0, 1));
    for (rel, id) in old.iter().zip(&ids) {
        let new_rel = rel.replacen("Drums/", "Percussion/", 1);
        assert_eq!(lib.must(&new_rel).id, *id, "{new_rel}");
        assert!(lib.row(rel).is_none(), "{rel} left behind");
    }
    assert_eq!(lib.attached(ids[2]), [1; 5]);
}

#[test]
fn move_with_new_mtime_updates_it() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"content");
    lib.set_mtime("a.wav", 3_000_000);
    lib.scan();
    let id = lib.must("a.wav").id;
    // Copy + delete (as across volumes): new mtime, same content.
    lib.write("b.wav", b"content");
    fs::remove_file(lib.path("a.wav")).expect("rm");
    let report = lib.scan();
    assert_eq!(report.counts.moved, 1);
    let row = lib.must("b.wav");
    assert_eq!(row.id, id);
    assert_ne!(row.mtime, 3_000_000 * 1_000_000_000);
    assert_eq!(lib.scan().counts.unchanged, 1);
}

#[test]
fn identical_files_pair_deterministically() {
    let mut lib = Lib::new();
    for rel in ["a.wav", "b.wav", "c.wav"] {
        lib.write(rel, b"same bytes");
    }
    lib.scan();
    let id = |rel: &str| lib.must(rel).id;
    let (a, b, c) = (id("a.wav"), id("b.wav"), id("c.wav"));

    // One of three identical files renamed: its row follows it.
    lib.rename("b.wav", "z.wav");
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, 1, 0, 2));
    assert_eq!(lib.must("z.wav").id, b);

    // Two renamed at once: sources and targets pair in path order
    // (a → x, c → y), whatever order the OS lists them in.
    lib.rename("c.wav", "y.wav");
    lib.rename("a.wav", "x.wav");
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, 2, 0, 1));
    assert_eq!(lib.must("x.wav").id, a);
    assert_eq!(lib.must("y.wav").id, c);
}

#[test]
fn surplus_identical_copies_are_added_or_removed() {
    let mut lib = Lib::new();
    lib.write("a.wav", b"dup");
    lib.scan();
    let a = lib.must("a.wav").id;
    // a.wav renamed to m.wav, plus a second copy n.wav.
    lib.rename("a.wav", "m.wav");
    lib.write("n.wav", b"dup");
    let report = lib.scan();
    assert_eq!(report.counts, counts(1, 0, 1, 0, 0));
    assert_eq!(lib.must("m.wav").id, a, "first target by path wins");
    assert_ne!(lib.must("n.wav").id, a);
}

#[test]
fn missing_row_is_revived_by_a_later_move() {
    let mut lib = Lib::new();
    lib.write("one.wav", b"one-shot");
    lib.write("notes.txt", b"x"); // root never looks like an empty mount point
    lib.scan();
    let id = lib.must("one.wav").id;
    lib.decorate(id);
    let stash = lib.outside().join("one.wav");
    fs::rename(lib.path("one.wav"), &stash).expect("stash");
    assert_eq!(lib.scan().counts.removed, 1);

    // Comes back under another name, in another scan.
    fs::create_dir(lib.path("sorted")).expect("mkdir");
    fs::rename(&stash, lib.path("sorted/one.wav")).expect("unstash");
    let report = lib.scan();
    assert_eq!(report.counts, counts(0, 0, 1, 0, 0));
    let row = lib.must("sorted/one.wav");
    assert_eq!((row.id, row.status.as_str()), (id, "analyzed"));
    assert_eq!(lib.attached(id), [1; 5]);
}

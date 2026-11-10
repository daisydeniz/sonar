//! Text index tests: build from the store, refresh, upsert/delete,
//! versioning.

use std::fs;
use std::path::Path;

use rusqlite::params;

use super::*;
use crate::search::{search, SearchOptions, Synonyms};
use crate::Store;

fn find(ix: &TextIndex, q: &str) -> Vec<i64> {
    let opts = SearchOptions {
        limit: 100,
        prefix: false,
    };
    let mut ids: Vec<i64> = search(ix, &Synonyms::builtin(), q, &opts)
        .expect("search")
        .hits
        .iter()
        .map(|h| h.file_id)
        .collect();
    ids.sort_unstable();
    ids
}

fn add_file(store: &Store, lib: i64, rel: &str) -> i64 {
    store
        .conn()
        .execute(
            "INSERT INTO files (library_id, rel_path, size, mtime) VALUES (?1, ?2, 1, 1)",
            params![lib, rel],
        )
        .expect("insert file");
    store.conn().last_insert_rowid()
}

/// A store with library `<tmp>/Splice` and five files: a tagged kick, a
/// file whose category says snare, a one-shot by override, a missing file
/// and a plain loop.
fn store_with_files(tmp: &Path) -> (Store, [i64; 5]) {
    let root = tmp.join("Splice");
    fs::create_dir(&root).expect("mkdir");
    let store = Store::open_in_memory().expect("store");
    let lib = store.add_library(&root).expect("library").id;
    let kick = add_file(&store, lib, "Drums/Kick_01.wav");
    let snare = add_file(&store, lib, "Drums/X1.wav");
    let shot = add_file(&store, lib, "Keys/Rhodes C.wav");
    let gone = add_file(&store, lib, "Drums/Kick_Gone.wav");
    let other = add_file(&store, lib, "Loops/Guitar Loop.wav");
    let c = store.conn();
    c.execute("INSERT INTO tags (name) VALUES ('Dark Techno')", [])
        .expect("tag");
    c.execute(
        "INSERT INTO file_tags (file_id, tag_id) VALUES (?1, ?2)",
        params![kick, c.last_insert_rowid()],
    )
    .expect("file tag");
    c.execute(
        "INSERT INTO analysis (file_id, category, kind) VALUES (?1, 'snare', 'loop')",
        [snare],
    )
    .expect("analysis");
    c.execute(
        "INSERT INTO analysis (file_id, kind) VALUES (?1, 'loop')",
        [shot],
    )
    .expect("analysis");
    c.execute(
        "INSERT INTO overrides (file_id, field, value) VALUES (?1, 'kind', 'one_shot')",
        [shot],
    )
    .expect("override");
    // An explicit NULL override clears the detected value.
    c.execute(
        "INSERT INTO overrides (file_id, field, value) VALUES (?1, 'kind', NULL)",
        [snare],
    )
    .expect("override");
    c.execute("UPDATE files SET status = 'missing' WHERE id = ?1", [gone])
        .expect("missing");
    (store, [kick, snare, shot, gone, other])
}

#[test]
fn rebuild_indexes_names_folders_tags_category_and_kind() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (store, [kick, snare, shot, _gone, other]) = store_with_files(tmp.path());
    let ix = TextIndex::in_ram().expect("index");
    let report = ix.rebuild(&store).expect("rebuild");
    assert_eq!(report.docs, 4, "missing file not indexed");
    assert_eq!(find(&ix, "kick"), [kick]);
    assert_eq!(find(&ix, "dark techno"), [kick], "tags");
    assert_eq!(find(&ix, "sd"), [snare], "category + synonym");
    assert_eq!(find(&ix, "oneshot"), [shot], "kind override");
    assert!(find(&ix, "loop")
        .iter()
        .all(|&id| id != snare && id != shot));
    assert_eq!(find(&ix, "splice drums"), [kick, snare], "root + folders");
    assert_eq!(find(&ix, "guitar"), [other]);
    // Rebuild replaces, never duplicates.
    assert_eq!(ix.rebuild(&store).expect("again").docs, 4);
}

#[test]
fn refresh_files_follows_the_store() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (store, [kick, snare, _, gone, _]) = store_with_files(tmp.path());
    let ix = TextIndex::in_ram().expect("index");
    ix.rebuild(&store).expect("rebuild");
    let c = store.conn();
    c.execute(
        "UPDATE files SET rel_path = 'Drums/Snare_Big.wav' WHERE id = ?1",
        [kick],
    )
    .expect("rename");
    c.execute("UPDATE files SET status = 'meta' WHERE id = ?1", [gone])
        .expect("restore");
    c.execute("DELETE FROM files WHERE id = ?1", [snare])
        .expect("delete");
    ix.refresh_files(&store, &[kick, gone, snare, 9999])
        .expect("refresh");
    assert_eq!(find(&ix, "kick"), [gone]);
    assert_eq!(find(&ix, "snare"), [kick]);
    assert_eq!(ix.num_docs(), 4);
}

#[test]
fn upsert_replaces_and_delete_removes() {
    let ix = TextIndex::in_ram().expect("index");
    let doc = |id, rel| IndexDoc::from_paths(id, Path::new("/L"), rel);
    ix.upsert(&[doc(1, "Kick.wav"), doc(2, "Snare.wav")])
        .expect("upsert");
    assert_eq!(ix.num_docs(), 2);
    ix.upsert(&[doc(1, "Clap.wav")]).expect("replace");
    assert_eq!(ix.num_docs(), 2);
    assert!(find(&ix, "kick").is_empty());
    assert_eq!(find(&ix, "clap"), [1]);
    ix.delete(&[2, 77, -1]).expect("delete");
    assert_eq!(ix.num_docs(), 1);
    assert!(find(&ix, "snare").is_empty());
}

#[test]
fn version_mismatch_and_unfinished_builds_trigger_a_rebuild() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (store, _) = store_with_files(tmp.path());
    let dir = tmp.path().join("home").join("index").join("text");

    let (ix, state) = TextIndex::open(&dir).expect("open new");
    assert_eq!(state, OpenState::NeedsRebuild(RebuildReason::New));
    ix.rebuild(&store).expect("rebuild");
    drop(ix);

    let (ix, state) = TextIndex::open(&dir).expect("reopen");
    assert_eq!(state, OpenState::Ready);
    assert_eq!(ix.num_docs(), 4);
    drop(ix);

    fs::write(
        dir.join(VERSION_FILE),
        "sonar text index: schema 0, analyzer 1",
    )
    .expect("write");
    let (ix, state) = TextIndex::open(&dir).expect("stale");
    assert!(
        matches!(&state, OpenState::NeedsRebuild(RebuildReason::VersionChanged { found })
            if found.contains("schema 0")),
        "{state:?}"
    );
    assert_eq!(ix.num_docs(), 0, "stale index is never searched");
    ix.rebuild(&store).expect("rebuild");
    drop(ix);

    // A build interrupted after removing the version file.
    fs::remove_file(dir.join(VERSION_FILE)).expect("rm");
    let (_, state) = TextIndex::open(&dir).expect("incomplete");
    assert_eq!(state, OpenState::NeedsRebuild(RebuildReason::Incomplete));
}

#[test]
fn foreign_directories_are_never_wiped() {
    let tmp = tempfile::tempdir().expect("tmp");
    let precious = tmp.path().join("notes.txt");
    fs::write(&precious, "keep me").expect("write");
    let err = TextIndex::open(tmp.path()).expect_err("foreign");
    assert!(matches!(err, crate::Error::ForeignIndexDir { .. }), "{err}");
    assert_eq!(fs::read_to_string(&precious).expect("read"), "keep me");
}

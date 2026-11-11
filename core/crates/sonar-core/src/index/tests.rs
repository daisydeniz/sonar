//! Text index tests: build from the store, refresh, upsert/delete,
//! versioning.

use std::fs;
use std::path::Path;

use rusqlite::params;

use super::*;
use crate::search::keys::parse_key;
use crate::search::{search, Filters, SearchOptions, Sort, SortField, Synonyms, ValueRange};
use crate::Store;

fn find(ix: &TextIndex, q: &str) -> Vec<i64> {
    let opts = SearchOptions {
        limit: 100,
        prefix: false,
        ..SearchOptions::default()
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

/// Ids matching `filters` (no text), in `sort` order.
fn filter(ix: &TextIndex, filters: Filters, sort: Sort) -> Vec<i64> {
    let opts = SearchOptions {
        limit: 100,
        filters,
        sort,
        ..SearchOptions::default()
    };
    search(ix, &Synonyms::builtin(), "", &opts)
        .expect("search")
        .hits
        .iter()
        .map(|h| h.file_id)
        .collect()
}

#[test]
fn filter_values_are_effective_values() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("Lib");
    fs::create_dir(&root).expect("mkdir");
    let store = Store::open_in_memory().expect("store");
    let lib = store.add_library(&root).expect("library").id;
    let prior = add_file(&store, lib, "Loops/Bass_Loop_128_Am.wav");
    let detected = add_file(&store, lib, "Loops/Pad 120 BPM Cm.wav");
    let overridden = add_file(&store, lib, "Loops/Arp 124 BPM F#m.wav");
    let hidden = add_file(&store, lib, "Shots/Snare.wav");
    let fav = add_file(&store, lib, "Shots/Clap.wav");
    let c = store.conn();
    for (sql, id) in [
        // Detected BPM beats the filename prior; the prior key stays.
        (
            "INSERT INTO analysis (file_id, bpm) VALUES (?1, 90.0)",
            detected,
        ),
        // Override beats the prior; a NULL override clears the key.
        (
            "INSERT INTO overrides (file_id, field, value) VALUES (?1, 'bpm', 100)",
            overridden,
        ),
        (
            "INSERT INTO overrides (file_id, field, value) VALUES (?1, 'key', NULL)",
            overridden,
        ),
        (
            "INSERT INTO audio_meta (file_id, duration_ms) VALUES (?1, 250)",
            hidden,
        ),
        ("INSERT INTO hidden (file_id) VALUES (?1)", hidden),
        (
            "INSERT INTO favorites (file_id, added_at) VALUES (?1, 0)",
            hidden,
        ),
        (
            "INSERT INTO favorites (file_id, added_at) VALUES (?1, 0)",
            fav,
        ),
        (
            "INSERT INTO analysis (file_id, key) VALUES (?1, 'G minor')",
            fav,
        ),
    ] {
        c.execute(sql, [id]).expect(sql);
    }
    let ix = TextIndex::in_ram().expect("index");
    assert_eq!(ix.rebuild(&store).expect("rebuild").docs, 5);
    let bpm = |b| Filters {
        bpm: Some(ValueRange::new(Some(b), Some(b))),
        ..Filters::default()
    };
    let by = Sort::default();
    assert_eq!(filter(&ix, bpm(128.0), by), [prior]);
    assert_eq!(filter(&ix, bpm(90.0), by), [detected]);
    assert_eq!(filter(&ix, bpm(100.0), by), [overridden]);
    let key = |k| Filters {
        keys: vec![parse_key(k).expect("key")],
        ..Filters::default()
    };
    assert_eq!(filter(&ix, key("Am"), by), [prior]);
    assert_eq!(filter(&ix, key("Cm"), by), [detected]);
    assert!(
        filter(&ix, key("F#m"), by).is_empty(),
        "cleared by override"
    );
    assert_eq!(filter(&ix, key("Gm"), by), [fav]);
    let short = Filters {
        duration_ms: Some(ValueRange::new(None, Some(300))),
        ..Filters::default()
    };
    assert!(filter(&ix, short.clone(), by).is_empty(), "hidden");
    let short = Filters {
        include_hidden: true,
        ..short
    };
    assert_eq!(filter(&ix, short, by), [hidden]);
    let favs = Filters {
        favorites_only: true,
        ..Filters::default()
    };
    assert_eq!(filter(&ix, favs.clone(), by), [fav]);
    let favs = Filters {
        include_hidden: true,
        ..favs
    };
    assert_eq!(filter(&ix, favs, by), [fav, hidden], "browse: by name");
    let libs = Filters {
        libraries: vec![lib],
        ..Filters::default()
    };
    let by_bpm = Sort::by(SortField::Bpm);
    assert_eq!(
        filter(&ix, libs, by_bpm),
        [detected, overridden, prior, fav]
    );
}

#[test]
fn one_shots_get_no_bpm_from_their_name() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("Lib");
    fs::create_dir(&root).expect("mkdir");
    let store = Store::open_in_memory().expect("store");
    let lib = store.add_library(&root).expect("library").id;
    let shot = add_file(&store, lib, "Shots/Kick 120 BPM.wav");
    let unknown = add_file(&store, lib, "Loops/Beat 120 BPM.wav");
    store
        .conn()
        .execute(
            "INSERT INTO analysis (file_id, kind) VALUES (?1, 'one_shot')",
            [shot],
        )
        .expect("analysis");
    let ix = TextIndex::in_ram().expect("index");
    ix.rebuild(&store).expect("rebuild");
    let f = Filters {
        bpm: Some(ValueRange::new(Some(120.0), Some(120.0))),
        ..Filters::default()
    };
    assert_eq!(filter(&ix, f, Sort::default()), [unknown]);
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

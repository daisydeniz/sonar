//! Tests for tag normalisation, the 0005 migration and tag management.

use rusqlite::Connection;

use super::migrations::{migrate, MIGRATIONS};
use super::user_data::store_with_files;
use super::{SimilarReason, Store};
use crate::Error;

fn names(s: &Store) -> Vec<(String, i64)> {
    s.tags()
        .unwrap()
        .into_iter()
        .map(|t| (t.name, t.files))
        .collect()
}

fn n(x: &str) -> String {
    x.to_owned()
}

#[test]
fn writes_are_normalised() {
    let mut s = store_with_files();
    s.add_tags(&[1], &["  Dark \u{a0} Vibe  ".into()]).unwrap();
    s.add_tags(&[2], &["dark vibe".into(), "DARK    VIBE".into()])
        .unwrap();
    assert_eq!(names(&s), [(n("Dark Vibe"), 2)]);
    assert_eq!(s.remove_tags(&[1], &[" dark   vibe ".into()]).unwrap(), 1);
    for bad in ["a\tb", "a\nb"] {
        assert!(matches!(
            s.add_tags(&[1], &[bad.into()]),
            Err(Error::InvalidInput { .. })
        ));
    }
}

/// Builds a v4 database whose `tags` table (rebuilt without the UNIQUE
/// constraint, as an old or damaged one could be) holds dirty names.
fn dirty_v4() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate(&mut conn, &MIGRATIONS[..4]).unwrap();
    conn.execute_batch(
        "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/l', 0);
         INSERT INTO files (id, library_id, rel_path, size, mtime) VALUES
           (1, 1, 'a', 1, 0), (2, 1, 'b', 1, 0), (3, 1, 'c', 1, 0), (4, 1, 'd', 1, 0);
         DROP TABLE file_tags; DROP TABLE tags;
         CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL) STRICT;
         CREATE TABLE file_tags (
           file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
           tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
           PRIMARY KEY (file_id, tag_id)) STRICT;
         INSERT INTO tags (id, name) VALUES
           (1, 'Dark'), (2, 'dark'), (3, ' dark '), (4, 'warm   pad'), (5, 'Kick'), (6, 'kick');
         -- dark: id 2 has 2 files, ids 1 and 3 one each; kick: tie -> oldest (5).
         INSERT INTO file_tags (file_id, tag_id) VALUES
           (1, 1), (2, 2), (3, 2), (3, 3), (4, 3), (1, 4), (2, 5), (3, 5), (3, 6), (4, 6);",
    )
    .unwrap();
    conn
}

#[test]
fn migration_merges_case_duplicates_into_the_most_used_spelling() {
    let mut conn = dirty_v4();
    assert_eq!(migrate(&mut conn, MIGRATIONS).unwrap(), 1);
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT id, name FROM tags ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        [(2, n("dark")), (4, n("warm pad")), (5, n("Kick"))],
        "dark: most used wins; kick: tie goes to the oldest"
    );
    let links = |tag: i64| -> Vec<i64> {
        conn.prepare("SELECT file_id FROM file_tags WHERE tag_id = ?1 ORDER BY file_id")
            .unwrap()
            .query_map([tag], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(links(2), [1, 2, 3, 4], "links of the merged tags are kept");
    assert_eq!(links(5), [2, 3, 4]);
    assert!(conn
        .execute("INSERT INTO tags (name) VALUES ('DARK')", [])
        .is_err());
    assert!(conn
        .execute("INSERT INTO tag_ignored_pairs (a, b) VALUES ('b', 'a')", [])
        .is_err());
}

#[test]
fn migration_on_a_clean_database_changes_nothing() {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate(&mut conn, &MIGRATIONS[..4]).unwrap();
    conn.execute_batch(
        "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/l', 0);
         INSERT INTO files (id, library_id, rel_path, size, mtime) VALUES (1, 1, 'a', 1, 0);
         INSERT INTO tags (id, name) VALUES (7, 'Punchy'), (9, 'unused');
         INSERT INTO file_tags (file_id, tag_id) VALUES (1, 7);",
    )
    .unwrap();
    migrate(&mut conn, MIGRATIONS).unwrap();
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT id, name FROM tags ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, [(7, n("Punchy")), (9, n("unused"))]);
}

fn tagged() -> Store {
    let mut s = store_with_files();
    s.add_tags(&[1, 2], &["dark".into()]).unwrap();
    s.add_tags(&[2, 3], &["Warm".into()]).unwrap();
    s.add_tags(&[3], &["pad".into()]).unwrap();
    s
}

#[test]
fn list_includes_unused_tags() {
    let s = tagged();
    s.conn()
        .execute("INSERT INTO tags (name) VALUES ('lonely')", [])
        .unwrap();
    assert_eq!(
        names(&s),
        [
            (n("dark"), 2),
            (n("lonely"), 0),
            (n("pad"), 1),
            (n("Warm"), 2)
        ]
    );
}

#[test]
fn rename_changes_spelling_and_reports_files() {
    let mut s = tagged();
    assert_eq!(s.rename_tag("DARK", "  Dark   Moody ").unwrap(), [1, 2]);
    assert_eq!(s.file_tags(1).unwrap(), ["Dark Moody"]);
    // A case-only rename of the same tag is not a merge.
    assert_eq!(s.rename_tag("dark moody", "dark moody").unwrap(), [1, 2]);
    assert!(matches!(
        s.rename_tag("nope", "x"),
        Err(Error::InvalidInput { .. })
    ));
    assert!(matches!(
        s.rename_tag("pad", " "),
        Err(Error::InvalidInput { .. })
    ));
}

#[test]
fn rename_onto_an_existing_tag_merges() {
    let mut s = tagged();
    assert_eq!(s.rename_tag("pad", "WARM").unwrap(), [3]);
    assert_eq!(
        names(&s),
        [(n("dark"), 2), (n("Warm"), 2)],
        "target spelling stays"
    );
    assert_eq!(s.rename_tag("dark", "warm").unwrap(), [1, 2]);
    assert_eq!(names(&s), [(n("Warm"), 3)]);
}

#[test]
fn merge_many_into_one() {
    let mut s = tagged();
    let files = s
        .merge_tags(&[n("dark"), n("PAD"), n("dark")], "Moody")
        .unwrap();
    assert_eq!(files, [1, 2, 3]);
    assert_eq!(names(&s), [(n("Moody"), 3), (n("Warm"), 2)]);
    // Into an existing tag; a source equal to the target is skipped.
    assert_eq!(
        s.merge_tags(&[n("moody"), n("warm")], "warm").unwrap(),
        [1, 2, 3]
    );
    assert_eq!(names(&s), [(n("Warm"), 3)]);
    // All-or-nothing on an unknown source or no sources.
    assert!(s.merge_tags(&[n("warm"), n("ghost")], "x").is_err());
    assert!(s.merge_tags(&[], "x").is_err());
    assert_eq!(names(&s), [(n("Warm"), 3)]);
}

#[test]
fn delete_removes_tag_everywhere() {
    let mut s = tagged();
    assert_eq!(s.delete_tag(" WARM ").unwrap(), [2, 3]);
    assert_eq!(names(&s), [(n("dark"), 2), (n("pad"), 1)]);
    assert!(s.file_tags(3).unwrap().iter().all(|t| t == "pad"));
    assert!(s.delete_tag("warm").is_err());
}

#[test]
fn similar_suggestions_with_reasons_and_ignore() {
    let mut s = store_with_files();
    s.add_tags(
        &[1],
        &["groove".into(), "lofi".into(), "hat".into(), "drum".into()],
    )
    .unwrap();
    s.add_tags(
        &[2],
        &["Grove".into(), "lo-fi".into(), "hats".into(), "bass".into()],
    )
    .unwrap();
    let pairs = s.similar_tags().unwrap();
    let shown: Vec<_> = pairs
        .iter()
        .map(|p| (p.a.as_str(), p.b.as_str(), p.reason))
        .collect();
    assert_eq!(
        shown,
        [
            ("groove", "Grove", SimilarReason::Typo),
            ("lo-fi", "lofi", SimilarReason::Separators)
        ]
    );
    assert_eq!((pairs[0].a_files, pairs[0].b_files), (1, 1));
    s.ignore_similar_tags("GROVE", " groove ").unwrap();
    s.ignore_similar_tags("grove", "groove").unwrap(); // idempotent
    assert_eq!(s.similar_tags().unwrap().len(), 1);
    assert!(s.ignore_similar_tags("a b", "A  B").is_err());
}

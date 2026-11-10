//! Store tests against the real schema (migration 0001 and later).

use super::*;
use rusqlite::params;

fn mem() -> Store {
    Store::open_in_memory().expect("open in-memory store")
}

/// `(table, columns in declaration order)` for the whole schema.
const SCHEMA: &[(&str, &[&str])] = &[
    (
        "libraries",
        &[
            "id",
            "root_path",
            "volume_uuid",
            "bookmark",
            "online",
            "added_at",
        ],
    ),
    (
        "files",
        &[
            "id",
            "library_id",
            "rel_path",
            "size",
            "mtime",
            "quick_hash",
            "content_hash",
            "status",
            "analysis_version",
            "error",
        ],
    ),
    (
        "audio_meta",
        &[
            "file_id",
            "duration_ms",
            "sample_rate",
            "channels",
            "bit_depth",
            "peak_db",
            "loudness_lufs",
        ],
    ),
    (
        "analysis",
        &[
            "file_id",
            "kind",
            "kind_conf",
            "bpm",
            "bpm_conf",
            "bpm_source",
            "key",
            "key_conf",
            "key_source",
            "pitch_midi",
            "pitch_conf",
            "category",
            "category_conf",
            "category_source",
        ],
    ),
    ("overrides", &["file_id", "field", "value"]),
    ("tags", &["id", "name"]),
    ("file_tags", &["file_id", "tag_id"]),
    ("collections", &["id", "name", "created_at"]),
    (
        "collection_items",
        &["collection_id", "file_id", "position"],
    ),
    ("favorites", &["file_id", "added_at"]),
    ("hidden", &["file_id"]),
    (
        "file_stages",
        &[
            "file_id", "stage", "version", "outcome", "message", "done_at",
        ],
    ),
];

fn columns(store: &Store, table: &str) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .expect("prepare");
    let rows = stmt
        .query_map([table], |r| r.get::<_, String>(0))
        .expect("query");
    rows.collect::<rusqlite::Result<_>>().expect("rows")
}

fn count(store: &Store, table: &str) -> i64 {
    store
        .conn()
        .query_row(
            &format!("SELECT count(*) FROM {}", quote_ident(table)),
            [],
            |r| r.get(0),
        )
        .expect("count")
}

/// Inserts library 1 with file 1 and returns the store.
fn store_with_file() -> Store {
    let store = mem();
    store
        .conn()
        .execute_batch(
            "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/samples', 0);
             INSERT INTO files (id, library_id, rel_path, size, mtime)
                 VALUES (1, 1, 'kick.wav', 100, 0);",
        )
        .expect("seed");
    store
}

#[test]
fn fresh_store_is_at_latest_version() {
    let store = mem();
    assert_eq!(store.schema_version().ok(), Some(LATEST_VERSION));
    assert_eq!(store.path(), None);
    assert_eq!(store.journal_mode().ok().as_deref(), Some("memory"));
}

#[test]
fn schema_has_exactly_the_documented_tables_and_columns() {
    let store = mem();
    let tables: Vec<String> = store
        .table_counts()
        .expect("counts")
        .into_iter()
        .map(|t| t.name)
        .collect();
    let mut expected: Vec<&str> = SCHEMA.iter().map(|(t, _)| *t).collect();
    expected.sort_unstable();
    assert_eq!(tables, expected);
    for (table, cols) in SCHEMA {
        assert_eq!(columns(&store, table), *cols, "columns of {table}");
    }
}

#[test]
fn connection_pragmas_are_applied() {
    let store = mem();
    let conn = store.conn();
    let fk: i64 = conn
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .expect("fk");
    let sync: i64 = conn
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .expect("sync");
    let busy: i64 = conn
        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
        .expect("busy");
    assert_eq!(fk, 1);
    assert_eq!(sync, 1, "NORMAL");
    assert_eq!(busy, 5000);
}

#[test]
fn file_store_uses_wal_and_creates_parent_dirs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("nested/dir").join(paths::DB_FILE_NAME);
    let store = Store::open(&path).expect("open");
    assert_eq!(store.journal_mode().ok().as_deref(), Some("wal"));
    assert_eq!(store.path(), Some(path.as_path()));
    assert!(path.is_file());
    let info = store.info().expect("info");
    assert_eq!(info.journal_mode, "wal");
    assert_eq!(info.schema_version, LATEST_VERSION);
    assert!(info.tables.iter().all(|t| t.rows == 0));
}

#[test]
fn reopening_keeps_version_and_data() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join(paths::DB_FILE_NAME);
    {
        let store = Store::open(&path).expect("first open");
        store
            .conn()
            .execute(
                "INSERT INTO libraries (root_path, added_at) VALUES (?1, ?2)",
                params!["/samples", 1_700_000_000_i64],
            )
            .expect("insert");
    }
    for _ in 0..2 {
        // A re-run of 0001 would fail (CREATE TABLE on existing tables).
        let store = Store::open(&path).expect("reopen");
        assert_eq!(store.schema_version().ok(), Some(LATEST_VERSION));
        assert_eq!(store.journal_mode().ok().as_deref(), Some("wal"));
        assert_eq!(count(&store, "libraries"), 1);
    }
}

#[test]
fn deleting_a_library_cascades_to_everything_about_its_files() {
    let store = store_with_file();
    store
        .conn()
        .execute_batch(
            "INSERT INTO audio_meta (file_id, duration_ms) VALUES (1, 500);
             INSERT INTO analysis (file_id, bpm, bpm_source) VALUES (1, 120.0, 'detected');
             INSERT INTO overrides (file_id, field, value) VALUES (1, 'bpm', '121');
             INSERT INTO tags (id, name) VALUES (1, 'punchy');
             INSERT INTO file_tags (file_id, tag_id) VALUES (1, 1);
             INSERT INTO collections (id, name, created_at) VALUES (1, 'set', 0);
             INSERT INTO collection_items (collection_id, file_id, position) VALUES (1, 1, 0);
             INSERT INTO favorites (file_id, added_at) VALUES (1, 0);
             INSERT INTO hidden (file_id) VALUES (1);
             DELETE FROM libraries WHERE id = 1;",
        )
        .expect("seed + delete");
    for table in [
        "files",
        "audio_meta",
        "analysis",
        "overrides",
        "file_tags",
        "collection_items",
        "favorites",
        "hidden",
    ] {
        assert_eq!(count(&store, table), 0, "{table} not cascaded");
    }
    // Tags and collections are not owned by a file.
    assert_eq!(count(&store, "tags"), 1);
    assert_eq!(count(&store, "collections"), 1);
}

#[test]
fn foreign_keys_reject_orphans() {
    let store = mem();
    let orphan = store.conn().execute(
        "INSERT INTO files (library_id, rel_path, size, mtime) VALUES (42, 'x.wav', 1, 0)",
        [],
    );
    assert!(orphan.is_err());
}

#[test]
fn check_constraints_reject_bad_values() {
    let store = store_with_file();
    let conn = store.conn();
    let bad = [
        "UPDATE files SET status = 'done' WHERE id = 1",
        "INSERT INTO analysis (file_id, bpm_source) VALUES (1, 'guess')",
        "INSERT INTO analysis (file_id, key_source) VALUES (1, 'Detected')",
        "INSERT INTO analysis (file_id, category_source) VALUES (1, '')",
        "INSERT INTO analysis (file_id, kind) VALUES (1, 'oneshot')",
        "INSERT INTO analysis (file_id, kind_conf) VALUES (1, 1.5)",
        "INSERT INTO libraries (root_path, online, added_at) VALUES ('/x', 2, 0)",
        // STRICT: wrong storage class.
        "INSERT INTO files (library_id, rel_path, size, mtime) VALUES (1, 'y', 'big', 0)",
        "INSERT INTO files (library_id, rel_path, size, mtime) VALUES (1, 'kick.wav', 1, 0)",
    ];
    for sql in bad {
        assert!(conn.execute(sql, []).is_err(), "accepted: {sql}");
    }
    let good = [
        "UPDATE files SET status = 'analyzed' WHERE id = 1",
        "INSERT INTO analysis (file_id, kind, bpm, bpm_source, key, key_source)
             VALUES (1, 'one_shot', 90.0, 'filename', 'A minor', NULL)",
    ];
    for sql in good {
        assert!(conn.execute(sql, []).is_ok(), "rejected: {sql}");
    }
}

#[test]
fn override_values_keep_their_type() {
    let store = store_with_file();
    let conn = store.conn();
    let types: Vec<String> = [
        ("bpm", rusqlite::types::Value::Real(121.5)),
        ("pitch_midi", rusqlite::types::Value::Integer(60)),
        ("key", rusqlite::types::Value::Text("A minor".into())),
        ("category", rusqlite::types::Value::Null),
    ]
    .into_iter()
    .map(|(field, value)| {
        conn.execute(
            "INSERT INTO overrides (file_id, field, value) VALUES (1, ?1, ?2)",
            params![field, value],
        )
        .expect("insert override");
        conn.query_row(
            "SELECT typeof(value) FROM overrides WHERE file_id = 1 AND field = ?1",
            [field],
            |r| r.get(0),
        )
        .expect("typeof")
    })
    .collect();
    assert_eq!(types, ["real", "integer", "text", "null"]);
}

#[test]
fn concurrent_first_opens_all_succeed() {
    use std::sync::{Arc, Barrier};
    const THREADS: usize = 8;
    // Several rounds: the WAL-switch race does not show up every time.
    const ROUNDS: usize = 10;
    let tmp = tempfile::tempdir().expect("tempdir");
    for round in 0..ROUNDS {
        let path = tmp.path().join(format!("{round}-{}", paths::DB_FILE_NAME));
        let barrier = Arc::new(Barrier::new(THREADS));
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let (path, barrier) = (path.clone(), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    let store = Store::open(&path)?;
                    Ok::<_, Error>((store.schema_version()?, store.journal_mode()?))
                })
            })
            .collect();
        for handle in handles {
            let (version, mode) = handle
                .join()
                .expect("thread panicked")
                .unwrap_or_else(|e| panic!("round {round}: concurrent open failed: {e:?}"));
            assert_eq!(version, LATEST_VERSION);
            assert_eq!(mode, "wal");
        }
    }
}

fn busy_error() -> Error {
    Error::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
        None,
    ))
}

#[test]
fn retry_busy_retries_only_lock_conflicts() {
    let mut calls = 0;
    let result = retry_busy(|| {
        calls += 1;
        if calls < 3 {
            Err(busy_error())
        } else {
            Ok(calls)
        }
    });
    assert_eq!(result.ok(), Some(3));

    let mut calls = 0;
    let result: Result<()> = retry_busy(|| {
        calls += 1;
        Err(Error::NoDataDir)
    });
    assert!(matches!(result, Err(Error::NoDataDir)));
    assert_eq!(calls, 1, "non-busy errors are not retried");
    assert!(is_busy(&busy_error()));
}

#[test]
fn tag_names_are_unique_case_insensitively() {
    let store = mem();
    let conn = store.conn();
    assert!(conn
        .execute("INSERT INTO tags (name) VALUES ('Punchy')", [])
        .is_ok());
    assert!(conn
        .execute("INSERT INTO tags (name) VALUES ('punchy')", [])
        .is_err());
}

#[test]
fn newer_schema_is_refused_and_file_left_untouched() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join(paths::DB_FILE_NAME);
    {
        let conn = Connection::open(&path).expect("raw open");
        conn.execute_batch("CREATE TABLE future (x INTEGER); PRAGMA user_version = 99;")
            .expect("seed");
    }
    let before = fs::read(&path).expect("read before");
    let result = Store::open(&path);
    assert!(
        matches!(
            result,
            Err(Error::UnsupportedSchema {
                found: 99,
                supported: LATEST_VERSION
            })
        ),
        "{result:?}"
    );
    assert_eq!(fs::read(&path).expect("read after"), before);
    assert!(!tmp.path().join("sonar.db-wal").exists(), "WAL not enabled");
}

#[test]
fn non_database_file_is_an_error_and_left_untouched() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join(paths::DB_FILE_NAME);
    let junk = vec![0x5a_u8; 4096];
    fs::write(&path, &junk).expect("write junk");
    let result = Store::open(&path);
    assert!(matches!(result, Err(Error::Sqlite(_))), "{result:?}");
    assert_eq!(fs::read(&path).expect("read after"), junk);
}

#[test]
fn directory_path_is_an_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(Store::open(tmp.path()).is_err());
}

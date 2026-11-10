//! Schema migrations keyed on `PRAGMA user_version`.
//!
//! Migration `i` (0-based) in [`MIGRATIONS`] upgrades the schema from version
//! `i` to `i + 1`. Each one runs in its own `BEGIN IMMEDIATE` transaction that
//! also bumps `user_version`, so a failure leaves the database at the previous
//! version with no partial changes. Migrations are append-only: never edit or
//! reorder a shipped one, add a new file instead.
//!
//! Migrations run with `foreign_keys=OFF` ([`migrate`] switches it off; the
//! pragma is a no-op inside a transaction, so it is set before the first
//! step). That makes the standard table rebuild safe — create `x_new`, copy,
//! `DROP TABLE x`, rename `x_new` to `x`
//! (<https://www.sqlite.org/lang_altertable.html#otheralter>) — because the
//! DROP no longer cascades into user data. Integrity is still enforced: each
//! step runs `PRAGMA foreign_key_check` before COMMIT and is rolled back if
//! any row violates a foreign key. The caller re-enables foreign keys
//! afterwards (`Store` does, and verifies it). The check scans the whole
//! database; on very large libraries (~500k files) a step may take seconds —
//! acceptable for rare migrations, revisit (per-table check) if it hurts.

use rusqlite::{ffi, Connection, TransactionBehavior};

use crate::{Error, Result};

/// One schema step: plain SQL, no `BEGIN`/`COMMIT`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Migration {
    /// Short name used in errors, e.g. `0001_init`.
    pub name: &'static str,
    /// SQL batch executed inside the migration transaction.
    pub sql: &'static str,
}

/// Every migration this build knows, in order.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "0001_init",
        sql: include_str!("migrations/0001_init.sql"),
    },
    Migration {
        name: "0002_file_stages",
        sql: include_str!("migrations/0002_file_stages.sql"),
    },
];

/// Latest schema version this build can create and read.
pub const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

/// Reads `PRAGMA user_version`.
pub(crate) fn user_version(conn: &Connection) -> Result<i64> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Fails with [`Error::UnsupportedSchema`] unless `0 <= found <= latest`.
pub(crate) fn ensure_supported(found: i64, latest: i64) -> Result<()> {
    if (0..=latest).contains(&found) {
        Ok(())
    } else {
        Err(Error::UnsupportedSchema {
            found,
            supported: latest,
        })
    }
}

/// First foreign key violation reported by `PRAGMA foreign_key_check`,
/// described as `child -> parent`, or `None` when the schema is consistent.
fn foreign_key_violation(conn: &Connection) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let child: String = row.get(0)?;
    let rowid: Option<i64> = row.get(1)?;
    let parent: String = row.get(2)?;
    let rowid = rowid.map_or_else(|| "?".to_owned(), |id| id.to_string());
    Ok(Some(format!(
        "foreign_key_check: {child} rowid {rowid} references a missing {parent} row"
    )))
}

/// SQLite-style error for a violation found by [`foreign_key_violation`].
fn foreign_key_error(detail: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        ffi::Error::new(ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
        Some(detail),
    )
}

/// Brings the database up to `migrations.len()`, returning how many
/// migrations were applied (0 when already current).
///
/// Leaves `foreign_keys` OFF on `conn`; the caller must switch it back on.
/// Safe against concurrent openers: the version is re-read under the write
/// lock before each step, so a migration never runs twice.
///
/// # Errors
/// [`Error::UnsupportedSchema`] if the database is newer than `migrations`
/// (nothing is written), [`Error::Migration`] if a step fails or leaves a
/// foreign key violation (that step is rolled back; earlier steps stay
/// committed), [`Error::Sqlite`] otherwise.
pub(crate) fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<usize> {
    conn.pragma_update(None, "foreign_keys", false)?;
    let latest = i64::try_from(migrations.len()).unwrap_or(i64::MAX);
    let current = user_version(conn)?;
    ensure_supported(current, latest)?;
    if current == latest {
        return Ok(0);
    }

    let mut applied = 0;
    loop {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = user_version(&tx)?;
        ensure_supported(current, latest)?;
        // `current` is in 0..=latest here, so the conversion cannot fail.
        let next = usize::try_from(current)
            .ok()
            .and_then(|i| migrations.get(i));
        let Some(step) = next else {
            return Ok(applied); // up to date (tx rolls back: nothing written)
        };
        let version = current + 1;
        let fail = |source| Error::Migration {
            version,
            name: step.name,
            source,
        };
        tx.execute_batch(step.sql).map_err(fail)?;
        if let Some(detail) = foreign_key_violation(&tx).map_err(fail)? {
            return Err(fail(foreign_key_error(detail))); // tx rolls back on drop
        }
        tx.pragma_update(None, "user_version", version)
            .map_err(fail)?;
        tx.commit().map_err(fail)?;
        applied += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        match Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => panic!("open in-memory db: {e}"),
        }
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n == 1)
        .unwrap_or(false)
    }

    const GOOD: Migration = Migration {
        name: "0001_a",
        sql: "CREATE TABLE a (x INTEGER PRIMARY KEY);",
    };
    const BROKEN: Migration = Migration {
        name: "0002_broken",
        // First statement succeeds, second is a syntax error: the whole
        // step must roll back.
        sql: "CREATE TABLE b (x INTEGER); CREATE TABLE oops (;",
    };

    #[test]
    fn latest_version_matches_list() {
        assert_eq!(LATEST_VERSION, MIGRATIONS.len() as i64);
        assert_eq!(MIGRATIONS.first().map(|m| m.name), Some("0001_init"));
        assert_eq!(MIGRATIONS.get(1).map(|m| m.name), Some("0002_file_stages"));
    }

    #[test]
    fn applies_all_in_order_then_is_idempotent() {
        let mut conn = mem();
        let second = Migration {
            name: "0002_b",
            sql: "CREATE TABLE b (y INTEGER REFERENCES a(x));",
        };
        let list = [GOOD, second];
        assert_eq!(migrate(&mut conn, &list).ok(), Some(2));
        assert_eq!(user_version(&conn).ok(), Some(2));
        assert!(table_exists(&conn, "a") && table_exists(&conn, "b"));
        // Re-running applies nothing and leaves the version alone.
        assert_eq!(migrate(&mut conn, &list).ok(), Some(0));
        assert_eq!(user_version(&conn).ok(), Some(2));
    }

    #[test]
    fn resumes_from_current_version() {
        let mut conn = mem();
        assert_eq!(migrate(&mut conn, &[GOOD]).ok(), Some(1));
        let more = [
            GOOD,
            Migration {
                name: "0002_c",
                sql: "CREATE TABLE c (z TEXT);",
            },
        ];
        // Only the new step runs (re-running GOOD would fail: table exists).
        assert_eq!(migrate(&mut conn, &more).ok(), Some(1));
        assert_eq!(user_version(&conn).ok(), Some(2));
    }

    #[test]
    fn failing_migration_rolls_back_and_keeps_previous_version() {
        let mut conn = mem();
        let result = migrate(&mut conn, &[GOOD, BROKEN]);
        match result {
            Err(Error::Migration { version, name, .. }) => {
                assert_eq!(version, 2);
                assert_eq!(name, "0002_broken");
            }
            other => panic!("expected Error::Migration, got {other:?}"),
        }
        assert_eq!(user_version(&conn).ok(), Some(1));
        assert!(table_exists(&conn, "a"), "step 1 stays committed");
        assert!(!table_exists(&conn, "b"), "step 2 rolled back entirely");
    }

    /// Parent `p` with a cascading child `c` (like libraries -> files).
    const PARENT_CHILD: Migration = Migration {
        name: "0001_pc",
        sql: "CREATE TABLE p (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
              CREATE TABLE c (id INTEGER PRIMARY KEY,
                              p_id INTEGER NOT NULL REFERENCES p(id) ON DELETE CASCADE);",
    };

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(-1)
    }

    #[test]
    fn rebuilding_a_parent_table_keeps_cascading_children() {
        let mut conn = mem();
        assert_eq!(migrate(&mut conn, &[PARENT_CHILD]).ok(), Some(1));
        let seeded = conn.execute_batch(
            "INSERT INTO p (id, name) VALUES (1, 'lib');
             INSERT INTO c (id, p_id) VALUES (10, 1), (11, 1);",
        );
        assert!(seeded.is_ok());
        // As a Store connection would be: FKs on before migrating. With FKs
        // on, the DROP below would delete both children via the cascade.
        assert!(conn.pragma_update(None, "foreign_keys", true).is_ok());
        let rebuild = Migration {
            name: "0002_rebuild_p",
            sql: "CREATE TABLE p_new (id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                                      extra INTEGER NOT NULL DEFAULT 0);
                  INSERT INTO p_new (id, name) SELECT id, name FROM p;
                  DROP TABLE p;
                  ALTER TABLE p_new RENAME TO p;",
        };
        assert_eq!(migrate(&mut conn, &[PARENT_CHILD, rebuild]).ok(), Some(1));
        assert_eq!(user_version(&conn).ok(), Some(2));
        assert_eq!(count(&conn, "c"), 2, "children survived the rebuild");
        assert_eq!(count(&conn, "p"), 1);
        // The child FK now points at the rebuilt table and still cascades.
        assert!(conn.pragma_update(None, "foreign_keys", true).is_ok());
        assert!(conn.execute("DELETE FROM p WHERE id = 1", []).is_ok());
        assert_eq!(count(&conn, "c"), 0);
    }

    #[test]
    fn migration_leaving_an_orphan_fails_and_rolls_back() {
        let mut conn = mem();
        let orphan = Migration {
            name: "0002_orphan",
            sql: "CREATE TABLE marker (x INTEGER);
                  INSERT INTO c (id, p_id) VALUES (9, 999);",
        };
        let result = migrate(&mut conn, &[PARENT_CHILD, orphan]);
        match result {
            Err(Error::Migration {
                version: 2,
                name: "0002_orphan",
                source,
            }) => assert!(source.to_string().contains("foreign_key_check"), "{source}"),
            other => panic!("expected Error::Migration for step 2, got {other:?}"),
        }
        assert_eq!(user_version(&conn).ok(), Some(1));
        assert_eq!(count(&conn, "c"), 0, "orphan row rolled back");
        assert!(!table_exists(&conn, "marker"), "whole step rolled back");
    }

    #[test]
    fn malformed_foreign_key_fails_the_step() {
        let mut conn = mem();
        let bad_fk = Migration {
            name: "0001_bad_fk",
            // Parent column is not unique: "foreign key mismatch".
            sql: "CREATE TABLE p (k INTEGER);
                  CREATE TABLE c (k INTEGER REFERENCES p(k));",
        };
        let result = migrate(&mut conn, &[bad_fk]);
        assert!(
            matches!(result, Err(Error::Migration { version: 1, .. })),
            "{result:?}"
        );
        assert_eq!(user_version(&conn).ok(), Some(0));
        assert!(!table_exists(&conn, "p"));
    }

    #[test]
    fn newer_database_is_rejected_without_writes() {
        let mut conn = mem();
        assert!(conn.pragma_update(None, "user_version", 7).is_ok());
        let result = migrate(&mut conn, &[GOOD]);
        assert!(matches!(
            result,
            Err(Error::UnsupportedSchema {
                found: 7,
                supported: 1
            })
        ));
        assert_eq!(user_version(&conn).ok(), Some(7));
        assert!(!table_exists(&conn, "a"));
    }

    #[test]
    fn negative_version_is_rejected() {
        assert!(ensure_supported(-1, 3).is_err());
        assert!(ensure_supported(0, 3).is_ok());
        assert!(ensure_supported(3, 3).is_ok());
        assert!(ensure_supported(4, 3).is_err());
    }

    #[test]
    fn shipped_migrations_have_no_transaction_control() {
        for m in MIGRATIONS {
            let upper = m.sql.to_ascii_uppercase();
            for word in ["BEGIN", "COMMIT", "ROLLBACK"] {
                let hit = upper
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("--"))
                    .any(|l| {
                        l.split(|c: char| !c.is_ascii_alphanumeric())
                            .any(|t| t == word)
                    });
                assert!(!hit, "{} contains {word}", m.name);
            }
        }
    }
}

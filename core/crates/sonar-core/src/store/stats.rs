//! Store statistics (`sonar stats`): per-library file counts by status,
//! sizes, durations and pipeline stage progress, plus the database size.
//!
//! Read-only and aggregate-only: every number comes from a `GROUP BY` over
//! `files`, `audio_meta` or `file_stages`, never from per-row work, so the
//! cost is one pass over `files` with primary-key lookups.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use super::Store;
use crate::jobs::queue::eligible;
use crate::jobs::Stage;
use crate::Result;

/// File counts of one library by `files.status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StatusCounts {
    /// All rows, including `missing`.
    pub total: u64,
    /// Discovered, nothing read yet.
    pub new: u64,
    /// Basic metadata read.
    pub meta: u64,
    /// DSP analysis done.
    pub analyzed: u64,
    /// Unreadable / failed.
    pub error: u64,
    /// Gone from disk (or volume offline), row kept.
    pub missing: u64,
}

/// Pipeline progress of one stage in one library, at the stage's current
/// version. Files whose result is from an older version count as pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageStats {
    /// The stage.
    pub stage: Stage,
    /// Finished cleanly.
    pub ok: u64,
    /// Finished on a damaged file.
    pub partial: u64,
    /// Failed.
    pub error: u64,
    /// Files still to be processed, by the job queue's own definition
    /// (including `error` files whose stage result is from an older
    /// version). For an offline library this is work that runs once the
    /// volume is online again; the queue skips offline libraries.
    pub pending: u64,
}

/// Statistics of one library.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryStats {
    /// `libraries.id`.
    pub id: i64,
    /// Root directory.
    pub root: String,
    /// False when the root was missing at the last scan.
    pub online: bool,
    /// Rows by status.
    pub files: StatusCounts,
    /// Total size of the files that are not `missing` (bytes).
    pub bytes: u64,
    /// Total duration of files with known duration (ms); `None` if none.
    pub duration_ms: Option<u64>,
    /// Files with a known duration.
    pub files_with_duration: u64,
    /// Runnable stages (`meta`, `dsp`).
    pub stages: Vec<StageStats>,
}

/// Statistics of the whole store.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoreStats {
    /// Database file (`None` for in-memory stores).
    pub path: Option<PathBuf>,
    /// Schema version.
    pub schema_version: i64,
    /// Size of the database file (bytes), without the WAL.
    pub db_bytes: u64,
    /// Size of the `-wal` file (bytes), 0 if absent.
    pub wal_bytes: u64,
    /// One entry per library, by id.
    pub libraries: Vec<LibraryStats>,
}

/// Runnable stages, in priority order.
const STAGES: [Stage; 2] = [Stage::Meta, Stage::Dsp];

/// Non-negative SQL aggregate (possibly NULL) as `u64`.
fn count(v: Option<i64>) -> u64 {
    v.and_then(|n| u64::try_from(n).ok()).unwrap_or(0)
}

/// The single aggregate query: per (library, status) the row count, size,
/// duration, and per stage ok/partial/error at the current version and
/// pending (present files without an up-to-date result).
fn stats_sql() -> String {
    let mut select = String::from(
        "SELECT f.library_id, f.status, count(*), sum(f.size), \
         count(a.duration_ms), sum(a.duration_ms)",
    );
    let mut joins = String::from(" FROM files f LEFT JOIN audio_meta a ON a.file_id = f.id");
    for (i, st) in STAGES.iter().enumerate() {
        let (name, v) = (st.as_str(), st.version());
        let cur = format!("f.status <> 'missing' AND s{i}.version >= {v}");
        for outcome in ["ok", "partial", "error"] {
            select += &format!(", sum({cur} AND s{i}.outcome = '{outcome}')");
        }
        // Aliases/params are spelled out per stage (the macro needs
        // literals); the const assert below forces an update here when a
        // third runnable stage is added to `STAGES`.
        const _: () = assert!(STAGES.len() == 2);
        let eligible = if i == 0 {
            eligible!("s0", "?1")
        } else {
            eligible!("s1", "?2")
        };
        select += &format!(", sum({eligible})");
        joins += &format!(
            " LEFT JOIN file_stages s{i} ON s{i}.file_id = f.id AND s{i}.stage = '{name}'"
        );
    }
    format!("{select}{joins} GROUP BY f.library_id, f.status")
}

impl Store {
    /// Collects [`StoreStats`] with one aggregate query.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`].
    pub fn stats(&self) -> Result<StoreStats> {
        let mut libs: BTreeMap<i64, LibraryStats> = BTreeMap::new();
        for lib in self.libraries()? {
            let stages = STAGES
                .iter()
                .map(|&stage| StageStats {
                    stage,
                    ok: 0,
                    partial: 0,
                    error: 0,
                    pending: 0,
                })
                .collect();
            libs.insert(
                lib.id,
                LibraryStats {
                    id: lib.id,
                    root: lib.root_path.display().to_string(),
                    online: lib.online,
                    files: StatusCounts::default(),
                    bytes: 0,
                    duration_ms: None,
                    files_with_duration: 0,
                    stages,
                },
            );
        }

        // One pass over `files`, with primary-key lookups into `audio_meta`
        // and `file_stages`; grouped by (library, status), so the result has
        // a handful of rows.
        let mut stmt = self.conn().prepare(&stats_sql())?;
        let mut rows = stmt.query(rusqlite::params![STAGES[0].version(), STAGES[1].version()])?;
        while let Some(r) = rows.next()? {
            let lib: i64 = r.get(0)?;
            let status: String = r.get(1)?;
            let Some(l) = libs.get_mut(&lib) else {
                continue;
            };
            let n = count(r.get(2)?);
            l.files.total += n;
            match status.as_str() {
                "new" => l.files.new = n,
                "meta" => l.files.meta = n,
                "analyzed" => l.files.analyzed = n,
                "error" => l.files.error = n,
                "missing" => l.files.missing = n,
                _ => {}
            }
            if status != "missing" {
                l.bytes += count(r.get(3)?);
                let with_duration = count(r.get(4)?);
                if with_duration > 0 {
                    l.files_with_duration += with_duration;
                    *l.duration_ms.get_or_insert(0) += count(r.get(5)?);
                }
            }
            for (i, st) in l.stages.iter_mut().enumerate() {
                let c = 6 + 4 * i;
                st.ok += count(r.get(c)?);
                st.partial += count(r.get(c + 1)?);
                st.error += count(r.get(c + 2)?);
                st.pending += count(r.get(c + 3)?);
            }
        }

        let file_len = |p: &std::path::Path| fs::metadata(p).map_or(0, |m| m.len());
        let (db_bytes, wal_bytes) = self.path().map_or((0, 0), |p| {
            let mut wal = p.as_os_str().to_owned();
            wal.push("-wal");
            (file_len(p), file_len(std::path::Path::new(&wal)))
        });
        Ok(StoreStats {
            path: self.path().map(PathBuf::from),
            schema_version: self.schema_version()?,
            db_bytes,
            wal_bytes,
            libraries: libs.into_values().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store_has_no_libraries() {
        let store = Store::open_in_memory().expect("open");
        let s = store.stats().expect("stats");
        assert!(s.libraries.is_empty());
        assert_eq!(s.db_bytes, 0);
    }

    /// Timing of `stats()` on a million rows (printed).
    #[test]
    #[ignore = "benchmark: cargo test --release -p sonar-core --lib million_rows -- --ignored --nocapture"]
    fn million_rows_stats_is_fast() {
        let tmp = tempfile::tempdir().expect("tmp");
        let store = Store::open(tmp.path().join("t.db")).expect("open");
        let lib = store.add_library(tmp.path()).expect("add");
        let conn = store.conn();
        conn.execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000000) \
             INSERT INTO files (library_id, rel_path, size, mtime, status) \
             SELECT ?1, 'f' || i, 1000, 0, CASE i % 3 WHEN 0 THEN 'meta' ELSE 'analyzed' END FROM n",
            [lib.id],
        )
        .expect("files");
        conn.execute(
            "INSERT INTO audio_meta (file_id, duration_ms) SELECT id, 1000 FROM files",
            [],
        )
        .expect("meta");
        conn.execute(
            "INSERT INTO file_stages SELECT id, 'meta', 1, 'ok', NULL, 0 FROM files",
            [],
        )
        .expect("stages");
        for q in [
            "SELECT library_id, status, count(*), sum(size) FROM files GROUP BY 1, 2",
            "SELECT f.library_id, count(a.duration_ms) FROM files f LEFT JOIN audio_meta a ON a.file_id = f.id GROUP BY 1",
            "SELECT f.library_id, count(s.outcome) FROM files f LEFT JOIN file_stages s ON s.file_id = f.id AND s.stage = 'meta' GROUP BY 1",
        ] {
            let t = std::time::Instant::now();
            let mut st = conn.prepare(q).expect("prep");
            let n = st.query_map([], |_| Ok(())).expect("q").count();
            eprintln!("{:.3} s ({n} rows): {q}", t.elapsed().as_secs_f64());
        }
        let t = std::time::Instant::now();
        let s = store.stats().expect("stats");
        eprintln!("stats over 1M files: {:.3} s", t.elapsed().as_secs_f64());
        assert_eq!(s.libraries[0].files.total, 1_000_000);
    }

    #[test]
    fn pending_matches_the_job_queue_including_error_retries() {
        let tmp = tempfile::tempdir().expect("tmp");
        let store = Store::open(tmp.path().join("t.db")).expect("open");
        let mut ids = Vec::new();
        for name in ["on", "off"] {
            let dir = tmp.path().join(name);
            fs::create_dir(&dir).expect("mkdir");
            ids.push(store.add_library(&dir).expect("add").id);
        }
        let conn = store.conn();
        conn.execute("UPDATE libraries SET online = 0 WHERE id = ?1", [ids[1]])
            .expect("offline");
        let mut n = 0;
        for lib in &ids {
            for st in ["new", "meta", "analyzed", "error", "error", "missing"] {
                n += 1;
                conn.execute(
                    "INSERT INTO files (library_id, rel_path, size, mtime, status) \
                     VALUES (?1, ?2, 100, 0, ?3)",
                    rusqlite::params![lib, format!("f{n}.wav"), st],
                )
                .expect("insert");
            }
        }
        // Stage rows for most files: error rows for `error` files, a mix of
        // outdated (version 0) and current results otherwise.
        for stage in ["meta", "dsp"] {
            conn.execute(
                "INSERT INTO file_stages \
                 SELECT id, ?1, CASE WHEN id % 3 = 2 THEN 0 ELSE 99 END, \
                        CASE WHEN status = 'error' THEN 'error' ELSE 'ok' END, NULL, 0 \
                 FROM files WHERE id % 4 <> 0",
                [stage],
            )
            .expect("stages");
        }
        let s = store.stats().expect("stats");
        for (i, stage) in STAGES.iter().enumerate() {
            let sum = |online: bool| -> u64 {
                s.libraries
                    .iter()
                    .filter(|l| l.online == online)
                    .map(|l| l.stages[i].pending)
                    .sum()
            };
            let queue = crate::jobs::queue::count_pending(conn, *stage).expect("count");
            assert!(queue > 0, "test must exercise pending work");
            assert_eq!(sum(true), queue, "{stage:?}");
            // id 2 (`meta`, outdated row) and id 5 (`error`, outdated error
            // row: retried by the queue); the old SQL counted only id 2.
            assert_eq!(sum(true), 2, "{stage:?}");
            assert!(sum(false) > 0, "offline work is still reported");
        }
    }

    #[test]
    fn counts_statuses_sizes_and_pending() {
        let tmp = tempfile::tempdir().expect("tmp");
        let store = Store::open(tmp.path().join("t.db")).expect("open");
        let lib = tmp.path().join("lib");
        fs::create_dir(&lib).expect("mkdir");
        let lib = store.add_library(&lib).expect("add");
        let conn = store.conn();
        for (i, st) in ["new", "new", "meta", "missing", "error"]
            .iter()
            .enumerate()
        {
            conn.execute(
                "INSERT INTO files (library_id, rel_path, size, mtime, status) \
                 VALUES (?1, ?2, 100, 0, ?3)",
                rusqlite::params![lib.id, format!("f{i}.wav"), st],
            )
            .expect("insert");
        }
        conn.execute(
            "INSERT INTO audio_meta (file_id, duration_ms) VALUES (3, 1500), (4, 9000)",
            [],
        )
        .expect("meta");
        conn.execute(
            "INSERT INTO file_stages VALUES (3, 'meta', ?1, 'ok', NULL, 0)",
            [Stage::Meta.version()],
        )
        .expect("stage");
        let s = store.stats().expect("stats");
        let l = &s.libraries[0];
        assert_eq!(
            l.files,
            StatusCounts {
                total: 5,
                new: 2,
                meta: 1,
                analyzed: 0,
                error: 1,
                missing: 1
            }
        );
        assert_eq!(l.bytes, 400, "missing rows do not count");
        // The missing file's duration is excluded.
        assert_eq!(l.duration_ms, Some(1500));
        assert_eq!(l.files_with_duration, 1);
        let meta = &l.stages[0];
        assert_eq!((meta.stage, meta.ok, meta.pending), (Stage::Meta, 1, 2));
        let dsp = &l.stages[1];
        assert_eq!((dsp.stage, dsp.ok, dsp.pending), (Stage::Dsp, 0, 3));
        assert!(s.db_bytes > 0);
    }
}

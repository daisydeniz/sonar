//! Job pipeline tests on real temp libraries and database files.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;

mod control;
mod vectors;
mod versions;
use tempfile::TempDir;

use super::*;
use crate::fixtures::wav::{encode, SampleFormat, WavSpec};
use crate::scan::scan_library;

/// A temp library of tiny WAVs registered in a file-backed store.
pub(super) struct Env {
    _tmp: TempDir,
    pub root: PathBuf,
    pub db: PathBuf,
}

/// 0.05 s of a 440 Hz tone at half scale, mono 16-bit 8 kHz.
fn wav_bytes() -> Vec<u8> {
    let samples: Vec<f32> = (0..400)
        .map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / 8000.0).sin())
        .collect();
    encode(&WavSpec::new(8000, 1, SampleFormat::Int16), &samples).bytes
}

impl Env {
    /// `good` valid files `s000.wav..` plus `bad` garbage files `x000.wav..`.
    pub fn new(good: usize, bad: usize) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("lib");
        fs::create_dir(&root).expect("mkdir");
        let bytes = wav_bytes();
        for i in 0..good {
            fs::write(root.join(format!("s{i:03}.wav")), &bytes).expect("write");
        }
        for i in 0..bad {
            fs::write(root.join(format!("x{i:03}.wav")), b"not audio at all").expect("write");
        }
        let db = tmp.path().join("sonar.db");
        let mut store = Store::open(&db).expect("store");
        let lib = store.add_library(&root).expect("library");
        scan_library(&mut store, lib.id).expect("scan");
        Self {
            _tmp: tmp,
            root: lib.root_path,
            db,
        }
    }

    pub fn conn(&self) -> Connection {
        Connection::open(&self.db).expect("open db")
    }

    pub fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).expect("count")
    }

    pub fn run(&self, opts: JobOptions) -> JobSummary {
        start(&self.db, opts).expect("start").wait().expect("job")
    }
}

pub(super) fn opts(threads: usize, batch: usize) -> JobOptions {
    JobOptions {
        threads,
        batch_size: batch,
        ..JobOptions::default()
    }
}

#[test]
fn full_run_records_every_stage_once() {
    let env = Env::new(10, 2);
    let s = env.run(opts(2, 4));
    assert!(!s.cancelled);
    assert_eq!(s.stages.len(), 2);
    let (meta, dsp) = (&s.stages[0], &s.stages[1]);
    assert_eq!(
        (meta.stage, meta.total, meta.ok, meta.errors),
        (Stage::Meta, 12, 10, 2)
    );
    // Files that failed `meta` have status error and are not offered to dsp.
    assert_eq!(
        (dsp.stage, dsp.total, dsp.ok, dsp.errors),
        (Stage::Dsp, 10, 10, 0)
    );

    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE stage='meta'"),
        12
    );
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE stage='dsp'"),
        10
    );
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE outcome='error' AND message IS NOT NULL"),
        2
    );
    assert_eq!(env.count("SELECT count(*) FROM audio_meta"), 10);
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE status='analyzed'"),
        10
    );
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE status='error' AND error IS NOT NULL"),
        2
    );
    assert_eq!(
        env.count("SELECT count(*) FROM audio_meta WHERE peak_db IS NULL OR loudness_lufs IS NULL"),
        0
    );

    // Nothing left to do: a second run is a no-op.
    let again = env.run(opts(2, 4));
    assert!(again.stages.iter().all(|s| s.total == 0 && s.done == 0));
}

#[test]
fn meta_stage_leaves_levels_empty_and_dsp_fills_them() {
    let env = Env::new(3, 0);
    let s = env.run(JobOptions {
        stages: vec![Stage::Meta],
        ..opts(1, 8)
    });
    assert_eq!(s.stages.len(), 1);
    assert_eq!(
        env.count("SELECT count(*) FROM audio_meta WHERE duration_ms > 0"),
        3
    );
    assert_eq!(
        env.count("SELECT count(*) FROM audio_meta WHERE peak_db IS NOT NULL"),
        0
    );
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE status='meta'"),
        3
    );

    env.run(JobOptions {
        stages: vec![Stage::Dsp],
        ..opts(1, 8)
    });
    assert_eq!(
        env.count("SELECT count(*) FROM audio_meta WHERE peak_db IS NOT NULL"),
        3
    );
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE status='analyzed'"),
        3
    );
}

#[test]
fn embed_is_declared_but_skipped() {
    let env = Env::new(2, 0);
    let s = env.run(JobOptions {
        stages: vec![Stage::Embed, Stage::Meta],
        ..opts(1, 8)
    });
    assert_eq!(s.stages.len(), 1);
    assert_eq!(s.stages[0].stage, Stage::Meta);
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE stage='embed'"),
        0
    );
}

#[test]
fn stages_run_strictly_in_order_across_the_store() {
    let env = Env::new(20, 0);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);
    env.run(JobOptions {
        // Requested out of order on purpose.
        stages: vec![Stage::Dsp, Stage::Meta],
        debug_before_file: Some(Arc::new(move |stage, _| {
            if let Ok(mut v) = log.lock() {
                v.push(stage);
            }
        })),
        on_event: Some(Arc::new(move |e| {
            if let Ok(mut v) = ev.lock() {
                v.push(e.clone());
            }
        })),
        ..opts(4, 3)
    });
    let seen = seen.lock().expect("lock").clone();
    assert_eq!(seen.len(), 40);
    let first_dsp = seen.iter().position(|s| *s == Stage::Dsp).expect("dsp ran");
    assert_eq!(first_dsp, 20, "all meta work precedes the first dsp file");
    assert!(seen[first_dsp..].iter().all(|s| *s == Stage::Dsp));

    let events = events.lock().expect("lock");
    let started: Vec<Stage> = events
        .iter()
        .filter_map(|e| match e {
            JobEvent::StageStarted { stage, .. } => Some(*stage),
            _ => None,
        })
        .collect();
    assert_eq!(started, [Stage::Meta, Stage::Dsp]);
    assert_eq!(events.last(), Some(&JobEvent::Finished));
    let progress = events
        .iter()
        .filter(|e| matches!(e, JobEvent::Progress { .. }))
        .count();
    assert_eq!(progress, 14, "one progress event per batch (7 + 7)");
}

#[test]
fn io_error_leaves_file_pending_and_job_continues() {
    let env = Env::new(4, 0);
    fs::remove_file(env.root.join("s001.wav")).expect("rm");
    let s = env.run(opts(1, 8));
    // Meta: 3 ok + 1 skipped (status stays `new`, so dsp offers it again
    // and skips it again).
    assert_eq!(s.stages[0].ok, 3);
    assert_eq!(s.stages[0].skipped, 1);
    assert_eq!(s.stages[0].errors, 0);
    assert_eq!(s.stages[1].skipped, 1);
    assert_eq!(
        env.count("SELECT count(*) FROM file_stages WHERE outcome='error'"),
        0
    );
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE rel_path='s001.wav' AND status='new'"),
        1
    );
    // Back on the next run (the drive is "still offline"): pending again.
    let again = env.run(JobOptions {
        stages: vec![Stage::Meta],
        ..opts(1, 8)
    });
    assert_eq!((again.stages[0].total, again.stages[0].skipped), (1, 1));
}

#[test]
fn offline_libraries_and_missing_files_are_not_eligible() {
    let env = Env::new(3, 0);
    env.conn()
        .execute(
            "UPDATE files SET status='missing' WHERE rel_path='s000.wav'",
            [],
        )
        .expect("update");
    env.conn()
        .execute("UPDATE libraries SET online=0", [])
        .expect("update");
    let s = env.run(opts(1, 8));
    assert!(s.stages.iter().all(|s| s.total == 0));
    env.conn()
        .execute("UPDATE libraries SET online=1", [])
        .expect("update");
    let s = env.run(JobOptions {
        stages: vec![Stage::Meta],
        ..opts(1, 8)
    });
    assert_eq!(s.stages[0].total, 2, "missing file stays out");
}

#[test]
fn result_is_dropped_when_the_file_changes_between_claim_and_commit() {
    let env = Env::new(3, 0);
    let db = env.db.clone();
    let hit = Arc::new(AtomicUsize::new(0));
    let hit2 = Arc::clone(&hit);
    let s = env.run(JobOptions {
        stages: vec![Stage::Meta],
        debug_before_file: Some(Arc::new(move |_, path| {
            if path.ends_with("s001.wav") && hit2.fetch_add(1, Ordering::SeqCst) == 0 {
                // A concurrent scan sees new content: resets the row.
                if let Ok(c) = Connection::open(&db) {
                    let _ = c.busy_timeout(Duration::from_secs(5));
                    let _ = c.execute(
                        "UPDATE files SET size = size + 1, status = 'new' \
                         WHERE rel_path = 's001.wav'",
                        [],
                    );
                }
            }
        })),
        ..opts(1, 8)
    });
    assert_eq!(s.stages[0].ok, 2);
    assert_eq!(s.stages[0].skipped, 1);
    assert_eq!(
        env.count(
            "SELECT count(*) FROM file_stages fs JOIN files f ON f.id = fs.file_id \
                   WHERE f.rel_path = 's001.wav'"
        ),
        0,
        "stale result not applied"
    );
    assert_eq!(env.count("SELECT count(*) FROM audio_meta"), 2);
}

#[test]
fn truncated_file_is_partial_with_a_message() {
    let env = Env::new(1, 0);
    let path = env.root.join("s000.wav");
    let mut bytes = fs::read(&path).expect("read");
    bytes.truncate(bytes.len() - 300); // header still claims the full length
    fs::write(&path, &bytes).expect("write");
    let mut store = Store::open(&env.db).expect("store");
    scan_library(&mut store, 1).expect("rescan");
    drop(store);
    env.run(JobOptions {
        stages: vec![Stage::Dsp],
        ..opts(1, 8)
    });
    let (outcome, msg): (String, Option<String>) = env
        .conn()
        .query_row("SELECT outcome, message FROM file_stages", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("row");
    assert_eq!(outcome, "partial");
    assert!(msg.is_some_and(|m| !m.is_empty()));
    assert_eq!(
        env.count("SELECT count(*) FROM files WHERE status='analyzed'"),
        1
    );
}

#[test]
fn start_fails_cleanly_on_an_unusable_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = start(tmp.path(), JobOptions::default()).expect_err("directory is no database");
    assert!(!err.to_string().is_empty());
}

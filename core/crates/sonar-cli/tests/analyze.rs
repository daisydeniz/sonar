//! `sonar analyze` end to end, including the T1.5 acceptance criterion:
//! kill -9 mid-run, run again, and the store ends complete with no
//! duplicates and no redone work.

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};

const GOOD: usize = 240;
const BAD: usize = 5;

struct Setup {
    _tmp: tempfile::TempDir,
    home: PathBuf,
}

impl Setup {
    /// `GOOD` tiny valid WAVs and `BAD` garbage files, scanned into a fresh
    /// `SONAR_HOME`.
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lib = tmp.path().join("lib");
        fs::create_dir(&lib).expect("mkdir");
        let tone: Vec<f32> = (0..800).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
        let wav = encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes;
        for i in 0..GOOD {
            fs::write(lib.join(format!("s{i:03}.wav")), &wav).expect("write");
        }
        for i in 0..BAD {
            fs::write(lib.join(format!("x{i:03}.wav")), b"definitely not audio").expect("write");
        }
        let setup = Self {
            home: tmp.path().join("home"),
            _tmp: tmp,
        };
        let out = setup.sonar(&["scan", lib.to_str().expect("utf-8")]);
        assert!(out.status.success(), "scan failed: {out:?}");
        setup
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sonar"));
        c.args(args)
            .env("SONAR_HOME", &self.home)
            .env_remove("RUST_LOG");
        c
    }

    fn sonar(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("spawn sonar")
    }

    fn db(&self) -> Connection {
        let conn = Connection::open(self.home.join("sonar.db")).expect("open db");
        conn.busy_timeout(Duration::from_secs(5)).expect("timeout");
        conn
    }

    fn count(&self, sql: &str) -> i64 {
        self.db().query_row(sql, [], |r| r.get(0)).expect("count")
    }
}

fn summary(out: &Output) -> serde_json::Value {
    assert!(out.status.success(), "analyze failed: {out:?}");
    serde_json::from_slice(&out.stdout).expect("json summary")
}

/// `(total, ok, errors)` of `stage` in an `analyze --json` summary.
fn stage_numbers(summary: &serde_json::Value, stage: &str) -> (u64, u64, u64) {
    let s = summary["stages"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["stage"] == stage))
        .unwrap_or_else(|| panic!("no {stage} stage in {summary}"));
    (
        s["total"].as_u64().expect("total"),
        s["ok"].as_u64().expect("ok"),
        s["errors"].as_u64().expect("errors"),
    )
}

#[test]
fn analyze_processes_everything_and_is_idempotent() {
    let s = Setup::new();
    let first = summary(&s.sonar(&["analyze", "--json", "--threads", "2"]));
    assert_eq!(
        stage_numbers(&first, "meta"),
        ((GOOD + BAD) as u64, GOOD as u64, BAD as u64)
    );
    assert_eq!(stage_numbers(&first, "dsp"), (GOOD as u64, GOOD as u64, 0));
    assert_eq!(first["cancelled"], false);
    assert_eq!(s.count("SELECT count(*) FROM audio_meta"), GOOD as i64);
    assert_eq!(
        s.count("SELECT count(*) FROM files WHERE status = 'analyzed'"),
        GOOD as i64
    );

    let again = summary(&s.sonar(&["analyze", "--json"]));
    assert_eq!(stage_numbers(&again, "meta").0, 0);
    assert_eq!(stage_numbers(&again, "dsp").0, 0);

    let text = s.sonar(&["analyze", "--stages", "meta"]);
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("meta: done 0 of 0"));
}

#[test]
fn analyze_rejects_unknown_stage() {
    let s = Setup::new();
    let out = s.sonar(&["analyze", "--stages", "bogus"]);
    assert!(!out.status.success());
}

/// Waits until `file_stages` has at least `min` rows, or fails.
fn wait_for_rows(s: &Setup, child: &mut Child, min: i64) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if s.count("SELECT count(*) FROM file_stages") >= min {
            return;
        }
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "analyze exited before reaching {min} rows"
        );
        assert!(Instant::now() < deadline, "timed out waiting for rows");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn kill9(child: &mut Child) {
    // `Child::kill` is SIGKILL on unix: no cleanup code runs in the child.
    child.kill().expect("kill");
    child.wait().expect("reap");
}

#[test]
fn kill_9_mid_run_then_resume_finishes_without_duplicates() {
    let s = Setup::new();
    let mut child = s
        .command(&[
            "analyze",
            "--threads",
            "2",
            "--batch",
            "8",
            "--debug-delay-ms",
            "15",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn analyze");
    wait_for_rows(&s, &mut child, 40);
    kill9(&mut child);

    // Partial, consistent state: whole batches only, nothing duplicated.
    let total_rows = (GOOD + BAD + GOOD) as i64;
    let before = s.count("SELECT count(*) FROM file_stages");
    assert!((40..total_rows).contains(&before), "partial: {before}");
    let meta_before = s.count("SELECT count(*) FROM file_stages WHERE stage = 'meta'");
    let dsp_before = s.count("SELECT count(*) FROM file_stages WHERE stage = 'dsp'");
    assert_eq!(meta_before + dsp_before, before);
    assert!(
        meta_before % 8 == 0 || meta_before == (GOOD + BAD) as i64,
        "meta rows are committed in whole batches of 8 (or the last one): {meta_before}"
    );

    // Resume: only what was not committed before the kill is redone.
    let resumed = summary(&s.sonar(&["analyze", "--json", "--threads", "4"]));
    let (meta_total, _, _) = stage_numbers(&resumed, "meta");
    let (dsp_total, _, _) = stage_numbers(&resumed, "dsp");
    assert_eq!(meta_total as i64, (GOOD + BAD) as i64 - meta_before);
    assert_eq!(dsp_total as i64, GOOD as i64 - dsp_before);

    // Complete, exactly one row per file and stage, errors only for the
    // corrupt files, nothing pending.
    assert_eq!(s.count("SELECT count(*) FROM file_stages"), total_rows);
    assert_eq!(
        s.count("SELECT count(*) FROM (SELECT 1 FROM file_stages GROUP BY file_id, stage HAVING count(*) > 1)"),
        0
    );
    assert_eq!(s.count("SELECT count(*) FROM audio_meta"), GOOD as i64);
    assert_eq!(
        s.count("SELECT count(*) FROM file_stages WHERE outcome = 'error'"),
        BAD as i64
    );
    assert_eq!(
        s.count(
            "SELECT count(*) FROM file_stages fs JOIN files f ON f.id = fs.file_id \
             WHERE fs.outcome = 'error' AND f.rel_path NOT LIKE 'x%'"
        ),
        0
    );
    assert_eq!(
        s.count("SELECT count(*) FROM files WHERE status = 'analyzed'"),
        GOOD as i64
    );
    let idle = summary(&s.sonar(&["analyze", "--json"]));
    assert_eq!(
        (
            stage_numbers(&idle, "meta").0,
            stage_numbers(&idle, "dsp").0
        ),
        (0, 0),
        "no file is pending"
    );
}

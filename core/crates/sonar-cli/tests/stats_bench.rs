//! `sonar stats` and `sonar bench ...` end to end (small sizes; numbers are
//! only meaningful from a release build, these tests check plumbing).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};

fn sonar(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env("SONAR_HOME", home)
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn sonar")
}

fn ok_stdout(out: &Output) -> String {
    assert!(out.status.success(), "failed: {out:?}");
    String::from_utf8(out.stdout.clone()).expect("utf-8")
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&ok_stdout(out)).expect("json")
}

/// 7 valid WAVs and 2 garbage files, scanned and fully analyzed.
fn analyzed_library(tmp: &Path) -> std::path::PathBuf {
    let lib = tmp.join("lib");
    fs::create_dir(&lib).expect("mkdir");
    let tone: Vec<f32> = (0..16_000).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
    let wav = encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes;
    for i in 0..7 {
        fs::write(lib.join(format!("s{i}.wav")), &wav).expect("write");
    }
    for i in 0..2 {
        fs::write(lib.join(format!("bad{i}.wav")), b"not audio").expect("write");
    }
    let home = tmp.join("home");
    ok_stdout(&sonar(&home, &["scan", lib.to_str().expect("utf-8")]));
    ok_stdout(&sonar(&home, &["analyze"]));
    home
}

#[test]
fn stats_on_an_empty_store() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let text = ok_stdout(&sonar(&home, &["stats"]));
    assert!(text.contains("no libraries yet"), "{text}");
    let v = json(&sonar(&home, &["stats", "--json"]));
    assert_eq!(v["libraries"], serde_json::json!([]));
    assert!(v["db_bytes"].as_u64().is_some_and(|n| n > 0));
}

#[test]
fn stats_reports_files_stages_and_duration() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = analyzed_library(tmp.path());
    let v = json(&sonar(&home, &["stats", "--json"]));
    assert!(v["schema_version"].as_i64().is_some_and(|n| n >= 2));
    let lib = &v["libraries"][0];
    assert_eq!(lib["online"], true);
    assert_eq!(lib["files"]["total"], 9);
    assert_eq!(lib["files"]["analyzed"], 7);
    assert_eq!(lib["files"]["error"], 2);
    assert_eq!(lib["files"]["missing"], 0);
    assert_eq!(lib["files_with_duration"], 7);
    assert_eq!(lib["duration_ms"], 7_000, "7 files of 1 s");
    assert!(lib["bytes"].as_u64().is_some_and(|n| n > 7 * 32_000));
    let stages = lib["stages"].as_array().expect("stages");
    let names: Vec<_> = stages.iter().map(|s| s["stage"].as_str()).collect();
    assert_eq!(names, [Some("meta"), Some("dsp")]);
    for s in stages {
        assert_eq!(s["ok"], 7, "{s}");
        assert_eq!(s["pending"], 0, "{s}");
    }
    // Files that fail `meta` never reach `dsp`.
    assert_eq!(
        (stages[0]["error"].as_u64(), stages[1]["error"].as_u64()),
        (Some(2), Some(0))
    );

    let text = ok_stdout(&sonar(&home, &["stats"]));
    assert!(text.contains("files: 9 (new 0, meta 0, analyzed 7, error 2, missing 0)"));
    assert!(
        text.contains("meta: ok 7, partial 0, error 2, pending 0"),
        "{text}"
    );
    assert!(text.contains("0:00:07"), "{text}");
}

#[test]
fn stats_shows_pending_before_analysis_and_missing_after_delete() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = analyzed_library(tmp.path());
    let lib = tmp.path().join("lib");
    fs::remove_file(lib.join("s0.wav")).expect("rm");
    fs::write(lib.join("new.wav"), b"x").expect("write");
    ok_stdout(&sonar(&home, &["scan"]));
    let v = json(&sonar(&home, &["stats", "--json"]));
    let l = &v["libraries"][0];
    assert_eq!(l["files"]["missing"], 1);
    assert_eq!(l["files"]["new"], 1);
    // The deleted file's old stage rows no longer count.
    assert_eq!(l["stages"][0]["ok"], 6);
    assert_eq!(l["stages"][0]["pending"], 1);
    assert_eq!(l["stages"][1]["pending"], 1);
}

#[test]
fn bench_scan_smoke() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let out = sonar(
        &home,
        &[
            "bench",
            "scan",
            "--files",
            "300",
            "--dir",
            tmp.path().to_str().expect("utf-8"),
            "--json",
        ],
    );
    let v = json(&out);
    assert_eq!(v["files"], 300);
    assert_eq!(v["first"]["added"], 300);
    assert_eq!(v["noop"]["unchanged"], 300);
    assert_eq!(v["incremental"]["changed"], v["edits"]["changed"]);
    assert!(v["first"]["files_per_sec"]
        .as_f64()
        .is_some_and(|r| r > 0.0));
    assert!(v["machine"]["cores"].as_u64().is_some_and(|c| c >= 1));
    // The user's store was not created, and the working folder is gone.
    assert!(!home.join("sonar.db").exists());
    let leftovers = fs::read_dir(tmp.path())
        .expect("ls")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("sonar-bench-"))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn bench_scan_keep_leaves_the_working_folder() {
    let tmp = tempfile::tempdir().expect("tmp");
    let out = sonar(
        &tmp.path().join("home"),
        &[
            "bench",
            "scan",
            "--files",
            "70",
            "--keep",
            "--dir",
            tmp.path().to_str().expect("utf-8"),
        ],
    );
    let text = ok_stdout(&out);
    assert!(text.contains("first scan"), "{text}");
    let kept: Vec<_> = fs::read_dir(tmp.path())
        .expect("ls")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("sonar-bench-"))
        .collect();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].path().join("bench.db").exists());
}

#[test]
fn bench_decode_smoke_on_fixtures() {
    let tmp = tempfile::tempdir().expect("tmp");
    let fx = tmp.path().join("fx");
    let gen = Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(["fixtures", fx.to_str().expect("utf-8"), "--skip-long"])
        .output()
        .expect("spawn");
    assert!(gen.status.success());
    let out = sonar(
        tmp.path(),
        &[
            "bench",
            "decode",
            "--dir",
            fx.to_str().expect("utf-8"),
            "--threads",
            "2",
            "--repeat",
            "1",
            "--json",
        ],
    );
    let v = json(&out);
    let runs = v["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["meta"], false);
    assert_eq!(runs[1]["meta"], true);
    assert!(runs[1]["realtime_x"].as_f64().is_some_and(|r| r > 0.0));
    assert!(v["files_ok"].as_u64() < v["files_found"].as_u64());
}

#[test]
fn bench_analyze_smoke() {
    let tmp = tempfile::tempdir().expect("tmp");
    let out = sonar(
        &tmp.path().join("home"),
        &[
            "bench",
            "analyze",
            "--files",
            "40",
            "--stages",
            "meta,dsp",
            "--threads",
            "2",
            "--json",
        ],
    );
    let v = json(&out);
    let stages = v["stages"].as_array().expect("stages");
    assert_eq!(stages.len(), 2);
    for s in stages {
        assert_eq!(s["done"], 40);
        assert_eq!(s["errors"], 0);
    }
    assert!(!tmp.path().join("home").join("sonar.db").exists());

    let text = ok_stdout(&sonar(
        tmp.path(),
        &["bench", "analyze", "--files", "20", "--stages", "dsp"],
    ));
    assert!(text.contains("dsp: 20 files"), "{text}");
    assert!(!text.contains("meta:"), "{text}");
}

#[test]
fn bench_watch_smoke() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let v = json(&sonar(
        &home,
        &[
            "bench",
            "watch",
            "--files",
            "200",
            "--copy-files",
            "50",
            "--copy-rate",
            "200",
            "--with-analyze",
            "--stages",
            "meta",
            "--json",
        ],
    ));
    let p = &v["copy_only"];
    assert_eq!(p["caught_up"], true, "{v}");
    assert_eq!(p["verified"], true, "{v}");
    assert_eq!(
        (p["expected_files"].clone(), p["db_files"].clone()),
        (250.into(), 250.into())
    );
    assert_eq!(p["watcher_errors"], 0, "{v}");
    assert_eq!(v["baseline"]["error"], serde_json::Value::Null, "{v}");
    let c = &v["contended"];
    assert_eq!(c["watch"]["verified"], true, "{v}");
    assert_eq!(c["watch"]["expected_files"], 300, "{v}");
    assert_eq!(c["pipeline"]["error"], serde_json::Value::Null, "{v}");
    assert!(!home.join("sonar.db").exists());
}

#[test]
fn bench_rejects_bad_arguments() {
    let tmp = tempfile::tempdir().expect("tmp");
    for args in [
        &["bench", "watch", "--copy-rate", "0"][..],
        &["bench", "watch", "--copy-files", "0"],
        &["bench"][..],
        &["bench", "scan", "--files", "0"],
        &["bench", "analyze", "--stages", "nope"],
        &["bench", "decode", "--repeat", "0"],
    ] {
        let out = sonar(tmp.path(), args);
        assert!(!out.status.success(), "{args:?}");
    }
}

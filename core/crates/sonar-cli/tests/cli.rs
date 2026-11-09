//! End-to-end tests of the `sonar` binary.

use std::process::{Command, Output};

fn sonar(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to spawn sonar binary")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn version_subcommand_prints_version() {
    let out = sonar(&["version"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out).trim(),
        format!("sonar {}", sonar_core::version())
    );
}

#[test]
fn version_flags_match_subcommand() {
    let expected = format!("sonar {}", sonar_core::version());
    for flag in ["--version", "-V"] {
        let out = sonar(&[flag]);
        assert!(out.status.success(), "{flag} failed");
        assert_eq!(stdout(&out).trim(), expected, "{flag}");
    }
}

#[test]
fn no_arguments_prints_usage_and_fails() {
    let out = sonar(&[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage"));
}

#[test]
fn unknown_subcommand_fails() {
    // `eval` in particular must stay unknown until the eval gate lands
    // (scripts/check.sh keys off `sonar eval --help`).
    for args in [&["frobnicate"][..], &["eval", "--help"][..]] {
        let out = sonar(args);
        assert!(!out.status.success(), "{args:?} should fail");
    }
}

#[test]
fn logs_go_to_stderr_not_stdout() {
    let out = Command::new(env!("CARGO_BIN_EXE_sonar"))
        .arg("version")
        .env("RUST_LOG", "debug")
        .output()
        .expect("failed to spawn sonar binary");
    assert!(out.status.success());
    assert_eq!(
        stdout(&out).trim(),
        format!("sonar {}", sonar_core::version())
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("parsed arguments"));
}

#[test]
fn fixtures_writes_manifest_and_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("fx");
    let dir_s = dir.to_str().expect("utf-8 temp path");
    let out = sonar(&["fixtures", dir_s, "--seed", "3", "--skip-long"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(
        text.contains("wrote ") && text.contains("fixtures.json"),
        "{text}"
    );

    let json = std::fs::read(dir.join("fixtures.json")).expect("manifest written");
    let manifest: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
    assert_eq!(manifest["seed"], 3);
    let fixtures = manifest["fixtures"].as_array().expect("fixtures array");
    assert!(fixtures.len() > 100, "{} fixtures", fixtures.len());
    for f in fixtures {
        let rel = f["path"].as_str().expect("path");
        assert!(dir.join(rel).is_file(), "{rel} missing");
    }
}

#[test]
fn fixtures_into_a_file_path_fails_cleanly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("occupied");
    std::fs::write(&file, b"x").expect("write");
    let out = sonar(&["fixtures", file.to_str().expect("utf-8"), "--skip-long"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("generating fixtures"));
}

#[test]
fn invalid_rust_log_falls_back_to_default() {
    let out = Command::new(env!("CARGO_BIN_EXE_sonar"))
        .arg("version")
        .env("RUST_LOG", "=[not a filter")
        .output()
        .expect("failed to spawn sonar binary");
    assert!(out.status.success());
}

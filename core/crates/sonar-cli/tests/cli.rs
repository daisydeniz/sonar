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

/// Runs `sonar store info` with `SONAR_HOME` pointed at `home`.
fn store_info(home: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(["store", "info"])
        .env("SONAR_HOME", home)
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to spawn sonar binary")
}

#[test]
fn store_info_creates_store_under_sonar_home() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("sonar-home");
    for run in 0..2 {
        let out = store_info(&home);
        assert!(
            out.status.success(),
            "run {run} stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = stdout(&out);
        let db = home.join("sonar.db");
        assert!(text.contains(&format!("path: {}", db.display())), "{text}");
        assert!(text.contains("schema version: 1"), "{text}");
        assert!(text.contains("journal mode: wal"), "{text}");
        for table in ["libraries", "files", "analysis", "hidden"] {
            assert!(
                text.lines()
                    .any(|l| l.split_whitespace().eq([table, "0"].into_iter())),
                "{table} row missing: {text}"
            );
        }
        assert!(db.is_file());
    }
}

#[test]
fn store_info_on_garbage_file_fails_cleanly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db = tmp.path().join("sonar.db");
    std::fs::write(&db, vec![0x5a_u8; 4096]).expect("write junk");
    let out = store_info(tmp.path());
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("opening the Sonar store"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
}

/// Runs `sonar <args>` with `SONAR_HOME` pointed at `home`.
fn sonar_in(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env("SONAR_HOME", home)
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to spawn sonar binary")
}

#[test]
fn scan_fixtures_then_rescan_all() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let lib = tmp.path().join("lib");
    let lib_s = lib.to_str().expect("utf-8 temp path");
    let out = sonar_in(&home, &["fixtures", lib_s, "--skip-long"]);
    assert!(out.status.success());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(lib.join("fixtures.json")).expect("manifest"))
            .expect("json");
    // Every fixture has a scanner extension (wav, aif, aiff, flac, ...).
    let wavs = manifest["fixtures"]
        .as_array()
        .expect("fixtures")
        .iter()
        .filter(|f| {
            f["path"].as_str().is_some_and(|p| {
                p.rsplit_once('.')
                    .is_some_and(|(_, e)| sonar_core::scan::AUDIO_EXTENSIONS.contains(&e))
            })
        })
        .count();
    assert_eq!(
        wavs,
        manifest["fixtures"].as_array().expect("fixtures").len()
    );
    assert!(wavs > 100, "{wavs}");

    let out = sonar_in(&home, &["scan", lib_s]);
    let text = stdout(&out);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.starts_with("library 1: "), "{text}");
    assert!(
        text.contains(&format!("added {wavs}, changed 0, moved 0, removed 0")),
        "{text}"
    );
    assert!(!text.contains("issues"), "{text}");

    // Same dir again: same library, nothing new.
    let out = sonar_in(&home, &["scan", lib_s]);
    let text = stdout(&out);
    assert!(text.starts_with("library 1: "), "{text}");
    assert!(text.contains(&format!("unchanged {wavs},")), "{text}");

    // No argument: rescans every library; a vanished root is reported
    // offline, not an error.
    std::fs::rename(&lib, tmp.path().join("gone")).expect("move away");
    let out = sonar_in(&home, &["scan"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("[offline]"), "{text}");
    assert!(text.contains("added 0,"), "{text}");
}

#[test]
fn scan_of_registered_but_vanished_dir_reports_offline() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let lib = tmp.path().join("drive");
    std::fs::create_dir(&lib).expect("mkdir");
    std::fs::write(lib.join("kick.wav"), b"kick").expect("write");
    let lib_s = lib.to_str().expect("utf-8 temp path");
    let out = sonar_in(&home, &["scan", lib_s]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("added 1,"));

    std::fs::remove_dir_all(&lib).expect("unmount");
    let out = sonar_in(&home, &["scan", lib_s]);
    let text = stdout(&out);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.starts_with("library 1: "), "{text}");
    assert!(text.contains("[offline]"), "{text}");
}

#[test]
fn scan_with_no_libraries_says_so() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = sonar_in(tmp.path(), &["scan"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("no libraries"));
}

#[test]
fn scan_of_missing_dir_fails_cleanly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("nope");
    let out = sonar_in(tmp.path(), &["scan", missing.to_str().expect("utf-8")]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("adding library"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
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

/// Writes a 2 s, 48 kHz, 24-bit stereo sine WAV into `dir`.
fn write_test_wav(dir: &std::path::Path) -> std::path::PathBuf {
    use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};
    let spec = WavSpec::new(48_000, 2, SampleFormat::Int24);
    let samples: Vec<f32> = (0..96_000)
        .flat_map(|i| {
            let t = f64::from(i) / 48_000.0;
            let s = (0.5 * (std::f64::consts::TAU * 440.0 * t).sin()) as f32;
            [s, s]
        })
        .collect();
    let path = dir.join("tone.wav");
    std::fs::write(&path, encode(&spec, &samples).bytes).expect("write wav");
    path
}

#[test]
fn decode_prints_source_duration_and_analysis_length() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let wav = write_test_wav(tmp.path());
    let wav_s = wav.to_str().expect("utf-8 temp path");

    let out = sonar(&["decode", wav_s]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    for want in [
        "format: wave / pcm_s24le",
        "source: 48000 Hz, 2 ch, 24-bit",
        "duration: 2.000000 s (96000 frames; header: 96000)",
        "analysis: 44100 samples @ 22050 Hz (2.000 s)",
        "partial: no",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }

    let out = sonar(&["decode", wav_s, "--json", "--max-secs", "1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON output");
    assert_eq!(v["source"]["num_frames"], 96_000);
    assert_eq!(v["source"]["bit_depth"], 24);
    assert_eq!(v["analysis"]["samples"], 22_050);
    assert_eq!(v["truncated_for_analysis"], true);
    assert_eq!(v["partial"], false);

    let out = sonar(&["decode", wav_s, "--json", "--full", "--rate", "16000"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON output");
    assert_eq!(v["analysis"]["samples"], 32_000);
    assert_eq!(v["analysis"]["sample_rate"], 16_000);
}

#[test]
fn decode_of_bad_input_fails_with_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let junk = tmp.path().join("junk.wav");
    std::fs::write(&junk, b"not audio at all").expect("write");
    let missing = tmp.path().join("missing.wav");
    for (path, want) in [(&junk, "unsupported audio format"), (&missing, "I/O error")] {
        let out = sonar(&["decode", path.to_str().expect("utf-8")]);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("decoding") && err.contains(want), "{err}");
    }
}

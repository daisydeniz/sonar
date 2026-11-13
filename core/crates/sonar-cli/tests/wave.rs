//! `sonar wave` end to end in a temp SONAR_HOME: peaks and preview PCM of a
//! scanned library file, by id and by path.

use std::fs;
use std::process::{Command, Output};

use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};

fn sonar(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env("SONAR_HOME", home)
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn sonar")
}

#[test]
fn wave_prints_peaks_and_pcm_summary() {
    let tmp = tempfile::tempdir().expect("tmp");
    let lib = tmp.path().join("lib");
    fs::create_dir_all(&lib).expect("mkdir");
    let tone: Vec<f32> = (0..1600).map(|i| 0.5 * (i as f32 * 0.05).sin()).collect();
    let bytes = encode(&WavSpec::new(8_000, 1, SampleFormat::Int16), &tone).bytes;
    fs::write(lib.join("tone.wav"), bytes).expect("write");
    let home = tmp.path().join("home");
    let scan = sonar(&home, &["scan", lib.to_str().expect("utf-8")]);
    assert!(scan.status.success());

    // By id (first file of a fresh store).
    let out = sonar(&home, &["wave", "1", "--buckets", "16", "--pcm", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(v["peaks"].as_array().expect("peaks").len(), 16);
    assert_eq!(v["pcm"]["sample_rate"], 8_000);
    assert_eq!(v["pcm"]["samples"], 1600);

    // By path, text output.
    let path = lib.join("tone.wav");
    let out = sonar(
        &home,
        &["wave", path.to_str().expect("utf-8"), "--buckets", "4"],
    );
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("4 peak buckets"));

    // Errors: unknown file, bucket count out of range.
    assert!(!sonar(&home, &["wave", "99"]).status.success());
    assert!(!sonar(&home, &["wave", "1", "--buckets", "0"])
        .status
        .success());
}

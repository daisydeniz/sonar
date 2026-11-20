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
    let lanes = v["peaks"].as_array().expect("peaks");
    assert_eq!(lanes.len(), 1, "mono: one channel");
    assert_eq!(lanes[0].as_array().expect("lane").len(), 16);
    assert_eq!(v["pcm"]["sample_rate"], 8_000);
    assert_eq!(v["pcm"]["samples"], 1600);

    // By path, text output.
    let path = lib.join("tone.wav");
    let out = sonar(
        &home,
        &["wave", path.to_str().expect("utf-8"), "--buckets", "4"],
    );
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("1 channel(s) x 4 peak buckets"), "{text}");

    // Stereo: one list per channel.
    let pcm: Vec<f32> = (0..800)
        .flat_map(|i| [0.5 * (i as f32 * 0.05).sin(), 0.0])
        .collect();
    let bytes = encode(&WavSpec::new(8_000, 2, SampleFormat::Int16), &pcm).bytes;
    let stereo = lib.join("stereo.wav");
    fs::write(&stereo, bytes).expect("write");
    assert!(sonar(&home, &["scan", lib.to_str().expect("utf-8")])
        .status
        .success());
    let out = sonar(
        &home,
        &[
            "wave",
            stereo.to_str().expect("utf-8"),
            "--buckets",
            "8",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let lanes = v["peaks"].as_array().expect("peaks");
    assert_eq!(lanes.len(), 2, "stereo: two channels");
    assert!(lanes[1]
        .as_array()
        .expect("right")
        .iter()
        .all(|p| p[1] == 0.0));

    // Errors: unknown file, bucket count out of range.
    assert!(!sonar(&home, &["wave", "99"]).status.success());
    assert!(!sonar(&home, &["wave", "1", "--buckets", "0"])
        .status
        .success());
}

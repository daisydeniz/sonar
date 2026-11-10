//! End-to-end test of `sonar aspects`.

use std::path::Path;
use std::process::{Command, Output};

use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};

fn sonar(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to spawn sonar binary")
}

/// 2 s mono sine at 44.1 kHz.
fn write_tone(path: &Path, freq: f64, amp: f64) {
    let samples: Vec<f32> = (0..88_200)
        .map(|i| (amp * (std::f64::consts::TAU * freq * f64::from(i) / 44_100.0).sin()) as f32)
        .collect();
    let spec = WavSpec::new(44_100, 1, SampleFormat::Int16);
    std::fs::write(path, encode(&spec, &samples).bytes).expect("write wav");
}

#[test]
fn aspects_prints_dims_normalizes_and_reports_bad_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (a, b, c) = (
        tmp.path().join("a440.wav"),
        tmp.path().join("a440_quiet.wav"),
        tmp.path().join("e330.wav"),
    );
    write_tone(&a, 440.0, 0.5);
    write_tone(&b, 440.0, 0.05);
    write_tone(&c, 329.63, 0.5);
    let junk = tmp.path().join("junk.wav");
    std::fs::write(&junk, b"not audio at all").expect("write");
    let s = |p: &Path| p.to_str().expect("utf-8").to_string();
    let (a, b, c, junk) = (s(&a), s(&b), s(&c), s(&junk));

    let out = sonar(&["aspects", &a]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("a440.wav: 2.000 s, spectrum 38, timbre 57, pitch 18, amplitude 22 dims; f0 440.0 Hz (voiced 1.00)"),
        "{text}"
    );
    assert!(!text.contains("normalized"), "{text}");

    let out = sonar(&["aspects", &a, &b, &c]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(
        text.contains("normalized over 3 files (overall 135 dims)"),
        "{text}"
    );

    let out = sonar(&["aspects", "--json", &junk, &a, &b, &c]);
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON output");
    assert!(v[0]["error"].is_string());
    assert_eq!(v[1]["aspects_version"], 1);
    for (name, dim) in [
        ("spectrum", 38),
        ("timbre", 57),
        ("pitch", 18),
        ("amplitude", 22),
    ] {
        assert_eq!(v[1]["raw"][name].as_array().map(Vec::len), Some(dim));
        assert_eq!(v[1]["normalized"][name].as_array().map(Vec::len), Some(dim));
    }
    assert_eq!(
        v[3]["normalized"]["overall"].as_array().map(Vec::len),
        Some(135)
    );
    // The two A440s are closer in Pitch than A440 vs E.
    let pitch = |i: usize| -> Vec<f64> {
        v[i]["normalized"]["pitch"]
            .as_array()
            .expect("pitch")
            .iter()
            .map(|x| x.as_f64().expect("number"))
            .collect()
    };
    let d = |x: &[f64], y: &[f64]| -> f64 {
        x.iter()
            .zip(y)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    assert!(d(&pitch(1), &pitch(2)) < d(&pitch(1), &pitch(3)));
}

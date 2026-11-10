//! End-to-end test of `sonar dsp`.

use std::process::{Command, Output};

fn sonar(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to spawn sonar binary")
}

#[test]
fn dsp_summarizes_files_and_reports_bad_ones() {
    use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};
    let tmp = tempfile::tempdir().expect("tempdir");
    // 2 s mono 440 Hz sine at 44.1 kHz.
    let samples: Vec<f32> = (0..88_200)
        .map(|i| (0.5 * (std::f64::consts::TAU * 440.0 * f64::from(i) / 44_100.0).sin()) as f32)
        .collect();
    let wav = tmp.path().join("tone.wav");
    let spec = WavSpec::new(44_100, 1, SampleFormat::Int16);
    std::fs::write(&wav, encode(&spec, &samples).bytes).expect("write wav");
    let junk = tmp.path().join("junk.wav");
    std::fs::write(&junk, b"not audio at all").expect("write");
    let (wav_s, junk_s) = (wav.to_str().expect("utf-8"), junk.to_str().expect("utf-8"));

    let out = sonar(&["dsp", wav_s]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // 44,100 analysis samples → 1 + 44,100 / 512 = 87 frames.
    assert!(
        text.contains("tone.wav: 2.000 s, 87 frames @ 43.07/s"),
        "{text}"
    );
    assert!(text.contains("mfcc mean: ["), "{text}");

    let out = sonar(&["dsp", "--json", junk_s, wav_s]);
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON output");
    assert!(v[0]["error"].is_string());
    assert_eq!(v[1]["frames"], 87);
    assert_eq!(v[1]["mfcc_mean"].as_array().map(Vec::len), Some(20));
    assert!(v[1]["onset"]["max"].as_f64().is_some_and(|m| m >= 0.0));
}

//! Tests of display rows, waveform peaks and preview PCM through the engine.

use super::*;
use crate::fixtures::wav::{encode, SampleFormat, WavSpec};

#[test]
fn rows_peaks_and_preview_for_a_scanned_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lib = tmp.path().join("lib");
    std::fs::create_dir(&lib).expect("mkdir");
    let tone: Vec<f32> = (0..800).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
    let bytes = encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes;
    std::fs::write(lib.join("kick_one.wav"), bytes).expect("write");

    let engine = Engine::open(tmp.path().join("home")).expect("open");
    engine.add_library(&lib).expect("add");
    let path = lib.join("kick_one.wav").canonicalize().expect("canon");
    let id = engine
        .file_id_by_path(&path)
        .expect("lookup")
        .expect("known");

    let row = engine.row(id).expect("row").expect("exists");
    assert_eq!((row.file_id, row.name.as_str()), (id, "kick_one"));
    assert_eq!(row.path, path);
    assert!(!row.favorite && row.tags.is_empty());
    assert!(engine.row(id + 99).expect("row").is_none());
    assert_eq!(engine.rows(&[id + 99, id]).expect("rows").len(), 1);

    let channels = engine.waveform_peaks(id, 50).expect("peaks");
    assert_eq!(channels.len(), 1, "mono file: one lane");
    let peaks = &channels[0];
    assert_eq!(peaks.len(), 50);
    assert!(peaks.iter().all(|&(lo, hi)| lo <= hi && hi.abs() <= 1.0));
    assert!(peaks.iter().any(|&(_, hi)| hi > 0.2));
    assert!(matches!(
        engine.waveform_peaks(id, 0),
        Err(Error::InvalidInput { .. })
    ));
    assert!(matches!(
        engine.waveform_peaks(id + 99, 10),
        Err(Error::FileNotFound { .. })
    ));

    let pcm = engine.preview_pcm(id).expect("pcm");
    assert_eq!((pcm.sample_rate, pcm.channels), (16_000, 1));
    assert_eq!(pcm.samples.len(), 800);
    assert!(!pcm.truncated);
}

/// Scans one WAV of `channels` interleaved channels and returns its peaks.
fn peaks_of(channels: u16, interleaved: &[f32], buckets: usize) -> Vec<Vec<(f32, f32)>> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lib = tmp.path().join("lib");
    std::fs::create_dir(&lib).expect("mkdir");
    let spec = WavSpec::new(44_100, channels, SampleFormat::Float32);
    std::fs::write(lib.join("multi.wav"), encode(&spec, interleaved).bytes).expect("write");
    let engine = Engine::open(tmp.path().join("home")).expect("open");
    engine.add_library(&lib).expect("add");
    let path = lib.join("multi.wav").canonicalize().expect("canon");
    let id = engine
        .file_id_by_path(&path)
        .expect("lookup")
        .expect("known");
    engine.waveform_peaks(id, buckets).expect("peaks")
}

/// Highest |sample| over a lane.
fn lane_peak(lane: &[(f32, f32)]) -> f32 {
    lane.iter().fold(0.0, |m, &(lo, hi)| m.max(-lo).max(hi))
}

#[test]
fn stereo_gives_one_lane_per_channel() {
    // Left: 0.8 sine for the whole file. Right: silent for the first half,
    // then a 0.1 sine; each lane must follow its own channel.
    let frames = 44_100;
    let tone =
        |i: usize, a: f32| a * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 44_100.0).sin();
    let pcm: Vec<f32> = (0..frames)
        .flat_map(|i| {
            [
                tone(i, 0.8),
                if i < frames / 2 { 0.0 } else { tone(i, 0.1) },
            ]
        })
        .collect();
    let lanes = peaks_of(2, &pcm, 20);
    assert_eq!(lanes.len(), 2);
    assert!(lanes.iter().all(|l| l.len() == 20));
    let (left, right) = (&lanes[0], &lanes[1]);
    // Interior buckets (the resampler edges ring a little).
    for b in 1..19 {
        let l = lane_peak(&left[b..=b]);
        assert!((l - 0.8).abs() < 0.03, "left bucket {b}: {l}");
    }
    for b in 1..9 {
        assert!(lane_peak(&right[b..=b]) < 1e-3, "right bucket {b} silent");
    }
    for b in 11..19 {
        let r = lane_peak(&right[b..=b]);
        assert!((r - 0.1).abs() < 0.01, "right bucket {b}: {r}");
    }
}

#[test]
fn more_than_two_channels_give_the_first_two() {
    // Channels at 0.2 / 0.4 / 0.9 constant-ish level (sines).
    let frames = 22_050;
    let pcm: Vec<f32> = (0..frames)
        .flat_map(|i| {
            let s = (i as f32 * 2.0 * std::f32::consts::PI * 300.0 / 44_100.0).sin();
            [0.2 * s, 0.4 * s, 0.9 * s]
        })
        .collect();
    let lanes = peaks_of(3, &pcm, 10);
    assert_eq!(lanes.len(), 2);
    let (l, r) = (lane_peak(&lanes[0][1..9]), lane_peak(&lanes[1][1..9]));
    assert!((l - 0.2).abs() < 0.01, "left {l}");
    assert!((r - 0.4).abs() < 0.01, "right {r}");
}

/// Peak latency benchmark (T12.3): 30 s of 44.1 kHz stereo 16-bit WAV at the
/// app's 8,192 buckets, median of 5 runs. Run with
/// `cargo test --release -p sonar-core peak_latency -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark"]
fn peak_latency_stereo_30s() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lib = tmp.path().join("lib");
    std::fs::create_dir(&lib).expect("mkdir");
    let frames = 44_100 * 30;
    let mut pcm = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f32 / 44_100.0;
        pcm.push(0.8 * (t * 440.0 * std::f32::consts::TAU).sin());
        pcm.push(0.2 * (t * 220.0 * std::f32::consts::TAU).sin());
    }
    let bytes = encode(&WavSpec::new(44_100, 2, SampleFormat::Int16), &pcm).bytes;
    std::fs::write(lib.join("loop_long.wav"), bytes).expect("write");
    let engine = Engine::open(tmp.path().join("home")).expect("open");
    engine.add_library(&lib).expect("add");
    let path = lib.join("loop_long.wav").canonicalize().expect("canon");
    let id = engine
        .file_id_by_path(&path)
        .expect("lookup")
        .expect("known");
    let mut ms: Vec<f64> = (0..5)
        .map(|_| {
            let t = std::time::Instant::now();
            let p = engine.waveform_peaks(id, 8192).expect("peaks");
            std::hint::black_box(p);
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    eprintln!(
        "waveform_peaks 30 s stereo, 8192 buckets: median {:.2} ms",
        ms[2]
    );
}

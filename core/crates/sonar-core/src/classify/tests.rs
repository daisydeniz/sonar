//! Unit tests on synthetic signals and names; the fixture accuracy test
//! is `tests/classify.rs`.

use std::f64::consts::TAU;
use std::path::Path;

use super::kind::{audio_logit, W_HINT};
use super::*;
use crate::decode::ANALYSIS_RATE;
use crate::dsp::{Analyzer, Features};

const SR: f64 = ANALYSIS_RATE as f64;

fn features(x: &[f32]) -> Features {
    Analyzer::new(ANALYSIS_RATE)
        .expect("analyzer")
        .analyze(x)
        .expect("features")
}

fn secs(x: &[f32]) -> f64 {
    x.len() as f64 / SR
}

fn classify(x: &[f32], hint: Option<Kind>) -> KindResult {
    classify_kind(&features(x), secs(x), hint)
}

/// Exponentially decaying sine (`tau` s), `len_s` long.
fn decaying(freq: f64, tau: f64, len_s: f64) -> Vec<f32> {
    let n = (len_s * SR) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / SR;
            (0.8 * (TAU * freq * t).sin() * (-t / tau).exp()) as f32
        })
        .collect()
}

/// 25 ms 1 kHz clicks every `period` s for `len_s` s (a metronome).
fn click_train(period: f64, len_s: f64) -> Vec<f32> {
    let mut x = vec![0.0f32; (len_s * SR) as usize];
    let click = decaying(1_000.0, 0.005, 0.025);
    let mut k = 0usize;
    loop {
        let at = (k as f64 * period * SR).round() as usize;
        if at >= x.len() {
            break;
        }
        for (d, s) in x[at..].iter_mut().zip(&click) {
            *d += s;
        }
        k += 1;
    }
    x
}

fn sine(freq: f64, len_s: f64) -> Vec<f32> {
    decaying(freq, f64::INFINITY, len_s)
}

fn white(len_s: f64) -> Vec<f32> {
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    (0..(len_s * SR) as usize)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            ((s >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.5
        })
        .collect()
}

#[test]
fn click_train_is_a_confident_loop() {
    let r = classify(&click_train(0.5, 8.0), None);
    assert_eq!(r.kind, Kind::Loop, "{r:?}");
    assert_eq!(r.source, KindSource::Detected);
    assert!(r.confidence > 0.95, "{r:?}");
    assert!(r.scores.periodicity > 0.5, "{r:?}");
    // The beat or a multiple of it (frame-grid quantization decides).
    let beats = r.scores.period_s / 0.5;
    assert!((beats - beats.round()).abs() < 0.06, "{r:?}");
    assert!(r.scores.onsets >= 14, "{r:?}");
    assert!(r.scores.tail_db.is_some_and(|t| t > -6.0), "{r:?}");
}

#[test]
fn decaying_kick_is_a_confident_one_shot() {
    let r = classify(&decaying(55.0, 0.15, 0.8), None);
    assert_eq!(r.kind, Kind::OneShot, "{r:?}");
    assert!(r.confidence > 0.95, "{r:?}");
    assert!(r.scores.periodicity < 0.1, "{r:?}");
    assert!(r.scores.tail_db.is_some_and(|t| t < -20.0), "{r:?}");
}

#[test]
fn steady_tone_and_noise_are_one_shots() {
    for (name, x) in [("sine 3 s", sine(440.0, 3.0)), ("noise 2 s", white(2.0))] {
        let r = classify(&x, None);
        assert_eq!(r.kind, Kind::OneShot, "{name}: {r:?}");
        assert!(r.confidence > 0.75, "{name}: {r:?}");
    }
}

#[test]
fn silence_empty_and_nan_give_a_defined_result() {
    let cases: [(&str, Vec<f32>); 4] = [
        ("empty", vec![]),
        ("one sample", vec![0.0]),
        ("silence", vec![0.0; 22_050]),
        ("nan", vec![f32::NAN; 22_050]),
    ];
    for (name, x) in cases {
        let r = classify(&x, None);
        assert_eq!(r.kind, Kind::OneShot, "{name}");
        assert_eq!(r.source, KindSource::Fallback, "{name}");
        assert_eq!(r.confidence, 0.0, "{name}");
        assert!(r.scores.silent, "{name}");
        let hinted = classify(&x, Some(Kind::Loop));
        assert_eq!(hinted.kind, Kind::Loop, "{name}");
        assert_eq!(hinted.source, KindSource::Filename, "{name}");
        assert!(hinted.confidence > 0.5 && hinted.confidence < 0.9, "{name}");
    }
}

#[test]
fn non_finite_features_and_durations_stay_finite() {
    let mut f = features(&click_train(0.5, 4.0));
    f.onset[3] = f32::NAN;
    f.onset[7] = f32::INFINITY;
    f.spectrogram.power[100] = f32::NAN;
    for d in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e12] {
        let r = classify_kind(&f, d, None);
        let s = r.scores;
        let all = [
            r.confidence,
            r.loop_probability,
            r.audio_logit,
            s.duration_s,
            s.analysis_s,
            s.periodicity,
            s.period_s,
            s.pulse_db,
            s.tail_db.unwrap_or(0.0),
        ];
        assert!(all.iter().all(|v| v.is_finite()), "{d}: {r:?}");
        assert!((0.0..=1.0).contains(&r.confidence));
    }
    // Duration below the window falls back to the window length.
    let r = classify_kind(&f, -1.0, None);
    assert_eq!(r.scores.duration_s, r.scores.analysis_s);
}

#[test]
fn long_file_has_unknown_tail() {
    let x = click_train(0.5, 8.0);
    let r = classify_kind(&features(&x), 240.0, None);
    assert!(r.scores.truncated && r.scores.tail_db.is_none(), "{r:?}");
    assert_eq!(r.scores.duration_s, 240.0);
    assert_eq!(r.kind, Kind::Loop);
}

#[test]
fn filename_hint_decides_only_ambiguous_audio() {
    // A rhythm-less 8 s pad: mildly one-shot on audio alone.
    let pad = sine(220.0, 8.0);
    let alone = classify(&pad, None);
    assert_eq!(alone.kind, Kind::OneShot, "{alone:?}");
    assert!(alone.audio_logit.abs() < W_HINT, "{alone:?}");
    let hinted = classify(&pad, Some(Kind::Loop));
    assert_eq!(hinted.kind, Kind::Loop, "{hinted:?}");
    assert_eq!(hinted.source, KindSource::Filename);
    assert_eq!(hinted.audio_logit, alone.audio_logit);
    // A clear rhythm outweighs a contrary hint.
    let clicks = click_train(0.5, 8.0);
    let r = classify(&clicks, Some(Kind::OneShot));
    assert_eq!(
        (r.kind, r.source),
        (Kind::Loop, KindSource::Detected),
        "{r:?}"
    );
    // An agreeing hint raises confidence.
    let kick = decaying(55.0, 0.15, 0.8);
    let (a, b) = (classify(&kick, None), classify(&kick, Some(Kind::OneShot)));
    assert!(b.confidence > a.confidence && b.source == KindSource::Detected);
}

#[test]
fn deterministic_and_matches_audio_logit() {
    let f = features(&click_train(0.4, 6.0));
    let (a, b) = (classify_kind(&f, 6.0, None), classify_kind(&f, 6.0, None));
    assert_eq!(a, b);
    assert_eq!(a.audio_logit, audio_logit(&a.scores));
    assert_eq!(a.scores, kind_scores(&f, 6.0));
}

#[test]
fn filename_hints() {
    let cases = [
        ("Drums/Kick_OneShot_01.wav", Some(Kind::OneShot)),
        ("Snare One-Shot.wav", Some(Kind::OneShot)),
        ("Hat_1shot.aif", Some(Kind::OneShot)),
        ("FX Hits/Riser.wav", Some(Kind::OneShot)),
        ("Bass Loop 120 Am.wav", Some(Kind::Loop)),
        ("Top_124bpm_Fmin.wav", Some(Kind::Loop)),
        ("Pack/Drum Loops/Top_03.wav", Some(Kind::Loop)),
        ("Loops/Kits/Snare.wav", Some(Kind::Loop)),
        ("Loops/A/B/x.wav", None),   // 3 levels up: too far
        ("Loop One Shot.wav", None), // conflicting
        ("Shot Glass.wav", None),
        ("loops/loop_006.wav", Some(Kind::Loop)),
        ("oneshots/hit_003.wav", Some(Kind::OneShot)),
        ("noise/noise_001.wav", None),
        ("", None),
    ];
    for (p, want) in cases {
        assert_eq!(kind_hint(Path::new(p)), want, "{p}");
    }
}

/// Classifier cost on top of the analyzer, 30 s at 22.05 kHz. Run with
/// `cargo test --release -p sonar-core kind_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing run; use --release"]
fn kind_timing_30s() {
    let mut x = click_train(0.47, 30.0);
    for (v, n) in x.iter_mut().zip(white(30.0)) {
        *v += 0.1 * n;
    }
    let mut an = Analyzer::new(ANALYSIS_RATE).expect("analyzer");
    let f = an.analyze(&x).expect("analyze");
    let runs = 100;
    let start = std::time::Instant::now();
    for _ in 0..runs {
        let r = classify_kind(&f, 30.0, None);
        assert_eq!(r.kind, Kind::Loop);
    }
    let ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(runs);
    let start = std::time::Instant::now();
    for _ in 0..10 {
        let _ = an.analyze(&x).expect("analyze");
    }
    let dsp_ms = start.elapsed().as_secs_f64() * 1e3 / 10.0;
    println!("classify_kind: {ms:.3} ms per 30 s file (analyzer {dsp_ms:.2} ms), single thread");
}

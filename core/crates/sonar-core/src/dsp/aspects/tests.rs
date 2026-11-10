//! Aspect extraction tests on analytic signals, plus an ignored timing run.

use super::amplitude::{self, ATTACK, CREST, DECAY, EFFECTIVE, ENV, ENV_POINTS, LOUDNESS};
use super::pitch::{CHROMA, CONFIDENCE, F0, F0_SPREAD, PC_COS, PC_SIN, VOICED};
use super::*;
use crate::dsp::tests::{noise, sine, SR};
use crate::dsp::Analyzer;

fn extract(x: &[f32]) -> AspectVectors {
    let f = Analyzer::new(SR)
        .expect("analyzer")
        .analyze(x)
        .expect("analyze");
    AspectExtractor::new(SR)
        .expect("extractor")
        .extract(x, &f)
        .expect("extract")
}

fn assert_shape(v: &AspectVectors) {
    for (a, x) in v.parts() {
        assert_eq!(x.len(), a.dim(), "{}", a.name());
        assert!(x.iter().all(|v| v.is_finite()), "{}: {x:?}", a.name());
    }
}

fn bits(v: &AspectVectors) -> Vec<u32> {
    v.parts()
        .iter()
        .flat_map(|(_, x)| x.iter().map(|f| f.to_bits()))
        .collect()
}

fn dist(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// `amp · e^{−t/τ}`-shaped noise burst (τ = 5 ms), then silence.
fn click(len: usize) -> Vec<f32> {
    let tau = 0.005 * SR as f32;
    noise(4, 1.0, len)
        .iter()
        .enumerate()
        .map(|(i, &n)| 0.9 * n * (-(i as f32) / tau).exp())
        .collect()
}

#[test]
fn dims_are_fixed_and_documented() {
    assert_eq!(
        [
            SPECTRUM_DIM,
            TIMBRE_DIM,
            PITCH_DIM,
            AMPLITUDE_DIM,
            OVERALL_DIM
        ],
        [38, 57, 18, 22, 135]
    );
    let total: usize = Aspect::ALL[..4].iter().map(|a| a.dim()).sum();
    assert_eq!(total, Aspect::Overall.dim());
    for len in [0usize, 1, 441, 30 * SR as usize] {
        for x in [
            vec![0.0; len],
            sine(440.0, 0.5, len),
            noise(2, 0.5, len),
            click(len),
        ] {
            assert_shape(&extract(&x));
        }
    }
}

#[test]
fn stable_across_runs_bitwise() {
    let x: Vec<f32> = sine(261.6, 0.4, 3 * SR as usize)
        .iter()
        .zip(noise(8, 0.1, 3 * SR as usize))
        .map(|(a, b)| a + b)
        .collect();
    let mut an = Analyzer::new(SR).expect("analyzer");
    let mut ex = AspectExtractor::new(SR).expect("extractor");
    let f = an.analyze(&x).expect("analyze");
    let first = ex.extract(&x, &f).expect("extract");
    let other = noise(1, 0.9, 20_000);
    let fo = an.analyze(&other).expect("analyze");
    let _ = ex.extract(&other, &fo).expect("extract");
    let f2 = an.analyze(&x).expect("analyze");
    assert_eq!(bits(&ex.extract(&x, &f2).expect("extract")), bits(&first));
    assert_eq!(bits(&extract(&x)), bits(&first));
}

#[test]
fn silence_nan_and_huge_inputs_are_finite() {
    let silent = extract(&vec![0.0; SR as usize]);
    assert_shape(&silent);
    assert!(silent.spectrum.iter().all(|&v| v == 0.0));
    assert!(silent.timbre.iter().all(|&v| v == 0.0));
    assert_eq!(silent.pitch[VOICED], 0.0);
    assert!(silent.pitch[CHROMA..].iter().all(|&c| c == 1.0 / 12.0));
    assert_eq!(silent.amplitude[LOUDNESS], -100.0);
    assert_eq!(silent.amplitude[ATTACK], -3.0);

    let mut x = sine(220.0, 1.0, 20_000);
    x[10] = f32::NAN;
    x[500] = f32::INFINITY;
    x[900] = f32::NEG_INFINITY;
    x[1500] = f32::MAX;
    for input in [
        x,
        vec![f32::MAX; 5000],
        vec![-1e30; 3000],
        vec![1e-30; 3000],
    ] {
        assert_shape(&extract(&input));
    }
}

#[test]
fn pitch_of_sine_and_noise() {
    let v = extract(&sine(440.0, 0.5, 2 * SR as usize));
    let p = &v.pitch;
    assert!(p[VOICED] > 0.99 && p[CONFIDENCE] > 0.95, "{p:?}");
    assert!(p[F0].abs() < 0.002, "f0 octaves {}", p[F0]);
    assert!(p[F0_SPREAD] < 0.002);
    // Pitch class 9 (A): angle 3π/2.
    assert!(p[PC_COS].abs() < 1e-3 && (p[PC_SIN] + 1.0).abs() < 1e-3);
    let chroma = &p[CHROMA..];
    assert!((chroma.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    assert!(chroma[9] > 0.5, "{chroma:?}");

    let low = extract(&sine(55.0, 0.5, 2 * SR as usize));
    assert!((low.pitch[F0] + 3.0).abs() < 0.002, "{}", low.pitch[F0]);

    let n = extract(&noise(6, 0.5, 2 * SR as usize));
    assert!(n.pitch[VOICED] < 0.1, "{:?}", n.pitch);
    assert!(n.pitch[CHROMA..]
        .iter()
        .all(|&c| (c - 1.0 / 12.0).abs() < 0.03));
}

#[test]
fn loud_and_quiet_sines_are_closer_than_sine_and_noise() {
    let len = 2 * SR as usize;
    let loud = extract(&sine(440.0, 0.5, len));
    let quiet = extract(&sine(440.0, 0.05, len));
    let noisy = extract(&noise(6, 0.5, len));
    // Library stats over a small, varied set.
    let mut lib = LibraryNorm::new();
    for v in [&loud, &quiet, &noisy] {
        lib.push(v).expect("push");
    }
    for f in [110.0, 880.0, 1500.0] {
        lib.push(&extract(&sine(f, 0.3, len))).expect("push");
    }
    lib.push(&extract(&click(len))).expect("push");
    let (l, q, n) = (
        lib.normalize(&loud),
        lib.normalize(&quiet),
        lib.normalize(&noisy),
    );
    // Overall also carries the 20 dB loudness difference (Amplitude).
    for (a, ratio) in [
        (Aspect::Timbre, 0.1),
        (Aspect::Pitch, 0.1),
        (Aspect::Spectrum, 0.1),
        (Aspect::Overall, 0.5),
    ] {
        let same = dist(l.get(a), q.get(a));
        let diff = dist(l.get(a), n.get(a));
        assert!(same < ratio * diff, "{}: {same} vs {diff}", a.name());
    }
    // c0 is dropped: a 20 dB gain change barely moves raw Timbre.
    assert!(dist(&loud.timbre, &quiet.timbre) < 0.05 * dist(&loud.timbre, &noisy.timbre));
    // … while Amplitude sees it.
    assert!((loud.amplitude[LOUDNESS] - quiet.amplitude[LOUDNESS] - 20.0).abs() < 0.1);
}

#[test]
fn amplitude_separates_click_from_sustained_tone() {
    let len = 2 * SR as usize;
    let c = extract(&click(len)).amplitude;
    let t = extract(&sine(440.0, 0.5, len)).amplitude;
    // Sine amplitude 0.5: RMS −9.03 dBFS, crest 3.01 dB.
    assert!((t[LOUDNESS] + 9.03).abs() < 0.1, "{}", t[LOUDNESS]);
    assert!((t[CREST] - 3.01).abs() < 0.2, "{}", t[CREST]);
    // Sustained: decay and effective duration = the whole 2 s.
    assert!((t[DECAY] - 2f32.log10()).abs() < 0.01, "{}", t[DECAY]);
    assert!((t[EFFECTIVE] - 2f32.log10()).abs() < 0.01);
    // Click: −20 dB after 5 ms · ln(10) ≈ 11.5 ms, effective ≈ 4.6 ms,
    // both blurred by the 16 ms smoothing.
    assert!(c[DECAY] < -1.4 && c[DECAY] > -2.2, "{}", c[DECAY]);
    assert!(c[EFFECTIVE] < -1.6, "{}", c[EFFECTIVE]);
    assert!(c[ATTACK] < -2.5 && t[ATTACK] < -2.5);
    assert!(c[CREST] > t[CREST] + 3.0);
    // Envelope: both start at the top; only the tone is still there in the
    // 0.73–1.26 s bin.
    let late = ENV + 10;
    assert!(c[ENV] > 0.9 && t[ENV] > 0.9);
    assert!(c[late] < 0.05 && t[late] > 0.95, "{c:?} {t:?}");
    assert_eq!(t[ENV + ENV_POINTS - 1], 0.0, "past the end is the floor");
    let _ = amplitude::ENV_T_MAX;
}

#[test]
fn features_from_another_rate_are_rejected() {
    let x = sine(440.0, 0.5, 4000);
    let f = Analyzer::new(16_000)
        .expect("analyzer")
        .analyze(&x)
        .expect("analyze");
    let mut ex = AspectExtractor::new(SR).expect("extractor");
    assert_eq!(ex.sample_rate(), SR);
    assert!(matches!(ex.extract(&x, &f), Err(crate::Error::Dsp { .. })));
}

/// Analyzer + aspects on 30 s at 22.05 kHz. Run with
/// `cargo test --release -p sonar-core aspects_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing run; use --release"]
fn aspects_timing_30s() {
    let len = 30 * SR as usize;
    let mut x = noise(7, 0.2, len);
    for (v, s) in x.iter_mut().zip(sine(220.0, 0.5, len)) {
        *v += s;
    }
    let mut an = Analyzer::new(SR).expect("analyzer");
    let mut ex = AspectExtractor::new(SR).expect("extractor");
    let f = an.analyze(&x).expect("analyze");
    let _ = ex.extract(&x, &f).expect("warm-up");
    let runs = 30;
    let start = std::time::Instant::now();
    for _ in 0..runs {
        let v = ex.extract(&x, &f).expect("extract");
        assert_eq!(v.pitch.len(), PITCH_DIM);
    }
    let aspects_ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(runs);
    let start = std::time::Instant::now();
    for _ in 0..runs {
        let f = an.analyze(&x).expect("analyze");
        let _ = ex.extract(&x, &f).expect("extract");
    }
    let total_ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(runs);
    println!(
        "aspects: {aspects_ms:.2} ms, analyzer + aspects: {total_ms:.2} ms per 30 s file (single thread)"
    );
}

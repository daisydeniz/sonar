//! `Analyzer` end-to-end tests, signal generators shared by the `dsp`
//! test modules, and an ignored timing run.

use super::*;
use crate::decode::ANALYSIS_RATE;

pub(super) const SR: u32 = ANALYSIS_RATE;

/// `amp · sin(2π f n / SR)`, `len` samples.
pub(super) fn sine(freq: f64, amp: f32, len: usize) -> Vec<f32> {
    let w = std::f64::consts::TAU * freq / f64::from(SR);
    (0..len)
        .map(|n| amp * (w * n as f64).sin() as f32)
        .collect()
}

/// Deterministic uniform noise in [-amp, amp) (xorshift64*).
pub(super) fn noise(seed: u64, amp: f32, len: usize) -> Vec<f32> {
    let mut s = seed | 1;
    (0..len)
        .map(|_| {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            let u = (s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32;
            amp * (2.0 * u - 1.0)
        })
        .collect()
}

fn assert_valid(f: &Features, len: usize) {
    let n = 1 + len / HOP;
    assert_eq!(f.spectrogram.n_frames, n);
    assert_eq!(f.spectrogram.power.len(), n * (N_FFT / 2 + 1));
    assert_eq!((f.log_mel.n_frames, f.log_mel.n_cols), (n, N_MELS));
    assert_eq!(f.log_mel.data.len(), n * N_MELS);
    assert_eq!((f.mfcc.n_frames, f.mfcc.n_cols), (n, N_MFCC));
    assert_eq!(f.onset.len(), n);
    let all = f
        .spectrogram
        .power
        .iter()
        .chain(&f.log_mel.data)
        .chain(&f.mfcc.data)
        .chain(&f.onset);
    for v in all {
        assert!(v.is_finite(), "non-finite value {v}");
    }
    assert!(f.onset.iter().all(|&o| o >= 0.0));
}

#[test]
fn short_empty_and_silent_inputs_are_finite() {
    let mut a = Analyzer::new(SR).expect("analyzer");
    for len in [
        0usize,
        1,
        100,
        441,
        N_FFT - 1,
        N_FFT,
        N_FFT + 1,
        SR as usize,
    ] {
        let silent = vec![0.0f32; len];
        let f = a.analyze(&silent).expect("analyze");
        assert_valid(&f, len);
        assert!(f.log_mel.data.iter().all(|&v| v == -100.0), "len {len}");
        assert!(f.onset.iter().all(|&o| o == 0.0), "len {len}");
        // Constant log-mel: c0 = −100·√N_MELS, the rest ≈ 0.
        let c0 = -100.0 * (N_MELS as f32).sqrt();
        for row in f.mfcc.rows() {
            assert!((row[0] - c0).abs() < 1e-2, "c0 {}", row[0]);
            assert!(row[1..].iter().all(|c| c.abs() < 1e-2));
        }
        let tone = sine(440.0, 0.5, len);
        assert_valid(&a.analyze(&tone).expect("analyze"), len);
    }
}

#[test]
fn non_finite_and_huge_samples_stay_finite() {
    let mut x = noise(3, 1.0, 4096);
    x[10] = f32::NAN;
    x[500] = f32::INFINITY;
    x[900] = f32::NEG_INFINITY;
    x[1500] = f32::MAX;
    x[2000] = -1e30;
    let mut a = Analyzer::new(SR).expect("analyzer");
    assert_valid(&a.analyze(&x).expect("analyze"), x.len());
    let all_max = vec![f32::MAX; 3000];
    assert_valid(&a.analyze(&all_max).expect("analyze"), all_max.len());
}

#[test]
fn analyzer_is_deterministic_and_reusable() {
    let x = noise(11, 0.3, 20_000);
    let mut a = Analyzer::new(SR).expect("analyzer");
    let first = a.analyze(&x).expect("analyze");
    let _ = a.analyze(&sine(1000.0, 1.0, 5000)).expect("analyze");
    assert_eq!(a.analyze(&x).expect("analyze"), first);
    assert_eq!(
        Analyzer::new(SR).expect("analyzer").analyze(&x).ok(),
        Some(first)
    );
}

#[test]
fn invalid_sample_rate_is_an_error() {
    assert!(matches!(Analyzer::new(0), Err(crate::Error::Dsp { .. })));
    assert!(matches!(Analyzer::new(40), Err(crate::Error::Dsp { .. })));
    assert!(Analyzer::new(8_000).is_ok());
}

#[test]
fn column_stats_mean_and_std() {
    let m = FeatureMatrix {
        n_frames: 4,
        n_cols: 2,
        data: vec![1.0, 5.0, 3.0, 5.0, 1.0, 5.0, 3.0, 5.0],
    };
    let (mean, std) = m.column_stats();
    assert_eq!(mean, vec![2.0, 5.0]);
    assert_eq!(std, vec![1.0, 0.0]);
    let (mean, std) = FeatureMatrix::zeros(0, 3).column_stats();
    assert_eq!((mean, std), (vec![0.0; 3], vec![0.0; 3]));
    assert!(FeatureMatrix::zeros(2, 3).row(5).is_empty());
}

/// Timing of STFT + mel + MFCC + onset on 30 s at 22.05 kHz. Run with
/// `cargo test --release -p sonar-core dsp_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing run; use --release"]
fn dsp_timing_30s() {
    let len = 30 * SR as usize;
    let mut x = noise(7, 0.2, len);
    for (v, s) in x.iter_mut().zip(sine(220.0, 0.5, len)) {
        *v += s;
    }
    let mut a = Analyzer::new(SR).expect("analyzer");
    let _ = a.analyze(&x).expect("warm-up");
    let runs = 50;
    let start = std::time::Instant::now();
    for _ in 0..runs {
        let f = a.analyze(&x).expect("analyze");
        assert_eq!(f.onset.len(), 1 + len / HOP);
    }
    let ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(runs);
    println!("dsp: {ms:.2} ms per 30 s file (single thread)");
}

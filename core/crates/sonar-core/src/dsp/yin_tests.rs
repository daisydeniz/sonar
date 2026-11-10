//! YIN tests on analytic signals.

use super::tests::{noise, sine, SR};
use super::yin::{Yin, YinConfig};

fn tracker() -> Yin {
    Yin::new(YinConfig::aspects(SR)).expect("yin")
}

/// Median f0 of voiced frames and the voiced fraction.
fn median_f0(y: &mut Yin, x: &[f32]) -> (f32, f32) {
    let frames = y.track(x).expect("track");
    let mut f0: Vec<f32> = frames
        .iter()
        .filter(|f| f.voiced)
        .map(|f| f.f0_hz)
        .collect();
    f0.sort_by(f32::total_cmp);
    let voiced = f0.len() as f32 / frames.len() as f32;
    (f0.get(f0.len() / 2).copied().unwrap_or(0.0), voiced)
}

#[test]
fn sine_frequencies_within_one_cent() {
    // Measured: < 0.2 cents for every frequency below.
    let mut y = tracker();
    for freq in [55.0f64, 110.0, 220.0, 440.0, 1000.0, 1760.0] {
        let x = sine(freq, 0.5, SR as usize);
        let (f0, voiced) = median_f0(&mut y, &x);
        let cents = 1200.0 * (f64::from(f0) / freq).log2();
        assert!(cents.abs() < 1.0, "{freq} Hz → {f0} Hz ({cents:.2} cents)");
        assert_eq!(voiced, 1.0, "{freq} Hz voiced fraction {voiced}");
        for fr in y.track(&x).expect("track") {
            assert!(fr.aperiodicity < 0.05, "{freq} Hz d' {}", fr.aperiodicity);
        }
    }
}

#[test]
fn harmonic_tone_reports_the_fundamental() {
    // 110 Hz with 5 harmonics of 1/k amplitude (no octave error).
    let len = SR as usize;
    let mut x = vec![0.0f32; len];
    for k in 1..=5 {
        for (v, s) in x
            .iter_mut()
            .zip(sine(110.0 * k as f64, 0.3 / k as f32, len))
        {
            *v += s;
        }
    }
    let (f0, voiced) = median_f0(&mut tracker(), &x);
    assert!((f0 / 110.0 - 1.0).abs() < 0.005, "{f0}");
    assert_eq!(voiced, 1.0);
}

#[test]
fn white_noise_and_silence_are_unvoiced() {
    let mut y = tracker();
    let frames = y.track(&noise(5, 0.5, SR as usize)).expect("track");
    assert!(frames.iter().all(|f| !f.voiced && f.energy > 0.0));
    assert!(frames.iter().all(|f| f.aperiodicity > 0.3));
    for len in [0usize, 1, 441, SR as usize] {
        let frames = y.track(&vec![0.0; len]).expect("track");
        assert_eq!(frames.len(), y.n_frames(len));
        assert!(frames
            .iter()
            .all(|f| !f.voiced && f.f0_hz == 0.0 && f.energy == 0.0));
    }
    // DC has no period either.
    let frames = y.track(&vec![0.5; 4000]).expect("track");
    assert!(frames.iter().all(|f| !f.voiced));
}

#[test]
fn frame_count_and_short_inputs() {
    let mut y = tracker();
    let fl = y.frame_len();
    assert_eq!(y.lag_range(), (11, 735));
    assert_eq!(fl, 1024 + 735 + 1);
    assert_eq!(y.n_frames(0), 1);
    assert_eq!(y.n_frames(fl), 1);
    assert_eq!(y.n_frames(fl + 1023), 1);
    assert_eq!(y.n_frames(fl + 1024), 2);
    // A 20 ms 440 Hz burst (zero-padded frame) is still found.
    let frames = y.track(&sine(440.0, 0.5, 441)).expect("track");
    assert_eq!(frames.len(), 1);
    assert!(
        (frames[0].f0_hz / 440.0 - 1.0).abs() < 0.01,
        "{:?}",
        frames[0]
    );
}

#[test]
fn non_finite_and_huge_inputs_stay_finite() {
    let mut x = sine(220.0, 1.0, 8000);
    x[10] = f32::NAN;
    x[500] = f32::INFINITY;
    x[900] = f32::NEG_INFINITY;
    x[3000] = f32::MAX;
    let mut y = tracker();
    for input in [x, vec![f32::MAX; 5000], vec![-1e30; 3000]] {
        for f in y.track(&input).expect("track") {
            assert!(f.f0_hz.is_finite() && f.energy.is_finite(), "{f:?}");
            assert!((0.0..=1.0).contains(&f.aperiodicity));
        }
    }
}

#[test]
fn deterministic_with_fresh_and_reused_tracker() {
    let x: Vec<f32> = sine(330.0, 0.4, 30_000)
        .iter()
        .zip(noise(9, 0.05, 30_000))
        .map(|(a, b)| a + b)
        .collect();
    let mut y = tracker();
    let first = y.track(&x).expect("track");
    let _ = y.track(&noise(1, 1.0, 9000)).expect("track");
    assert_eq!(y.track(&x).expect("track"), first);
    assert_eq!(tracker().track(&x).expect("track"), first);
}

#[test]
fn invalid_configs_are_errors() {
    let base = YinConfig::aspects(SR);
    for cfg in [
        YinConfig { fmin: 0.0, ..base },
        YinConfig { fmax: 10.0, ..base },
        YinConfig {
            fmax: f32::INFINITY,
            ..base
        },
        YinConfig {
            fmax: 20_000.0,
            ..base
        },
        YinConfig { window: 0, ..base },
        YinConfig { hop: 0, ..base },
        YinConfig {
            sample_rate: 0,
            ..base
        },
    ] {
        assert!(
            matches!(Yin::new(cfg), Err(crate::Error::Dsp { .. })),
            "{cfg:?}"
        );
    }
}

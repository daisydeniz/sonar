//! Onset strength tests on click trains, steps, silence and stationary tones.

use super::tests::{noise, sine, SR};
use super::{onset_strength, Analyzer, FeatureMatrix, HOP};

fn envelope(x: &[f32]) -> Vec<f32> {
    Analyzer::new(SR)
        .expect("analyzer")
        .analyze(x)
        .expect("analyze")
        .onset
}

#[test]
fn click_train_peaks_at_click_frames() {
    let period = 20; // frames (≈ 129 BPM)
    let first = 10;
    let n_frames = 300;
    let mut x = vec![0.0f32; n_frames * HOP];
    let clicks: Vec<usize> = (first..n_frames - 5).step_by(period).collect();
    for &c in &clicks {
        x[c * HOP] = 1.0;
    }
    let env = envelope(&x);
    let peak = env.iter().copied().fold(0.0f32, f32::max);
    assert!(peak > 1.0, "peak {peak}");
    for &c in &clicks {
        let lo = c - 3;
        let best = (lo..=c + 3).fold(lo, |a, t| if env[t] > env[a] { t } else { a });
        assert!(best.abs_diff(c) <= 1, "click {c}: peak at {best}");
        assert!(env[best] > 0.5 * peak, "click {c}: weak peak {}", env[best]);
        // Between clicks the envelope is flat zero (signal decays → negative flux).
        let quiet = env
            .get(c + 3..(c + period - 3).min(env.len()))
            .unwrap_or(&[]);
        assert!(quiet.iter().all(|&o| o < 0.01 * peak), "after click {c}");
    }
}

#[test]
fn noise_burst_onset_is_located() {
    // Silence, then noise from the centre of frame 50.
    let start = 50 * HOP;
    let mut x = vec![0.0f32; 100 * HOP];
    for (v, n) in x[start..].iter_mut().zip(noise(5, 0.5, 50 * HOP)) {
        *v = n;
    }
    let env = envelope(&x);
    let best = (0..env.len()).fold(0, |a, t| if env[t] > env[a] { t } else { a });
    assert!(best.abs_diff(50) <= 1, "onset at {best}");
    assert!(env[..47].iter().all(|&o| o == 0.0));
}

#[test]
fn silence_gives_zero_envelope() {
    for len in [0usize, 10, 3 * HOP, SR as usize] {
        let env = envelope(&vec![0.0; len]);
        assert_eq!(env.len(), 1 + len / HOP);
        assert!(env.iter().all(|&o| o == 0.0), "len {len}");
    }
}

#[test]
fn stationary_sine_has_near_zero_flux() {
    let x = sine(440.0, 0.5, 3 * SR as usize);
    let env = envelope(&x);
    let n = env.len();
    let steady = &env[5..n - 5];
    let max = steady.iter().copied().fold(0.0f32, f32::max);
    assert!(max < 0.01, "steady-state flux {max} dB");
    // The first frames see the tone fade in (partial windows at the start;
    // only the few bands around 440 Hz rise, so the band mean is small).
    let ramp = env[1..4].iter().copied().fold(0.0f32, f32::max);
    assert!(
        ramp > 0.05 && ramp > 50.0 * max,
        "ramp {ramp}, steady {max}"
    );
}

#[test]
fn degenerate_matrices() {
    assert!(onset_strength(&FeatureMatrix::zeros(0, 64)).is_empty());
    assert_eq!(onset_strength(&FeatureMatrix::zeros(1, 64)), vec![0.0]);
    assert_eq!(onset_strength(&FeatureMatrix::zeros(3, 0)), vec![0.0; 3]);
    // Rising level: flux = mean rise per band (+10 dB, then +20 dB).
    let m = FeatureMatrix {
        n_frames: 3,
        n_cols: 2,
        data: vec![-50.0, -50.0, -40.0, -40.0, -20.0, -20.0],
    };
    assert_eq!(onset_strength(&m), vec![0.0, 10.0, 20.0]);
}

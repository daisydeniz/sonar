//! STFT tests against analytic signals.

use super::stft::{hann_periodic, Stft};
use super::tests::{noise, sine, SR};
use super::{HOP, N_FFT};

fn argmax(v: &[f32]) -> usize {
    v.iter()
        .enumerate()
        .fold((0, f32::NEG_INFINITY), |(bi, bv), (i, &x)| {
            if x > bv {
                (i, x)
            } else {
                (bi, bv)
            }
        })
        .0
}

#[test]
fn periodic_hann_shape_and_sum() {
    let w = hann_periodic(N_FFT);
    assert_eq!(w.len(), N_FFT);
    assert_eq!(w[0], 0.0);
    assert!((w[N_FFT / 2] - 1.0).abs() < 1e-7);
    // Periodic: symmetric around N/2, w[n] = w[N − n].
    for n in 1..N_FFT {
        assert!((w[n] - w[N_FFT - n]).abs() < 1e-6);
    }
    let sum: f64 = w.iter().map(|&x| f64::from(x)).sum();
    assert!((sum - N_FFT as f64 / 2.0).abs() < 1e-3, "Σw = {sum}");
}

#[test]
fn bin_centred_sine_peak_bin_and_magnitude() {
    let mut stft = Stft::new(N_FFT, HOP).expect("stft");
    for (k0, amp) in [(93usize, 1.0f32), (10, 0.25), (500, 0.5), (1000, 0.8)] {
        let f = k0 as f64 * f64::from(SR) / N_FFT as f64;
        let spec = stft.power(&sine(f, amp, SR as usize), SR).expect("stft");
        // Frames fully inside the signal.
        for t in 4..spec.n_frames - 4 {
            let frame = spec.frame(t).expect("frame");
            assert_eq!(argmax(frame), k0, "f = {f} Hz, frame {t}");
            // |X[k0]| = A · Σw / 2 = A · N / 4.
            let mag = frame[k0].sqrt();
            let expect = amp * N_FFT as f32 / 4.0;
            assert!(
                (mag - expect).abs() / expect < 1e-3,
                "k0 {k0}: |X| = {mag}, expected {expect}"
            );
        }
    }
}

#[test]
fn off_bin_sine_peaks_at_nearest_bin() {
    let mut stft = Stft::new(N_FFT, HOP).expect("stft");
    for f in [55.0, 440.0, 1234.5, 5000.0, 9876.0] {
        let spec = stft.power(&sine(f, 0.7, SR as usize), SR).expect("stft");
        let expect = (f * N_FFT as f64 / f64::from(SR)).round() as usize;
        let frame = spec.frame(spec.n_frames / 2).expect("frame");
        assert_eq!(argmax(frame), expect, "f = {f}");
        assert!((spec.bin_hz(expect) as f64 - f).abs() <= f64::from(SR) / N_FFT as f64 / 2.0);
    }
}

#[test]
fn parseval_holds_per_frame() {
    let x = noise(42, 0.5, 4 * N_FFT);
    let mut stft = Stft::new(N_FFT, HOP).expect("stft");
    let spec = stft.power(&x, SR).expect("stft");
    let w = hann_periodic(N_FFT);
    let n_bins = spec.n_bins();
    for t in [4usize, 7, 10] {
        let start = t * HOP - N_FFT / 2;
        let time: f64 = (0..N_FFT)
            .map(|i| f64::from(w[i] * x[start + i]).powi(2))
            .sum();
        let p = spec.frame(t).expect("frame");
        let inner: f64 = p[1..n_bins - 1].iter().map(|&v| f64::from(v)).sum();
        let freq = (f64::from(p[0]) + 2.0 * inner + f64::from(p[n_bins - 1])) / N_FFT as f64;
        assert!(
            (time - freq).abs() / time < 1e-4,
            "frame {t}: {time} vs {freq}"
        );
    }
}

#[test]
fn frames_are_centred_on_hop_multiples() {
    // Impulse at the centre of frame 5: w[N/2] = 1, so |X[k]|² = 1 for all k;
    // frames 4 and 6 see it at w[N/2 ± HOP] = 0.5 → power 0.25; frame 3 and
    // 7 see it at w[0] = 0 (frame 7) or not at all (frame 3).
    let mut x = vec![0.0f32; 20 * HOP];
    x[5 * HOP] = 1.0;
    let mut stft = Stft::new(N_FFT, HOP).expect("stft");
    let spec = stft.power(&x, SR).expect("stft");
    let level = |t: usize, want: f32| {
        let frame = spec.frame(t).expect("frame");
        assert!(frame.iter().all(|&p| (p - want).abs() < 1e-5), "frame {t}");
    };
    level(5, 1.0);
    level(4, 0.25);
    level(6, 0.25);
    level(3, 0.0);
    level(7, 0.0);
    assert!((spec.frame_time(5) - 5.0 * HOP as f32 / SR as f32).abs() < 1e-7);
    assert!((spec.frame_rate() - SR as f32 / HOP as f32).abs() < 1e-3);
}

#[test]
fn frame_count_and_short_inputs() {
    let mut stft = Stft::new(N_FFT, HOP).expect("stft");
    for len in [0usize, 1, 511, 512, 513, 2047, 2048, 10_000] {
        let spec = stft.power(&vec![0.1; len], SR).expect("stft");
        assert_eq!(spec.n_frames, 1 + len / HOP, "len {len}");
        assert_eq!(spec.frames().count(), spec.n_frames);
        assert!(spec.power.iter().all(|p| p.is_finite()));
        assert!(spec.frame(spec.n_frames).is_none());
    }
    let empty = stft.power(&[], SR).expect("stft");
    assert!(empty.power.iter().all(|&p| p == 0.0));
    // One sample of DC at frame 0's centre: flat spectrum of w[N/2]² = 1.
    let one = stft.power(&[1.0], SR).expect("stft");
    assert!(one.power.iter().all(|&p| (p - 1.0).abs() < 1e-6));
}

#[test]
fn invalid_parameters_are_errors() {
    assert!(Stft::new(0, 1).is_err());
    assert!(Stft::new(1023, 256).is_err());
    assert!(Stft::new(1024, 0).is_err());
    assert!(Stft::new(1024, 2048).is_err());
    assert!(Stft::new(1024, 1024).is_ok());
}

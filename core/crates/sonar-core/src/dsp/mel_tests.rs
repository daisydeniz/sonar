//! Mel scale and filterbank tests.

use super::mel::{hz_to_mel, mel_to_hz, power_to_db, MelFilterbank, MelNorm, POWER_FLOOR};
use super::stft::Stft;
use super::tests::{sine, SR};
use super::{HOP, MEL_FMIN, N_FFT, N_MELS};

fn bank(norm: MelNorm) -> MelFilterbank {
    MelFilterbank::new(SR, N_FFT, N_MELS, MEL_FMIN, f64::from(SR) / 2.0, norm).expect("bank")
}

#[test]
fn slaney_mel_scale_known_points_and_inverse() {
    assert_eq!(hz_to_mel(0.0), 0.0);
    assert!((hz_to_mel(500.0) - 7.5).abs() < 1e-12);
    assert!((hz_to_mel(1000.0) - 15.0).abs() < 1e-12);
    // 6.4 kHz is exactly 27 log steps above 1 kHz.
    assert!((hz_to_mel(6400.0) - 42.0).abs() < 1e-9);
    for f in [0.0, 20.0, 440.0, 999.9, 1000.0, 1000.1, 4000.0, 11_025.0] {
        assert!((mel_to_hz(hz_to_mel(f)) - f).abs() < 1e-6, "{f}");
    }
    // Monotonic.
    let m: Vec<f64> = (0..200).map(|i| hz_to_mel(i as f64 * 60.0)).collect();
    assert!(m.windows(2).all(|p| p[1] > p[0]));
}

#[test]
fn filters_are_nonempty_triangles() {
    let fb = bank(MelNorm::Peak);
    assert_eq!((fb.n_mels(), fb.n_bins()), (N_MELS, N_FFT / 2 + 1));
    for b in 0..N_MELS {
        let w = fb.weights(b).expect("band");
        assert!(w.iter().all(|&x| (0.0..=1.0).contains(&x)), "band {b}");
        let nz: Vec<usize> = (0..w.len()).filter(|&k| w[k] > 0.0).collect();
        assert!(nz.len() >= 2, "band {b} has {} bins", nz.len());
        // Contiguous support, rises to one peak then falls.
        let (first, last) = (nz[0], nz[nz.len() - 1]);
        assert_eq!(last - first + 1, nz.len(), "band {b} gaps");
        let peak = (first..=last).fold(first, |a, k| if w[k] > w[a] { k } else { a });
        assert!(w[first..=peak].windows(2).all(|p| p[1] >= p[0]), "band {b}");
        assert!(w[peak..=last].windows(2).all(|p| p[1] <= p[0]), "band {b}");
        // Centres increase with band index.
        if b > 0 {
            assert!(fb.center_hz(b) > fb.center_hz(b - 1));
        }
    }
    assert!(fb.weights(N_MELS).is_none());
}

#[test]
fn peak_filters_partition_unity_between_outer_centres() {
    let fb = bank(MelNorm::Peak);
    let lo = fb.center_hz(0).expect("c0");
    let hi = fb.center_hz(N_MELS - 1).expect("cN");
    let dense: Vec<Vec<f32>> = (0..N_MELS).map(|b| fb.weights(b).expect("band")).collect();
    let bin_hz = f64::from(SR) / N_FFT as f64;
    let mut checked = 0;
    for k in 0..fb.n_bins() {
        let f = k as f64 * bin_hz;
        let total: f32 = dense.iter().map(|w| w[k]).sum();
        if f >= lo && f <= hi {
            assert!((total - 1.0).abs() < 1e-5, "bin {k} ({f} Hz): Σw = {total}");
            checked += 1;
        } else {
            assert!(total <= 1.0 + 1e-6);
        }
    }
    assert!(checked > 900, "only {checked} bins inside");
}

#[test]
fn slaney_filters_have_unit_area_in_hz() {
    let fb = bank(MelNorm::Slaney);
    let bin_hz = f64::from(SR) / N_FFT as f64;
    for b in 0..N_MELS {
        let area: f64 = fb
            .weights(b)
            .expect("band")
            .iter()
            .map(|&w| f64::from(w) * bin_hz)
            .sum();
        assert!((area - 1.0).abs() < 0.1, "band {b}: area {area}");
    }
}

#[test]
fn sine_lands_in_its_band() {
    let fb = bank(MelNorm::Slaney);
    let mut stft = Stft::new(N_FFT, HOP).expect("stft");
    let mut mel = vec![0.0f32; N_MELS];
    for b in [2usize, 10, 25, 40, 55, 62] {
        let f = fb.center_hz(b).expect("centre");
        let spec = stft.power(&sine(f, 0.5, 8 * N_FFT), SR).expect("stft");
        fb.apply(spec.frame(spec.n_frames / 2).expect("frame"), &mut mel);
        let best = (0..N_MELS).fold(0, |a, i| if mel[i] > mel[a] { i } else { a });
        assert_eq!(best, b, "sine at {f:.1} Hz");
    }
}

#[test]
fn log_mel_floor_and_db_scale() {
    assert_eq!(power_to_db(0.0), -100.0);
    assert_eq!(power_to_db(f32::NAN), -100.0);
    assert_eq!(power_to_db(POWER_FLOOR / 10.0), -100.0);
    assert!((power_to_db(1.0)).abs() < 1e-6);
    assert!((power_to_db(100.0) - 20.0).abs() < 1e-5);
    let fb = bank(MelNorm::Slaney);
    let mut stft = Stft::new(N_FFT, HOP).expect("stft");
    let silent = fb.log_mel(&stft.power(&[0.0; 3000], SR).expect("stft"));
    assert_eq!((silent.n_frames, silent.n_cols), (1 + 3000 / HOP, N_MELS));
    assert!(silent.data.iter().all(|&v| v == -100.0));
    // +6.02 dB per doubling of amplitude.
    let quiet = fb.log_mel(&stft.power(&sine(1000.0, 0.25, 8192), SR).expect("stft"));
    let loud = fb.log_mel(&stft.power(&sine(1000.0, 0.5, 8192), SR).expect("stft"));
    let b = (0..N_MELS)
        .max_by(|&i, &j| loud.row(8)[i].total_cmp(&loud.row(8)[j]))
        .expect("band");
    assert!((loud.row(8)[b] - quiet.row(8)[b] - 6.0206).abs() < 1e-3);
}

#[test]
fn invalid_ranges_are_errors() {
    let nyq = f64::from(SR) / 2.0;
    assert!(MelFilterbank::new(SR, N_FFT, 0, 0.0, nyq, MelNorm::Slaney).is_err());
    assert!(MelFilterbank::new(SR, N_FFT, 64, 100.0, 50.0, MelNorm::Slaney).is_err());
    assert!(MelFilterbank::new(SR, N_FFT, 64, 0.0, nyq + 1.0, MelNorm::Slaney).is_err());
    assert!(MelFilterbank::new(SR, N_FFT, 64, f64::NAN, nyq, MelNorm::Slaney).is_err());
    assert!(MelFilterbank::new(SR, 1, 64, 0.0, nyq, MelNorm::Slaney).is_err());
}

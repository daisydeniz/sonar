//! DCT / MFCC / delta tests against closed-form results.

use super::mfcc::{deltas, Dct, DELTA_WIDTH};
use super::{FeatureMatrix, N_MELS, N_MFCC};

/// Direct orthonormal DCT-II in f64.
fn dct_direct(x: &[f64], k: usize) -> f64 {
    let n = x.len() as f64;
    let s = if k == 0 {
        (1.0 / n).sqrt()
    } else {
        (2.0 / n).sqrt()
    };
    s * x
        .iter()
        .enumerate()
        .map(|(i, v)| v * (std::f64::consts::PI * k as f64 * (2 * i + 1) as f64 / (2.0 * n)).cos())
        .sum::<f64>()
}

#[test]
fn constant_input_has_only_c0() {
    let dct = Dct::new(N_MELS, N_MFCC).expect("dct");
    let mut out = vec![0.0f32; N_MFCC];
    for c in [-100.0f32, -3.5, 0.0, 42.0] {
        dct.apply(&[c; N_MELS], &mut out);
        assert!(
            (out[0] - c * (N_MELS as f32).sqrt()).abs() < 1e-3,
            "c0 {}",
            out[0]
        );
        assert!(out[1..].iter().all(|v| v.abs() < 1e-3), "{out:?}");
    }
}

#[test]
fn full_dct_is_orthonormal() {
    let n = N_MELS;
    let dct = Dct::new(n, n).expect("dct");
    for i in 0..n {
        for j in 0..n {
            let dot: f64 = dct
                .row(i)
                .expect("row")
                .iter()
                .zip(dct.row(j).expect("row"))
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum();
            let want = if i == j { 1.0 } else { 0.0 };
            assert!((dot - want).abs() < 1e-5, "<{i},{j}> = {dot}");
        }
    }
    assert!(dct.row(n).is_none());
}

#[test]
fn matches_direct_formula() {
    let x: Vec<f64> = (0..N_MELS)
        .map(|i| -60.0 + 30.0 * ((i as f64) * 0.37).sin() + (i as f64) * 0.5)
        .collect();
    let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
    let dct = Dct::new(N_MELS, N_MFCC).expect("dct");
    let mut out = vec![0.0f32; N_MFCC];
    dct.apply(&xf, &mut out);
    for (k, &got) in out.iter().enumerate() {
        let want = dct_direct(&x, k);
        assert!(
            (f64::from(got) - want).abs() < 1e-3,
            "c{k}: {got} vs {want}"
        );
    }
}

#[test]
fn cosine_basis_input_selects_one_coefficient() {
    let dct = Dct::new(N_MELS, N_MFCC).expect("dct");
    let basis = dct.row(5).expect("row").to_vec();
    let mut out = vec![0.0f32; N_MFCC];
    dct.apply(&basis, &mut out);
    for (k, v) in out.iter().enumerate() {
        let want = if k == 5 { 1.0 } else { 0.0 };
        assert!((v - want).abs() < 1e-5, "c{k} = {v}");
    }
}

#[test]
fn mfcc_matrix_maps_each_frame() {
    let dct = Dct::new(4, 2).expect("dct");
    let m = FeatureMatrix {
        n_frames: 2,
        n_cols: 4,
        data: vec![1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0],
    };
    let c = dct.mfcc(&m);
    assert_eq!((c.n_frames, c.n_cols), (2, 2));
    assert!((c.row(0)[0] - 2.0).abs() < 1e-6 && c.row(0)[1].abs() < 1e-6);
    assert!((c.row(1)[0] - 4.0).abs() < 1e-6);
    assert_eq!((dct.n_in(), dct.n_out()), (4, 2));
}

#[test]
fn deltas_of_ramp_constant_and_edges() {
    // Column 0: ramp 3t (delta 3 away from the edges); column 1: constant.
    let n = 12;
    let m = FeatureMatrix {
        n_frames: n,
        n_cols: 2,
        data: (0..n).flat_map(|t| [3.0 * t as f32, 7.0]).collect(),
    };
    let d = deltas(&m, DELTA_WIDTH);
    assert_eq!((d.n_frames, d.n_cols), (n, 2));
    for t in DELTA_WIDTH..n - DELTA_WIDTH {
        assert!((d.row(t)[0] - 3.0).abs() < 1e-5, "t {t}: {}", d.row(t)[0]);
    }
    // Edge replication: t = 0 sees c(−1) = c(−2) = c(0): (1·3 + 2·6) / 10.
    assert!((d.row(0)[0] - 1.5).abs() < 1e-5);
    assert!(d.rows().all(|r| r[1] == 0.0));
    assert!(deltas(&m, 0).data.iter().all(|&v| v == 0.0));
    let one = FeatureMatrix::zeros(1, 3);
    assert_eq!(deltas(&one, DELTA_WIDTH), one);
    assert_eq!(deltas(&FeatureMatrix::zeros(0, 3), 2).n_frames, 0);
}

#[test]
fn invalid_sizes_are_errors() {
    assert!(Dct::new(0, 0).is_err());
    assert!(Dct::new(10, 0).is_err());
    assert!(Dct::new(10, 11).is_err());
}

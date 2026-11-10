//! `NormStats` / `LibraryNorm` tests.

use rayon::prelude::*;

use super::norm::{STD_FLOOR, Z_CLAMP};
use super::*;
use crate::dsp::tests::noise;

/// Deterministic pseudo-random vectors with per-dimension offset/scale.
fn vectors(n: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
    let raw = noise(seed, 1.0, n * dim);
    raw.chunks_exact(dim)
        .map(|r| {
            r.iter()
                .enumerate()
                .map(|(i, &u)| 10.0 * i as f32 - 3.0 + (1.0 + i as f32) * u)
                .collect()
        })
        .collect()
}

fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x - y).abs() <= tol * (1.0 + x.abs().max(y.abs())))
}

#[test]
fn welford_matches_two_pass_and_merge_matches_sequential() {
    let dim = 7;
    let data = vectors(1000, dim, 3);
    let mut seq = NormStats::new(dim);
    for v in &data {
        seq.push(v).expect("push");
    }
    assert_eq!(seq.count(), 1000);
    for d in 0..dim {
        let col: Vec<f64> = data.iter().map(|v| f64::from(v[d])).collect();
        let mean = col.iter().sum::<f64>() / 1000.0;
        let var = col.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / 1000.0;
        assert!((seq.mean()[d] - mean).abs() < 1e-9 * (1.0 + mean.abs()));
        assert!((seq.std()[d] - var.sqrt()).abs() < 1e-9 * (1.0 + var.sqrt()));
    }
    // Uneven split, plus an empty part.
    let mut a = NormStats::new(dim);
    let mut b = NormStats::new(dim);
    for v in &data[..137] {
        a.push(v).expect("push");
    }
    for v in &data[137..] {
        b.push(v).expect("push");
    }
    a.merge(&b).expect("merge");
    a.merge(&NormStats::new(dim)).expect("merge empty");
    let mut empty = NormStats::new(dim);
    empty.merge(&seq).expect("merge into empty");
    for s in [&a, &empty] {
        assert_eq!(s.count(), seq.count());
        assert!(close(s.mean(), seq.mean(), 1e-12));
        assert!(close(&s.std(), &seq.std(), 1e-12));
    }
    // rayon fold + reduce.
    let par = data
        .par_iter()
        .fold(
            || NormStats::new(dim),
            |mut s, v| {
                s.push(v).expect("push");
                s
            },
        )
        .reduce(
            || NormStats::new(dim),
            |mut x, y| {
                x.merge(&y).expect("merge");
                x
            },
        );
    assert!(close(par.mean(), seq.mean(), 1e-12) && close(&par.std(), &seq.std(), 1e-12));
}

#[test]
fn z_scores_have_zero_mean_unit_std() {
    let dim = 5;
    let data = vectors(2000, dim, 9);
    let mut s = NormStats::new(dim);
    for v in &data {
        s.push(v).expect("push");
    }
    let mut z = NormStats::new(dim);
    for v in &data {
        z.push(&s.normalize(v)).expect("push");
    }
    for (m, sd) in z.mean().iter().zip(z.std()) {
        assert!(m.abs() < 1e-4, "mean {m}");
        assert!((sd - 1.0).abs() < 1e-4, "std {sd}");
    }
}

#[test]
fn constant_dims_unseen_dims_and_outliers_stay_finite() {
    let mut s = NormStats::new(3);
    for _ in 0..10 {
        s.push(&[2.0, 5.0, f32::NAN]).expect("push");
    }
    assert_eq!(s.std(), vec![0.0; 3]);
    assert_eq!(s.normalize(&[2.0, 5.0, 0.0]), vec![0.0; 3]);
    let z = s.normalize(&[2.0 + STD_FLOOR as f32, 1e30, f32::NAN]);
    assert!((z[0] - 1.0).abs() < 1e-3, "{z:?}");
    assert_eq!(z[1], Z_CLAMP);
    assert_eq!(z[2], 0.0);
    // Empty stats and extra dimensions: identity, clamped.
    assert_eq!(
        NormStats::new(1).normalize(&[0.5, -100.0]),
        vec![0.5, -Z_CLAMP]
    );
    assert!(s.push(&[1.0, 2.0]).is_err());
    assert_eq!(s.count(), 10);
}

#[test]
fn library_norm_roundtrips_and_validates() {
    let mut lib = LibraryNorm::new();
    assert_eq!(lib.count(), 0);
    let make = |seed: u64| AspectVectors {
        spectrum: noise(seed, 1.0, SPECTRUM_DIM),
        timbre: noise(seed + 1, 5.0, TIMBRE_DIM),
        pitch: noise(seed + 2, 0.5, PITCH_DIM),
        amplitude: noise(seed + 3, 2.0, AMPLITUDE_DIM),
    };
    for seed in 0..20 {
        lib.push(&make(seed * 10)).expect("push");
    }
    let json = serde_json::to_string(&lib).expect("serialize");
    let back: LibraryNorm = serde_json::from_str(&json).expect("deserialize");
    // serde_json's default float parser may be off by one ulp.
    assert_eq!((back.version, back.count()), (lib.version, lib.count()));
    for (b, l) in [
        (&back.spectrum, &lib.spectrum),
        (&back.timbre, &lib.timbre),
        (&back.pitch, &lib.pitch),
        (&back.amplitude, &lib.amplitude),
    ] {
        assert!(close(b.mean(), l.mean(), 1e-14) && close(&b.std(), &l.std(), 1e-14));
    }

    let n = lib.normalize(&make(3));
    for a in Aspect::ALL {
        assert_eq!(n.get(a).len(), a.dim(), "{}", a.name());
        assert!(n.get(a).iter().all(|v| v.is_finite()));
    }
    // Overall = z / √dim per aspect (equal weights).
    let s = 1.0 / (SPECTRUM_DIM as f32).sqrt();
    assert!((n.overall[0] - n.spectrum[0] * s).abs() < 1e-6);
    let t = 1.0 / (TIMBRE_DIM as f32).sqrt();
    assert!((n.overall[SPECTRUM_DIM] - n.timbre[0] * t).abs() < 1e-6);

    // A wrong dimension changes nothing.
    let mut bad = make(1);
    bad.pitch.pop();
    assert!(lib.push(&bad).is_err());
    assert_eq!(lib.count(), 20);
    assert_eq!(lib.spectrum.count(), lib.pitch.count());

    let mut other = LibraryNorm::new();
    other.push(&make(500)).expect("push");
    let mut merged = lib.clone();
    merged.merge(&other).expect("merge");
    assert_eq!(merged.count(), 21);
    other.version += 1;
    assert!(merged.merge(&other).is_err());
    assert_eq!(merged.count(), 21);

    // Damaged stats (lengths, negative variance) still give finite output.
    let damaged = r#"{"version":1,"spectrum":{"count":3,"mean":[1.0],"m2":[-5.0, 2.0]},
        "timbre":{"count":0,"mean":[],"m2":[]},"pitch":{"count":2,"mean":[1e308],"m2":[1e308]},
        "amplitude":{"count":1,"mean":[0.0],"m2":[0.0]}}"#;
    let d: LibraryNorm = serde_json::from_str(damaged).expect("deserialize");
    let n = d.normalize(&make(7));
    assert_eq!(n.overall.len(), OVERALL_DIM);
    assert!(n.overall.iter().all(|v| v.is_finite()));
}

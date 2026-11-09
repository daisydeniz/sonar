//! Basic meta (T1.4) against the whole generated fixture set (`sonar
//! fixtures`, incl. the 310 s file and every container): exact format and
//! length, sample peak within ±0.01 dB of `truth.peak_dbfs` for lossless
//! files, `None` for digital silence, loudness of pure sines against the
//! BS.1770 prediction from `truth.rms_dbfs`, and the corrupt-file
//! `expect` contract. Tolerances are documented in `docs/EVAL.md`.
//!
//! Run in release with `--nocapture` to see meta on/off throughput.

use std::time::Instant;

use sonar_core::decode::{DecodeOptions, Decoder};
use sonar_core::fixtures::aac::{encode as aac_encode, AacSpec, Tags};
use sonar_core::fixtures::manifest::{Expect, Fixture};
use sonar_core::fixtures::truth::Category;
use sonar_core::fixtures::wav::{encode, SampleFormat, WavSpec};
use sonar_core::fixtures::{generate, FixtureOptions};
use sonar_core::meta::analyze_file;
use sonar_core::meta::kweight::k_weighting_gain_db;
use sonar_core::meta::meter::LOUDNESS_OFFSET;
use sonar_core::meta::{BasicMeta, MetaOptions};
use sonar_core::Error;

/// Peak tolerance for lossless files (dB): the decoder returns the stored
/// samples exactly, so only `f32` rounding remains.
const PEAK_TOL_LOSSLESS: f64 = 0.01;
/// Upper bound on one packet's frames for the fixture codecs (PCM blocks,
/// FLAC/ALAC frames ≤ 4096): the counted-vs-metered length difference on a
/// truncated file (ADR-012) is less than one packet.
const MAX_PACKET_FRAMES: u64 = 4_096;
/// Loudness tolerance (LU) for sustained pure sines ≥ 0.4 s: the BS.1770
/// prediction is exact for a stationary sine; fades and block edges cost
/// < 0.05 LU.
const SINE_TOL: f64 = 0.1;
/// Loudness tolerance (LU) for sine bursts shorter than one 400 ms block
/// (measured as one block; K-filter onset transient).
const SHORT_SINE_TOL: f64 = 0.2;

/// BS.1770 loudness of a pure sine of `rms_dbfs` per channel at `hz` on
/// `channels` channels of weight 1.
fn predicted_sine_lufs(f: &Fixture) -> Option<f64> {
    let fmt = f.format.as_ref()?;
    let t = &f.truth;
    if t.category != Some(Category::Sine) {
        return None;
    }
    let (hz, rms, peak) = (t.pitch_hz?, t.rms_dbfs?, t.peak_dbfs?);
    // A pure sine has a 3.01 dB crest factor (a little more with the
    // fixtures' 10 ms fades); the hard-clipped sine has far less (its
    // harmonics are weighted differently), so it is not predicted.
    if peak - rms < 2.95 {
        return None;
    }
    let k = k_weighting_gain_db(hz, f64::from(fmt.sample_rate));
    Some(rms + 10.0 * f64::from(fmt.channels).log10() + k + LOUDNESS_OFFSET)
}

fn check_audio(f: &Fixture, m: &BasicMeta) -> Option<f64> {
    let id = &f.path;
    let fmt = f.format.as_ref().unwrap();
    assert_eq!(m.sample_rate, fmt.sample_rate, "{id}");
    assert_eq!(m.channels, fmt.channels, "{id}");
    assert_eq!(m.bit_depth, fmt.bit_depth.map(u32::from), "{id}");
    assert_eq!(m.num_frames, fmt.num_frames, "{id}");
    assert!(
        (m.duration_ms as f64 - fmt.duration_s * 1e3).abs() <= 0.5,
        "{id}: {} ms vs {} s",
        m.duration_ms,
        fmt.duration_s
    );
    assert!(!m.partial, "{id}");
    // Every fixture is shorter than the default 600 s meta window.
    let exact_s = fmt.num_frames as f64 / f64::from(fmt.sample_rate);
    assert!(
        (m.measured_s - exact_s).abs() < 1e-9,
        "{id}: measured {}",
        m.measured_s
    );

    if f.truth.is_silent == Some(true) {
        // Also every lossy fixture but the AAC burst: digital silence.
        assert_eq!((m.peak_dbfs, m.loudness_lufs), (None, None), "{id}");
        return None;
    }
    if fmt.num_frames == 0 {
        return None;
    }
    let peak = m.peak_dbfs.unwrap_or_else(|| panic!("{id}: no peak"));
    let lufs = m
        .loudness_lufs
        .unwrap_or_else(|| panic!("{id}: no loudness"));
    assert!(peak.is_finite() && lufs.is_finite(), "{id}");
    if let Some(want) = f.truth.peak_dbfs {
        // Lossless: the stored samples; lossy (AAC burst): ±0.5 dB.
        let tol = if fmt.lossless { PEAK_TOL_LOSSLESS } else { 0.5 };
        assert!(
            (peak - want).abs() <= tol,
            "{id}: peak {peak:.4} vs {want:.4} dBFS"
        );
    }
    let want = predicted_sine_lufs(f)?;
    let tol = if fmt.duration_s >= 0.4 {
        SINE_TOL
    } else {
        SHORT_SINE_TOL
    };
    let err = lufs - want;
    assert!(
        err.abs() <= tol,
        "{id}: {lufs:.3} vs predicted {want:.3} LUFS"
    );
    Some(err)
}

#[test]
fn every_fixture_meta_matches_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = generate(tmp.path(), &FixtureOptions::default()).unwrap();
    let opts = DecodeOptions::default();
    let meta_opts = MetaOptions::default();
    let mut dec = Decoder::new();
    let (mut ok, mut corrupt, mut sines, mut worst) = (0, 0, 0, 0.0f64);

    let probe = DecodeOptions {
        max_analysis_secs: Some(0.0),
        ..DecodeOptions::default()
    };
    for f in &manifest.fixtures {
        let path = tmp.path().join(&f.path);
        let result = dec.decode_with_meta(&path, &opts, &meta_opts);
        // The decode result does not depend on metering: same as `decode()`
        // with the analysis window and with a zero window (everything
        // decoded only for the meter), corrupt files included. One known
        // difference (ADR-012): when a truncated final packet lies past
        // the analysis window, `decode()` counts its declared duration while
        // metering decodes it and finds fewer frames — only for partial
        // files, only shorter, by less than one packet.
        for o in [&opts, &probe] {
            let plain = dec.decode(&path, o).map_err(|e| e.to_string());
            let with = dec.decode_with_meta(&path, o, &meta_opts);
            let mut with = with.map(|(d, _)| d).map_err(|e| e.to_string());
            if let (Ok(p), Ok(w)) = (&plain, &mut with) {
                let (pn, wn) = (p.source.num_frames, w.source.num_frames);
                if p.partial && wn < pn && pn - wn < MAX_PACKET_FRAMES {
                    eprintln!("{}: counted {pn}, decoded {wn} frames", f.path);
                    w.source.num_frames = pn;
                    w.source.duration_s = p.source.duration_s;
                    w.truncated_for_analysis = p.truncated_for_analysis;
                }
            }
            assert_eq!(plain, with, "{}: {:?}", f.path, o.max_analysis_secs);
        }
        if let Some(c) = &f.truth.corrupt {
            corrupt += 1;
            match (&c.expect, &result) {
                (_, Err(Error::Decode { .. } | Error::UnsupportedFormat { .. })) => {}
                (Expect::ErrorOrPartial, Ok((_, m))) => {
                    for v in [m.peak_dbfs, m.loudness_lufs].into_iter().flatten() {
                        assert!(v.is_finite(), "{}", f.path);
                    }
                }
                (want, got) => panic!("{}: expected {want:?}, got {got:?}", f.path),
            }
            continue;
        }
        let (d, m) = result.unwrap_or_else(|e| panic!("{}: {e}", f.path));
        assert_eq!(d.source.num_frames, m.num_frames, "{}", f.path);
        if let Some(err) = check_audio(f, &m) {
            sines += 1;
            worst = worst.max(err.abs());
        }
        ok += 1;
    }
    eprintln!("meta: {ok} audio, {corrupt} corrupt, {sines} sines predicted (worst {worst:.3} LU)");
    assert!(
        ok >= 100 && corrupt >= 15 && sines >= 30,
        "{ok} {corrupt} {sines}"
    );
    assert!(manifest
        .fixtures
        .iter()
        .any(|f| f.format.as_ref().is_some_and(|x| x.duration_s > 300.0)));
}

/// Fixed-number anchors, independent of the filter code under test: the
/// K-weighting gain at 48 kHz evaluated (Python `cmath`) from the
/// *published* BS.1770-4 Table 1/2 coefficients is −1.133498 dB at 100 Hz
/// and +4.041882 dB at 10 kHz, so a −20 dBFS stereo sine reads
/// −20 + K − 0.691 = −21.8245 / −16.6491 LUFS (1 kHz: −19.9933).
#[test]
fn sine_loudness_matches_published_coefficient_anchors() {
    let tmp = tempfile::tempdir().unwrap();
    let rate = 48_000u32;
    for (hz, want) in [(100.0, -21.8245), (1_000.0, -19.9933), (10_000.0, -16.6491)] {
        let x: Vec<f32> = (0..rate * 5)
            .map(|i| (0.1 * (std::f64::consts::TAU * hz * f64::from(i) / 48_000.0).sin()) as f32)
            .flat_map(|s| [s, s])
            .collect();
        let spec = WavSpec::new(rate, 2, SampleFormat::Float32);
        let path = tmp.path().join(format!("sine_{hz}.wav"));
        std::fs::write(&path, encode(&spec, &x).bytes).unwrap();
        let m = analyze_file(&path, &MetaOptions::default()).unwrap();
        let got = m.loudness_lufs.unwrap();
        assert!((got - want).abs() < 0.01, "{hz} Hz: {got:.4} vs {want}");
    }
}

/// Throughput of the pipeline decode with and without meta, over the full
/// set, the 310 s file alone and a 600 s AAC file (printed).
#[test]
#[ignore = "benchmark: cargo test --release --test meta -- --ignored --nocapture"]
fn throughput_with_and_without_meta() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = generate(tmp.path(), &FixtureOptions::default()).unwrap();
    let paths: Vec<_> = manifest
        .fixtures
        .iter()
        .filter(|f| f.truth.corrupt.is_none())
        .map(|f| {
            (
                tmp.path().join(&f.path),
                f.format.as_ref().unwrap().duration_s,
            )
        })
        .collect();
    let opts = DecodeOptions::default();
    let mut dec = Decoder::new();
    let time = |dec: &mut Decoder, meta: bool, only_long: bool| {
        let t = Instant::now();
        let mut n = 0;
        for (p, secs) in &paths {
            if only_long && *secs < 300.0 {
                continue;
            }
            if meta {
                dec.decode_with_meta(p, &opts, &MetaOptions::default())
                    .unwrap();
            } else {
                dec.decode(p, &opts).unwrap();
            }
            n += 1;
        }
        (n, t.elapsed().as_secs_f64())
    };
    let _ = time(&mut dec, true, false); // warm caches / FFT plans
    let (n, off) = time(&mut dec, false, false);
    let (_, on) = time(&mut dec, true, false);
    let (_, long_off) = time(&mut dec, false, true);
    let (_, long_on) = time(&mut dec, true, true);
    eprintln!(
        "decode {n} files: meta off {off:.3} s ({:.0} files/s), meta on {on:.3} s ({:.0} files/s); \
         310 s file: off {:.1} ms, on {:.1} ms",
        n as f64 / off,
        n as f64 / on,
        long_off * 1e3,
        long_on * 1e3
    );

    // 600 s stereo 44.1 kHz AAC-LC (digital silence, real decoder work).
    let spec = AacSpec {
        sample_rate: 44_100,
        channels: 2,
        frames: 600 * 44_100,
        tags: Tags::Both,
        burst_unit: None,
    };
    let aac = tmp.path().join("long.m4a");
    std::fs::write(&aac, aac_encode(&spec).unwrap()).unwrap();
    let mut ms = |meta: bool| {
        let t = Instant::now();
        if meta {
            dec.decode_with_meta(&aac, &opts, &MetaOptions::default())
                .unwrap();
        } else {
            dec.decode(&aac, &opts).unwrap();
        }
        t.elapsed().as_secs_f64() * 1e3
    };
    let (off, on) = (ms(false), ms(true));
    eprintln!("600 s AAC stereo: meta off {off:.1} ms, meta on {on:.1} ms");
}

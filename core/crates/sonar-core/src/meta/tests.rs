//! Meter tests against EBU Tech 3341 (2016) "Loudness Metering: 'EBU
//! Mode' metering to supplement EBU R 128 loudness normalization",
//! Table 1 cases 1–5 (synthesised here: 1 kHz sines, levels in dBFS of
//! the sine *peak*, identical L/R), plus BS.1770 mono/weighting rules and
//! decode integration (meta window, bit-identical analysis samples).

use std::f64::consts::TAU;

use super::*;
use crate::fixtures::wav::{encode, SampleFormat, WavSpec};

/// Tolerance of EBU Tech 3341 for integrated loudness (±0.1 LU).
const EBU_TOL: f64 = 0.1;

/// Feeds a continuous-phase sine of `freq` Hz through a fresh meter:
/// `segs` = `(level dBFS, seconds)`, same signal on `channels` channels
/// with the given weights (1.0 each when `None`).
fn measure(
    rate: u32,
    channels: usize,
    weights: Option<&[f64]>,
    freq: f64,
    segs: &[(f64, f64)],
) -> Meter {
    let ones = vec![1.0; channels];
    let mut m = Meter::new();
    m.reset(rate, weights.unwrap_or(&ones), u64::MAX);
    let mut planes = vec![Vec::with_capacity(4096); channels];
    let mut n = 0u64;
    for &(db, secs) in segs {
        let amp = 10f64.powf(db / 20.0);
        let frames = (secs * f64::from(rate)).round() as u64;
        let mut done = 0;
        while done < frames {
            let len = (frames - done).min(4096) as usize;
            for p in &mut planes {
                p.clear();
            }
            for i in 0..len as u64 {
                let x = (amp * (TAU * freq * (n + i) as f64 / f64::from(rate)).sin()) as f32;
                for p in &mut planes {
                    p.push(x);
                }
            }
            m.push(&planes, 0..len);
            n += len as u64;
            done += len as u64;
        }
    }
    m
}

fn lufs(m: &Meter) -> f64 {
    m.integrated_lufs().expect("loudness above the gate")
}

fn ebu_3341_cases(rate: u32) {
    let cases: [(&str, &[(f64, f64)]); 5] = [
        ("case 1", &[(-23.0, 20.0)]),
        ("case 2", &[(-33.0, 20.0)]),
        ("case 3", &[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)]),
        (
            "case 4",
            &[
                (-72.0, 10.0),
                (-36.0, 10.0),
                (-23.0, 60.0),
                (-36.0, 10.0),
                (-72.0, 10.0),
            ],
        ),
        ("case 5", &[(-26.0, 20.0), (-20.0, 20.1), (-26.0, 20.0)]),
    ];
    for (name, segs) in cases {
        let want = if name == "case 2" { -33.0 } else { -23.0 };
        let got = lufs(&measure(rate, 2, None, 1_000.0, segs));
        eprintln!("EBU 3341 {name} @ {rate} Hz: {got:.4} LUFS (want {want})");
        assert!(
            (got - want).abs() <= EBU_TOL,
            "{name} @ {rate} Hz: {got:.3} vs {want}"
        );
    }
}

#[test]
fn ebu_tech_3341_cases_1_to_5_at_48k() {
    ebu_3341_cases(48_000);
}

#[test]
fn ebu_tech_3341_cases_1_to_5_at_44k1() {
    ebu_3341_cases(44_100);
}

/// BS.1770 mono = one channel with G = 1 (no dual-mono +3 dB): a
/// full-scale sine (mean square 0.5) reads 10·log10(0.5) = −3.01 LUFS.
#[test]
fn mono_full_scale_sine_is_minus_3_01() {
    let got = lufs(&measure(48_000, 1, None, 1_000.0, &[(0.0, 5.0)]));
    eprintln!("mono full-scale 1 kHz: {got:.4} LUFS");
    assert!((got + 3.01).abs() <= EBU_TOL, "{got}");
}

#[test]
fn loudness_is_rate_independent() {
    let at = |rate| lufs(&measure(rate, 2, None, 1_000.0, &[(-20.0, 3.0)]));
    let reference = at(48_000);
    assert!((reference + 20.0).abs() <= EBU_TOL, "{reference}");
    for rate in [22_050, 44_100, 96_000] {
        let got = at(rate);
        eprintln!("1 kHz −20 dBFS stereo @ {rate} Hz: {got:.4} LUFS (48 kHz: {reference:.4})");
        assert!(
            (got - reference).abs() <= EBU_TOL,
            "{rate} Hz: {got} vs {reference}"
        );
    }
}

#[test]
fn surround_channels_weigh_1_41_and_lfe_is_ignored() {
    // Planes: L, R, C, LFE, Ls, Rs (5.1). Same sine on one channel only.
    let w = [1.0, 1.0, 1.0, 0.0, 1.41, 1.41];
    let only = |ch: usize| {
        let mut ws = w;
        for (i, x) in ws.iter_mut().enumerate() {
            if i != ch {
                *x = 0.0;
            }
        }
        lufs(&measure(48_000, 6, Some(&ws), 1_000.0, &[(-20.0, 2.0)]))
    };
    let front = only(0);
    assert!((only(4) - front - 10.0 * 1.41f64.log10()).abs() < 1e-6);
    let mut lfe = measure(48_000, 6, Some(&w), 1_000.0, &[(-20.0, 2.0)]);
    let all = lufs(&lfe);
    // Five weighted channels: 3 × 1.0 + 2 × 1.41.
    assert!((all - front - 10.0 * 5.82f64.log10()).abs() < 1e-6);
    lfe.reset(48_000, &[0.0], u64::MAX);
    lfe.push(&[vec![0.5; 48_000]], 0..48_000);
    assert_eq!(lfe.integrated_lufs(), None, "LFE alone is not measured");
    assert!(
        (lfe.peak_dbfs().unwrap() + 6.0206).abs() < 1e-3,
        "peak still counts"
    );
}

#[test]
fn silence_and_sub_gate_signals_have_no_loudness() {
    let m = measure(44_100, 2, None, 1_000.0, &[(-200.0, 1.0)]);
    assert_eq!(m.integrated_lufs(), None);
    let mut m = Meter::new();
    m.reset(44_100, &[1.0], u64::MAX);
    m.push(&[vec![0.0; 44_100]], 0..44_100);
    assert_eq!((m.peak_dbfs(), m.integrated_lufs()), (None, None));
    // −75 dBFS mono sine ≈ −78 LUFS: below the −70 LUFS absolute gate.
    let m = measure(44_100, 1, None, 1_000.0, &[(-75.0, 1.0)]);
    assert_eq!(m.integrated_lufs(), None);
    assert!((m.peak_dbfs().unwrap() + 75.0).abs() < 0.01);
    // Nothing measured at all.
    let mut m = Meter::new();
    m.reset(48_000, &[1.0, 1.0], 0);
    m.push(&[vec![1.0; 10], vec![1.0; 10]], 0..10);
    assert_eq!(
        (m.peak_dbfs(), m.integrated_lufs(), m.measured_frames()),
        (None, None, 0)
    );
}

/// Shorter than one 400 ms block: measured as one block over the span.
#[test]
fn spans_shorter_than_a_block_are_one_block() {
    let got = lufs(&measure(48_000, 2, None, 1_000.0, &[(-20.0, 0.2)]));
    assert!((got + 20.0).abs() < 0.2, "{got}");
    let got = lufs(&measure(48_000, 2, None, 1_000.0, &[(-20.0, 0.39)]));
    assert!((got + 20.0).abs() < 0.2, "{got}");
}

/// Either side of the 400 ms block boundary: 399 ms is one whole-span
/// block (approximation), 401 ms is one gated BS.1770 block (the trailing
/// 1 ms is dropped). Both agree with −20 LUFS and with each other.
#[test]
fn block_boundary_399_and_401_ms_agree() {
    let at = |secs| lufs(&measure(48_000, 2, None, 1_000.0, &[(-20.0, secs)]));
    let (short, long) = (at(0.399), at(0.401));
    eprintln!("399 ms: {short:.4} LUFS, 401 ms: {long:.4} LUFS");
    assert!((short + 20.0).abs() < 0.2 && (long + 20.0).abs() < 0.2);
    assert!((short - long).abs() < 0.2, "{short} vs {long}");
}

/// A plain 4-channel WAV (no channel mask) with signal only on channel 4:
/// symphonia's count-derived default layout calls it LFE, but it must
/// still be measured (unknown layout → all weights 1.0).
#[test]
fn plain_four_channel_wav_measures_every_channel() {
    let tmp = tempfile::tempdir().unwrap();
    let rate = 48_000;
    let silent = vec![0.0; rate as usize];
    let ch4 = tone(rate, 1.0, 0.1);
    let chans = [silent.clone(), silent.clone(), silent, ch4];
    let path = write_wav(tmp.path(), "quad.wav", rate, &chans);
    let m = analyze_file(&path, &MetaOptions::default()).unwrap();
    assert_eq!(m.channels, 4);
    // One −20 dBFS 1 kHz channel of weight 1: −20 − 3.01 ≈ −23.0 LUFS.
    let l = m.loudness_lufs.expect("channel 4 is measured");
    assert!((l + 23.01).abs() < 0.1, "{l}");
}

#[test]
fn peak_is_max_abs_over_channels_and_ignores_non_finite() {
    let mut m = Meter::new();
    m.reset(48_000, &[1.0, 1.0], u64::MAX);
    let l = vec![0.1, -0.25, f32::NAN, 0.0];
    let r = vec![0.2, f32::INFINITY, f32::NEG_INFINITY, 0.05];
    m.push(&[l, r], 0..4);
    let want = 20.0 * 0.25f64.log10();
    assert!((m.peak_dbfs().unwrap() - want).abs() < 1e-9);
    // Float files may exceed full scale.
    m.push(&[vec![2.0], vec![0.0]], 0..1);
    assert!((m.peak_dbfs().unwrap() - 6.0206).abs() < 1e-3);
}

#[test]
fn chunking_silence_and_cap_are_consistent() {
    let rate = 44_100;
    let x: Vec<f32> = (0..rate * 2)
        .map(|i| {
            if i % 3000 < 1000 {
                0.0
            } else {
                (i as f32 * 0.05).sin() * 0.3
            }
        })
        .collect();
    let whole = {
        let mut m = Meter::new();
        m.reset(rate, &[1.0], u64::MAX);
        m.push(std::slice::from_ref(&x), 0..x.len());
        m
    };
    let mut chunked = Meter::new();
    chunked.reset(rate, &[1.0], u64::MAX);
    let planes = vec![x.clone()];
    let mut pos = 0;
    for len in [1, 1151, 4410, 7, 100_000].iter().cycle() {
        let end = (pos + len).min(x.len());
        chunked.push(&planes, pos..end);
        pos = end;
        if pos == x.len() {
            break;
        }
    }
    let (a, b) = (lufs(&whole), lufs(&chunked));
    assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    assert_eq!(whole.peak_dbfs(), chunked.peak_dbfs());

    // push_silence == pushing zeros.
    let mut zeros = Meter::new();
    zeros.reset(rate, &[1.0], u64::MAX);
    zeros.push(&planes, 0..5000);
    zeros.push(&[vec![0.0; 3000]], 0..3000);
    zeros.push(&planes, 5000..x.len());
    let mut gap = Meter::new();
    gap.reset(rate, &[1.0], u64::MAX);
    gap.push(&planes, 0..5000);
    gap.push_silence(3000);
    gap.push(&planes, 5000..x.len());
    assert_eq!(zeros.integrated_lufs(), gap.integrated_lufs());
    assert_eq!(zeros.measured_frames(), gap.measured_frames());

    // The cap stops measuring mid-push; ranges past the planes are clipped.
    let mut capped = Meter::new();
    capped.reset(rate, &[1.0], 10_000);
    capped.push(&planes, 0..8_000);
    assert!(!capped.is_full());
    capped.push(&planes, 8_000..usize::MAX);
    capped.push_silence(50);
    assert!(capped.is_full());
    assert_eq!(capped.measured_frames(), 10_000);
}

#[test]
fn meta_window_and_duration_arithmetic() {
    assert_eq!(MetaOptions::default().max_frames(48_000), 28_800_000);
    let at = |s| MetaOptions { max_meta_secs: s }.max_frames(44_100);
    assert_eq!((at(-1.0), at(f64::NAN), at(0.0)), (0, 0, 0));
    assert_eq!(at(f64::INFINITY), u64::MAX);
    assert_eq!(at(0.5), 22_050);
    assert_eq!(duration_ms(44_100, 44_100), 1_000);
    assert_eq!(duration_ms(22, 44_100), 0);
    assert_eq!(duration_ms(23, 44_100), 1);
    assert_eq!(duration_ms(u64::MAX, 1_000), u64::MAX);
    assert_eq!(super::meter::step_frames(11_025), 1_103);
}

fn write_wav(dir: &Path, name: &str, rate: u32, chans: &[Vec<f32>]) -> std::path::PathBuf {
    let spec = WavSpec::new(rate, chans.len() as u16, SampleFormat::Int24);
    let frames = chans[0].len();
    let inter: Vec<f32> = (0..frames)
        .flat_map(|i| chans.iter().map(move |c| c[i]))
        .collect();
    let path = dir.join(name);
    std::fs::write(&path, encode(&spec, &inter).bytes).unwrap();
    path
}

fn tone(rate: u32, secs: f64, amp: f64) -> Vec<f32> {
    (0..(secs * f64::from(rate)) as usize)
        .map(|i| (amp * (TAU * 1_000.0 * i as f64 / f64::from(rate)).sin()) as f32)
        .collect()
}

/// The meta window caps what is measured, not the duration; the analysis
/// samples are bit-identical with and without meta.
#[test]
fn meta_window_caps_measurement_and_keeps_analysis_samples() {
    let tmp = tempfile::tempdir().unwrap();
    let rate = 44_100;
    // 2 s at −20 dBFS, then 4 s at −6 dBFS.
    let mut x = tone(rate, 2.0, 0.1);
    x.extend(tone(rate, 4.0, 0.5));
    let path = write_wav(tmp.path(), "steps.wav", rate, &[x.clone(), x]);
    let mut dec = Decoder::new();
    for analysis in [Some(0.0), Some(1.0), Some(30.0), None] {
        let opts = DecodeOptions {
            max_analysis_secs: analysis,
            ..DecodeOptions::default()
        };
        let plain = dec.decode(&path, &opts).unwrap();
        let (d, short) = dec
            .decode_with_meta(&path, &opts, &MetaOptions { max_meta_secs: 1.5 })
            .unwrap();
        assert_eq!(d, plain, "{analysis:?}: analysis output changed by meta");
        assert_eq!((short.num_frames, short.duration_ms), (6 * 44_100, 6_000));
        assert!(
            (short.measured_s - 1.5).abs() < 1e-9,
            "{}",
            short.measured_s
        );
        assert!((short.peak_dbfs.unwrap() + 20.0).abs() < 0.01);
        assert!((short.loudness_lufs.unwrap() + 20.0).abs() < 0.1);
        let (_, full) = dec
            .decode_with_meta(&path, &opts, &MetaOptions::default())
            .unwrap();
        assert!((full.measured_s - 6.0).abs() < 1e-9);
        assert!((full.peak_dbfs.unwrap() + 6.02).abs() < 0.01);
        // The relative gate drops the blocks of the −20 dBFS part (14 LU
        // below) but keeps the 3 blocks straddling the step (¼, ½, ¾
        // loud): mean of 37 + 1.5 + 0.06 loud-block powers over 40 blocks
        // = −0.16 dB below the −6.02 LUFS of the loud part alone.
        assert!((full.loudness_lufs.unwrap() + 6.18).abs() < 0.05);
        assert!(!full.partial && !short.partial);
    }
    let alone = analyze_file(&path, &MetaOptions::default()).unwrap();
    assert_eq!(
        alone,
        dec.decode_with_meta(&path, &secs(30.0), &MetaOptions::default())
            .unwrap()
            .1
    );
    let none = analyze_file(&path, &MetaOptions { max_meta_secs: 0.0 }).unwrap();
    assert_eq!(
        (none.peak_dbfs, none.loudness_lufs, none.measured_s),
        (None, None, 0.0)
    );
    assert_eq!(none.num_frames, 6 * 44_100);
}

fn secs(s: f64) -> DecodeOptions {
    DecodeOptions {
        max_analysis_secs: Some(s),
        ..DecodeOptions::default()
    }
}

#[test]
fn corrupt_input_is_an_error_not_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let junk = tmp.path().join("junk.wav");
    std::fs::write(&junk, b"RIFF\x10\0\0\0WAVEnot audio").unwrap();
    assert!(analyze_file(&junk, &MetaOptions::default()).is_err());
    assert!(analyze_file(&tmp.path().join("missing.wav"), &MetaOptions::default()).is_err());
}

#[test]
fn basic_meta_json_has_no_infinities() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_wav(tmp.path(), "silent.wav", 48_000, &[vec![0.0; 4800]]);
    let m = analyze_file(&path, &MetaOptions::default()).unwrap();
    assert_eq!((m.peak_dbfs, m.loudness_lufs), (None, None));
    assert_eq!(
        (m.sample_rate, m.channels, m.bit_depth),
        (48_000, 1, Some(24))
    );
    let json = serde_json::to_value(&m).unwrap();
    assert!(json["peak_dbfs"].is_null() && json["loudness_lufs"].is_null());
    assert_eq!(json["duration_ms"], 100);
}

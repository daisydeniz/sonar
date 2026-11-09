//! Decoder tests on small hand-built files: WAV via the fixture encoder,
//! AIFF (big-endian PCM) written byte by byte here. Corrupt-input tests
//! live in `robust_tests.rs` and reuse these helpers.

use std::f64::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::*;
use crate::fixtures::wav::{encode, SampleFormat, WavSpec};
use crate::Error;

pub(super) fn sine(freq: f64, rate: u32, frames: usize, amp: f64) -> Vec<f32> {
    (0..frames)
        .map(|i| (amp * (TAU * freq * i as f64 / f64::from(rate)).sin()) as f32)
        .collect()
}

pub(super) fn interleave(chans: &[Vec<f32>]) -> Vec<f32> {
    let frames = chans[0].len();
    (0..frames)
        .flat_map(|i| chans.iter().map(move |c| c[i]))
        .collect()
}

pub(super) fn wav(rate: u32, fmt: SampleFormat, chans: &[Vec<f32>]) -> Vec<u8> {
    let spec = WavSpec::new(rate, chans.len() as u16, fmt);
    encode(&spec, &interleave(chans)).bytes
}

/// Minimal AIFF: `COMM` + `SSND`, big-endian signed PCM.
pub(super) fn aiff(rate: u32, channels: u16, bits: u16, samples: &[i32]) -> Vec<u8> {
    let frames = samples.len() / usize::from(channels);
    let mut data = Vec::new();
    for s in samples {
        match bits {
            16 => data.extend_from_slice(&(*s as i16).to_be_bytes()),
            24 => data.extend_from_slice(&s.to_be_bytes()[1..]),
            _ => unreachable!("test helper supports 16/24 bit"),
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    let form_len = 4 + (8 + 18) + (8 + 8 + data.len()) + data.len() % 2;
    out.extend_from_slice(&(form_len as u32).to_be_bytes());
    out.extend_from_slice(b"AIFFCOMM");
    out.extend_from_slice(&18u32.to_be_bytes());
    out.extend_from_slice(&channels.to_be_bytes());
    out.extend_from_slice(&(frames as u32).to_be_bytes());
    out.extend_from_slice(&bits.to_be_bytes());
    // 80-bit IEEE extended sample rate (integer rates only).
    let e = 31 - rate.leading_zeros();
    out.extend_from_slice(&(16_383 + e as u16).to_be_bytes());
    out.extend_from_slice(&(u64::from(rate) << (63 - e)).to_be_bytes());
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&((8 + data.len()) as u32).to_be_bytes());
    out.extend_from_slice(&[0; 8]); // offset, block size
    out.extend_from_slice(&data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

pub(super) struct Tmp(tempfile::TempDir);

impl Tmp {
    pub(super) fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    pub(super) fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

pub(super) fn secs(s: f64) -> DecodeOptions {
    DecodeOptions {
        max_analysis_secs: Some(s),
        ..DecodeOptions::default()
    }
}

pub(super) fn is_decode_error(r: &Result<Decoded>) -> bool {
    matches!(
        r,
        Err(Error::Decode { .. } | Error::UnsupportedFormat { .. })
    )
}

#[test]
fn stereo_wav_is_downmixed_resampled_and_described() {
    let tmp = Tmp::new();
    let tone = sine(440.0, 44_100, 88_200, 0.5);
    let p = tmp.write(
        "a.wav",
        &wav(44_100, SampleFormat::Int16, &[tone.clone(), tone]),
    );
    let d = decode_file(&p, &DecodeOptions::default()).unwrap();
    assert_eq!(d.sample_rate, ANALYSIS_RATE);
    assert_eq!(d.samples.len(), 44_100);
    assert!(!d.truncated_for_analysis && !d.partial && d.problem.is_none());
    let s = &d.source;
    assert_eq!(
        (s.container.as_str(), s.codec.as_str()),
        ("wave", "pcm_s16le")
    );
    assert_eq!(
        (s.sample_rate, s.channels, s.bit_depth),
        (44_100, 2, Some(16))
    );
    assert_eq!((s.num_frames, s.declared_frames), (88_200, Some(88_200)));
    assert_eq!(s.duration_s, 2.0);
    let want = sine(440.0, 22_050, 44_100, 0.5);
    let err = d.samples[441..43_659]
        .iter()
        .zip(&want[441..43_659])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(err < 1e-3, "max deviation {err}");
}

#[test]
fn same_rate_file_is_the_exact_channel_mean() {
    let tmp = Tmp::new();
    let l: Vec<f32> = (0..1_000).map(|i| (i as f32 * 0.01).sin() * 0.8).collect();
    let r: Vec<f32> = (0..1_000).map(|i| (i as f32 * 0.03).cos() * 0.4).collect();
    let p = tmp.write(
        "f.wav",
        &wav(22_050, SampleFormat::Float32, &[l.clone(), r.clone()]),
    );
    let d = decode_file(&p, &DecodeOptions::default()).unwrap();
    assert_eq!(d.source.codec, "pcm_f32le");
    assert_eq!(d.source.bit_depth, Some(32));
    assert_eq!(d.samples.len(), 1_000);
    for i in 0..1_000 {
        assert!(
            (d.samples[i] - (l[i] + r[i]) / 2.0).abs() < 1e-7,
            "frame {i}"
        );
    }
}

#[test]
fn window_caps_samples_but_duration_stays_exact() {
    let tmp = Tmp::new();
    // Down- and upsampling; 8/16 kHz have FFT blocks longer than 50 ms.
    for rate in [48_000u32, 16_000, 8_000] {
        let frames = rate as usize * 7 / 2; // 3.5 s
        let tone: Vec<f32> = sine(440.0, rate, frames, 0.4)
            .iter()
            .zip(sine(2_500.0, rate, frames, 0.3))
            .map(|(a, b)| a + b)
            .collect();
        let p = tmp.write("long.wav", &wav(rate, SampleFormat::Int24, &[tone]));
        let d = decode_file(&p, &secs(1.0)).unwrap();
        assert_eq!(d.samples.len(), 22_050, "{rate} Hz");
        assert!(d.truncated_for_analysis && !d.partial);
        assert_eq!(d.source.num_frames, frames as u64);
        assert_eq!(d.source.duration_s, 3.5);
        assert_eq!(d.source.bit_depth, Some(24));
        // The windowed output is the start of the full-length output,
        // right up to the window edge.
        // (Equality holds because both runs feed the resampler identical
        // chunks up to the edge; deterministic FFT on one platform.)
        let full = decode_file(
            &p,
            &DecodeOptions {
                max_analysis_secs: None,
                ..secs(0.0)
            },
        )
        .unwrap();
        assert_eq!(full.samples.len(), 77_175, "{rate} Hz");
        assert!(!full.truncated_for_analysis);
        let diff = d
            .samples
            .iter()
            .zip(&full.samples)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        // Bit-identical: the margin covers every input frame the edge
        // output depends on (a bare 50 ms margin left 3e-7 at 8 kHz).
        assert_eq!(diff, 0.0, "{rate} Hz: window edge differs by {diff}");
    }
}

#[test]
fn zero_or_invalid_window_gives_duration_only() {
    let tmp = Tmp::new();
    let p = tmp.write(
        "w.wav",
        &wav(44_100, SampleFormat::Int16, &[vec![0.1; 4_410]]),
    );
    for s in [0.0, -3.0, f64::NAN] {
        let d = decode_file(&p, &secs(s)).unwrap();
        assert!(d.samples.is_empty());
        assert_eq!(d.source.num_frames, 4_410);
        assert!(d.truncated_for_analysis);
    }
}

#[test]
fn zero_frame_file_is_ok_and_empty() {
    let tmp = Tmp::new();
    let p = tmp.write("z.wav", &wav(44_100, SampleFormat::Int16, &[vec![]]));
    let d = decode_file(&p, &DecodeOptions::default()).unwrap();
    assert!(d.samples.is_empty());
    assert_eq!((d.source.num_frames, d.source.duration_s), (0, 0.0));
    assert!(!d.partial && !d.truncated_for_analysis);
}

#[test]
fn aiff_big_endian_pcm_decodes() {
    let tmp = Tmp::new();
    // 16-bit stereo at the analysis rate: exact values survive.
    let ints: Vec<i32> = (0..400).map(|i| (i * 157 % 30_000) - 15_000).collect();
    let p = tmp.write("s.aif", &aiff(22_050, 2, 16, &ints));
    let d = decode_file(&p, &DecodeOptions::default()).unwrap();
    assert_eq!(d.source.container, "aiff");
    assert_eq!((d.source.channels, d.source.bit_depth), (2, Some(16)));
    assert_eq!(d.source.num_frames, 200);
    for (i, s) in d.samples.iter().enumerate() {
        let want = (ints[2 * i] + ints[2 * i + 1]) as f32 / 2.0 / 32_768.0;
        assert!((s - want).abs() < 1e-6, "frame {i}: {s} vs {want}");
    }
    // 24-bit mono at 44.1 kHz: resampled, level kept.
    let tone: Vec<i32> = sine(1_000.0, 44_100, 44_100, 0.5)
        .iter()
        .map(|s| (f64::from(*s) * 8_388_608.0).round() as i32)
        .collect();
    let p = tmp.write("m.aiff", &aiff(44_100, 1, 24, &tone));
    let d = decode_file(&p, &DecodeOptions::default()).unwrap();
    assert_eq!(
        (d.source.sample_rate, d.source.bit_depth),
        (44_100, Some(24))
    );
    assert_eq!(d.samples.len(), 22_050);
    let peak = d.samples[441..21_609]
        .iter()
        .fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((peak - 0.5).abs() < 2e-3, "peak {peak}");
}

#[test]
fn cancel_flag_stops_with_cancelled() {
    let tmp = Tmp::new();
    let p = tmp.write(
        "c.wav",
        &wav(44_100, SampleFormat::Int16, &[vec![0.1; 10_000]]),
    );
    let opts = DecodeOptions {
        cancel: Some(Arc::new(AtomicBool::new(true))),
        ..DecodeOptions::default()
    };
    assert!(matches!(decode_file(&p, &opts), Err(Error::Cancelled)));
}

#[test]
fn invalid_target_rate_is_an_error() {
    let tmp = Tmp::new();
    let p = tmp.write(
        "r.wav",
        &wav(44_100, SampleFormat::Int16, &[vec![0.1; 100]]),
    );
    let opts = DecodeOptions {
        target_rate: 0,
        ..DecodeOptions::default()
    };
    assert!(is_decode_error(&decode_file(&p, &opts)));
}

#[test]
fn reused_decoder_matches_fresh_decodes() {
    let tmp = Tmp::new();
    let a = tmp.write(
        "a.wav",
        &wav(
            44_100,
            SampleFormat::Int16,
            &[sine(300.0, 44_100, 30_000, 0.4)],
        ),
    );
    let b = tmp.write(
        "b.wav",
        &wav(
            48_000,
            SampleFormat::Float32,
            &[sine(90.0, 48_000, 9_000, 0.9)],
        ),
    );
    let opts = DecodeOptions::default();
    let mut dec = Decoder::new();
    let a1 = dec.decode(&a, &opts).unwrap();
    let b1 = dec.decode(&b, &opts).unwrap();
    let a2 = dec.decode(&a, &opts).unwrap();
    assert_eq!(a1, a2);
    assert_eq!(a1, decode_file(&a, &opts).unwrap());
    assert_eq!(b1, decode_file(&b, &opts).unwrap());
}

#[test]
fn window_arithmetic() {
    // 44.1 kHz: 50 ms (2,205) beats two 1,024-frame blocks.
    let w = analysis_window(Some(30.0), 44_100, 22_050, 1_024);
    assert_eq!(w.window_frames, 1_323_000);
    assert_eq!(w.keep_frames, 1_323_000 + 2_205);
    assert_eq!(w.max_out, 661_500);
    // 8 kHz: two 1,120-frame blocks (280 ms) beat 50 ms (400).
    let w = analysis_window(Some(1.0), 8_000, 22_050, 1_120);
    assert_eq!(w.keep_frames, 8_000 + 2_240);
    let w = analysis_window(Some(30.0), 22_050, 22_050, 0);
    assert_eq!(w.keep_frames, 661_500);
    let w = analysis_window(None, 768_000, 22_050, 1_280);
    assert_eq!(w.keep_frames, HARD_MAX_SOURCE_FRAMES);
    let w = analysis_window(Some(f64::INFINITY), 48_000, 22_050, 1_280);
    assert_eq!(w.keep_frames, HARD_MAX_SOURCE_FRAMES);
    let w = analysis_window(Some(-1.0), 48_000, 22_050, 1_280);
    assert_eq!(w.keep_frames, 0);
}

#[test]
fn decoded_reports_analysis_secs() {
    let d = Decoded {
        samples: vec![0.0; 11_025],
        sample_rate: 22_050,
        source: SourceInfo {
            container: "wave".into(),
            codec: "pcm_s16le".into(),
            sample_rate: 44_100,
            channels: 1,
            bit_depth: Some(16),
            num_frames: 22_050,
            declared_frames: Some(22_050),
            duration_s: 0.5,
        },
        truncated_for_analysis: false,
        partial: false,
        problem: None,
    };
    assert_eq!(d.analysis_secs(), 0.5);
}

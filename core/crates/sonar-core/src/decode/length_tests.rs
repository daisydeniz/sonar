//! Exact length of truncated and over-long PCM WAV/AIFF (T1.7d): counted
//! by the bytes each packet holds, AIFF `COMM` honoured, truncated `FORM`
//! reopened; header and counted paths agree, and decoded output never
//! holds phantom frames.

use super::tests::{aiff, sine, wav, Tmp};
use super::*;
use crate::fixtures::wav::SampleFormat;

/// Every probe the code base uses, on one file: the two length-only probes
/// (header path allowed and always counted, which must agree), a short and
/// the default analysis window, the dsp stage (window + meter) and a whole
/// file decode at the native rate. All must report `frames`; the decoded
/// output must hold exactly the surviving frames. Returns the dsp result.
fn check_all(path: &Path, rate: u32, frames: u64, partial: bool) -> (Decoded, BasicMeta) {
    let name = path.display().to_string();
    let mut dec = Decoder::new();
    let zero = DecodeOptions {
        max_analysis_secs: Some(0.0),
        ..DecodeOptions::default()
    };
    let no_meter = MetaOptions { max_meta_secs: 0.0 };
    for m in [None, Some(&no_meter)] {
        let fast = dec
            .decode_guarded(path, &zero, m, true)
            .map_err(|e| e.to_string());
        let slow = dec
            .decode_counted(path, &zero, m)
            .map_err(|e| e.to_string());
        assert_eq!(fast, slow, "{name}: header == counted");
        let (d, meta) = fast.unwrap();
        assert_eq!(
            (d.source.num_frames, d.partial),
            (frames, partial),
            "{name}"
        );
        assert_eq!(d.problem.is_some(), partial, "{name}");
        if let Some(meta) = meta {
            assert_eq!((meta.num_frames, meta.partial), (frames, partial), "{name}");
        }
    }
    let expect_out = |secs: f64| -> usize {
        let window = (secs * f64::from(rate)).ceil() as u64;
        usize::try_from((frames.min(window) * 22_050).div_ceil(u64::from(rate))).unwrap()
    };
    // Counted past a short window, and the default window (resampled).
    for secs in [0.01, 30.0] {
        let d = dec.decode(path, &super::tests::secs(secs)).unwrap();
        assert_eq!(
            (d.source.num_frames, d.partial),
            (frames, partial),
            "{name}"
        );
        assert_eq!(d.samples.len(), expect_out(secs), "{name}: {secs} s");
    }
    // Whole file at the native rate: one output sample per surviving frame.
    let native = DecodeOptions {
        target_rate: rate,
        max_analysis_secs: None,
        cancel: None,
    };
    let d = dec.decode(path, &native).unwrap();
    assert_eq!(d.samples.len() as u64, frames, "{name}: native output");
    assert_eq!(d.source.num_frames, frames, "{name}");
    // The dsp stage: 30 s window plus the meter.
    let (d, meta) = dec
        .decode_with_meta(path, &DecodeOptions::default(), &MetaOptions::default())
        .unwrap();
    assert_eq!(
        (d.source.num_frames, d.partial),
        (frames, partial),
        "{name}"
    );
    assert_eq!((meta.num_frames, meta.partial), (frames, partial), "{name}");
    (d, meta)
}

/// Offset of the first sample byte after the chunk `tag`.
fn payload_at(bytes: &[u8], tag: &[u8; 4], header: usize) -> usize {
    bytes.windows(4).position(|w| w == tag).unwrap() + header
}

#[test]
fn truncated_wav_counts_exactly_the_surviving_frames() {
    let tmp = Tmp::new();
    let rate = 44_100;
    let n = 10_000usize;
    for (fmt, bytes) in [(SampleFormat::Int16, 2), (SampleFormat::Int24, 3)] {
        for channels in [1usize, 2] {
            let chans: Vec<Vec<f32>> = (0..channels)
                .map(|c| sine(440.0 * (c + 1) as f64, rate, n, 0.5))
                .collect();
            let full = wav(rate, fmt, &chans);
            let at = payload_at(&full, b"data", 8);
            let fb = bytes * channels;
            let name = |k: &str| format!("{bytes}b{channels}c_{k}.wav");
            // Intact: header path, not partial.
            check_all(&tmp.write(&name("full"), &full), rate, n as u64, false);
            // Whole frames not on a packet (1,152) boundary, and cuts
            // inside a frame (its bytes are dropped).
            for (k, len) in [
                ("9999", at + 9_999 * fb),
                ("1153", at + 1_153 * fb),
                ("1151", at + 1_151 * fb),
                ("5000_plus1", at + 5_000 * fb + 1),
                ("5000_minus1", at + 5_001 * fb - 1),
                ("last_byte", full.len() - 1),
            ] {
                let surviving = ((len - at) / fb) as u64;
                let p = tmp.write(&name(k), &full[..len]);
                let (d, _) = check_all(&p, rate, surviving, true);
                assert_eq!(d.source.declared_frames, Some(n as u64), "{k}");
            }
        }
    }
}

#[test]
fn truncated_wav_output_is_the_surviving_audio() {
    let tmp = Tmp::new();
    let ch = sine(440.0, 8_000, 3_000, 0.5);
    let full = wav(8_000, SampleFormat::Int16, &[ch.clone(), ch.clone()]);
    let at = payload_at(&full, b"data", 8);
    // 2,000 whole frames and half of the next.
    let p = tmp.write("cut.wav", &full[..at + 2_000 * 4 + 2]);
    let native = DecodeOptions {
        target_rate: 8_000,
        max_analysis_secs: None,
        cancel: None,
    };
    let d = decode_file(&p, &native).unwrap();
    assert_eq!(d.samples.len(), 2_000);
    for (got, want) in d.samples.iter().zip(&ch) {
        assert!((got - want).abs() < 1e-4, "{got} vs {want}");
    }
}

/// [`aiff`] with `extra` bytes after the sound data inside `SSND`, the
/// `COMM` frame count replaced by `comm` and `after` chunks at the end.
fn aiff_with(samples: &[i32], channels: u16, comm: Option<u32>, after: &[u8]) -> Vec<u8> {
    let mut b = aiff(44_100, channels, 16, samples);
    if let Some(c) = comm {
        b[22..26].copy_from_slice(&c.to_be_bytes());
    }
    b.extend_from_slice(after);
    let form = (b.len() - 8) as u32;
    b[4..8].copy_from_slice(&form.to_be_bytes());
    b
}

fn ramp(n: usize) -> Vec<i32> {
    (0..n as i32).map(|i| (i % 2_000) * 10 - 10_000).collect()
}

#[test]
fn truncated_aiff_counts_exactly_the_surviving_frames() {
    let tmp = Tmp::new();
    for channels in [1u16, 2] {
        let n = 10_000usize;
        let full = aiff(44_100, channels, 16, &ramp(n * usize::from(channels)));
        let at = payload_at(&full, b"SSND", 16);
        let fb = 2 * usize::from(channels);
        check_all(
            &tmp.write(&format!("{channels}c_full.aiff"), &full),
            44_100,
            n as u64,
            false,
        );
        for (k, len) in [
            ("9999", at + 9_999 * fb),
            ("1151", at + 1_151 * fb),
            ("mid_frame", at + 4_000 * fb + 1),
            ("last_byte", full.len() - 1),
            ("half", full.len() / 2),
        ] {
            let surviving = ((len - at) / fb) as u64;
            let p = tmp.write(&format!("{channels}c_{k}.aiff"), &full[..len]);
            let (d, _) = check_all(&p, 44_100, surviving, true);
            assert_eq!(d.source.declared_frames, Some(n as u64), "{k}");
        }
    }
    // Cut inside the SSND header: no audio left, an error on every path.
    let full = aiff(44_100, 1, 16, &ramp(100));
    let p = tmp.write("hdr.aiff", &full[..payload_at(&full, b"SSND", 12)]);
    assert!(decode_file(&p, &DecodeOptions::default()).is_err());
}

#[test]
fn aiff_cut_after_its_sound_data_is_complete() {
    let tmp = Tmp::new();
    let anno = [&b"ANNO"[..], &40u32.to_be_bytes(), &[b'x'; 40]].concat();
    let full = aiff_with(&ramp(5_000), 1, None, &anno);
    // Inside the ANNO body, and inside its chunk header: the audio is whole.
    for cut in [20, 45] {
        let p = tmp.write(&format!("anno{cut}.aiff"), &full[..full.len() - cut]);
        check_all(&p, 44_100, 5_000, false);
    }
}

#[test]
fn aiff_comm_is_the_declared_length() {
    let tmp = Tmp::new();
    // SSND holds 6,000 frames; the second 1,000 are loud.
    let mut s = vec![1_000i32; 6_000];
    s[5_000..].fill(30_000);
    // COMM says 5,000: trimmed to 5,000, not partial; the meter never sees
    // the loud tail and the output holds no frame past COMM.
    let p = tmp.write("long.aiff", &aiff_with(&s, 1, Some(5_000), &[]));
    let (d, meta) = check_all(&p, 44_100, 5_000, false);
    assert_eq!(d.source.declared_frames, Some(5_000));
    let peak = meta.peak_dbfs.unwrap();
    let want = 20.0 * (1_000.0f64 / 32_768.0).log10();
    assert!((peak - want).abs() < 0.01, "peak {peak} dBFS");
    // Truncated as well, but every COMM frame survives.
    let full = aiff_with(&s, 1, Some(5_000), &[]);
    let p = tmp.write("long_cut.aiff", &full[..full.len() - 1_999]);
    check_all(&p, 44_100, 5_000, false);
    // Stereo 24-bit, SSND 6,000 frames, COMM 5,000: capped to COMM, not
    // partial; the loud tail past COMM is never measured.
    let mut s24 = vec![256_000i32; 12_000];
    s24[10_000..].fill(7_000_000);
    let mut b = aiff(44_100, 2, 24, &s24);
    b[22..26].copy_from_slice(&5_000u32.to_be_bytes());
    let p = tmp.write("long_s24.aiff", &b);
    let (d, meta) = check_all(&p, 44_100, 5_000, false);
    assert_eq!(d.source.declared_frames, Some(5_000));
    let peak = meta.peak_dbfs.unwrap();
    let want = 20.0 * (256_000.0f64 / 8_388_608.0).log10();
    assert!((peak - want).abs() < 0.01, "s24 peak {peak} dBFS");
    // COMM says 7,000: the stream is shorter than declared.
    let p = tmp.write("short.aiff", &aiff_with(&s, 1, Some(7_000), &[]));
    let (d, _) = check_all(&p, 44_100, 6_000, true);
    assert_eq!(d.source.declared_frames, Some(7_000));
    // COMM 0 (never finalised): the SSND size is used.
    let p = tmp.write("zero.aiff", &aiff_with(&s, 1, Some(0), &[]));
    let (d, _) = check_all(&p, 44_100, 6_000, false);
    assert_eq!(d.source.declared_frames, Some(6_000));
}

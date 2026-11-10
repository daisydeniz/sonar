//! The header length path ([`super::header`]) against the packet-counting
//! path: identical results on every fixture (corrupt ones included), on
//! truncated and oversized-declared files, and the payload is never read.

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use symphonia::core::io::MediaSource;

use super::source::open_source;
use super::tests::{aiff, sine, wav, Tmp};
use super::*;
use crate::fixtures::manifest::{Codec, Container};
use crate::fixtures::wav::SampleFormat;
use crate::fixtures::{generate, FixtureOptions};

type Both = std::result::Result<(Decoded, Option<BasicMeta>), String>;

/// The two length-only probes the code base uses: plain `decode` with a
/// zero window and the meta stage (zero analysis + zero meter window).
fn probes() -> [(DecodeOptions, Option<MetaOptions>); 2] {
    let zero = DecodeOptions {
        max_analysis_secs: Some(0.0),
        ..DecodeOptions::default()
    };
    [
        (zero.clone(), None),
        (zero, Some(MetaOptions { max_meta_secs: 0.0 })),
    ]
}

/// (header path allowed, always counted) for one probe.
fn both(
    dec: &mut Decoder,
    path: &Path,
    o: &DecodeOptions,
    m: Option<&MetaOptions>,
) -> (Both, Both) {
    let fast = dec
        .decode_guarded(path, o, m, true)
        .map_err(|e| e.to_string());
    let counted = dec.decode_counted(path, o, m).map_err(|e| e.to_string());
    (fast, counted)
}

/// Whether the header path accepts `path` (opened like the decoder does;
/// guarded because hostile fixtures can panic inside symphonia).
fn takes_header(path: &Path) -> bool {
    crate::quiet_panic::guarded(|| {
        let Ok(src) = source::open(path) else {
            return false;
        };
        header::pcm_frame_bytes(&src).is_some_and(|b| header::verified_frames(src, b).is_some())
    })
    .unwrap_or(false)
}

#[test]
fn header_length_equals_counted_length_on_every_fixture() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = generate(tmp.path(), &FixtureOptions::default()).unwrap();
    let mut dec = Decoder::new();
    let (mut header, mut counted, mut corrupt) = (0, 0, 0);
    for f in &manifest.fixtures {
        let path = tmp.path().join(&f.path);
        if f.format.is_none() && f.truth.corrupt.is_none() {
            continue; // not audio (manifest etc.)
        }
        for (o, m) in probes() {
            let (fast, slow) = both(&mut dec, &path, &o, m.as_ref());
            assert_eq!(fast, slow, "{}: meta={}", f.path, m.is_some());
        }
        let pcm = f.format.as_ref().is_some_and(|x| {
            x.codec == Codec::Pcm && matches!(x.container, Container::Wav | Container::Aiff)
        });
        if takes_header(&path) {
            header += 1;
        } else {
            counted += 1;
            assert!(
                !pcm || f.truth.corrupt.is_some(),
                "{}: intact PCM WAV/AIFF must take the header path",
                f.path
            );
        }
        corrupt += usize::from(f.truth.corrupt.is_some());
    }
    // (The corrupt `expect` contract in the meta stage is checked in
    // `jobs::work::tests`.)
    eprintln!("header path {header}, counted {counted} ({corrupt} corrupt)");
    assert!(
        header >= 100 && counted >= 30 && corrupt >= 15,
        "{header} / {counted}"
    );
}

/// One file, both paths: identical results; `header` = whether the header
/// path must accept it.
fn compare(tmp: &Tmp, name: &str, bytes: &[u8], header: bool) -> Both {
    let path = tmp.write(name, bytes);
    let mut dec = Decoder::new();
    let mut last = None;
    for (o, m) in probes() {
        let (fast, slow) = both(&mut dec, &path, &o, m.as_ref());
        assert_eq!(fast, slow, "{name}: meta={}", m.is_some());
        last = Some(fast);
    }
    assert_eq!(takes_header(&path), header, "{name}: header path");
    last.unwrap()
}

/// [`compare`] for a file that must be counted.
fn check(tmp: &Tmp, name: &str, bytes: &[u8]) -> Both {
    compare(tmp, name, bytes, false)
}

/// RIFF WAVE with a raw `fmt ` body, `data` (padded to even length) and
/// optional chunks after it.
fn raw_wav(fmt: &[u8], data: &[u8], after: &[u8]) -> Vec<u8> {
    let mut body = b"WAVEfmt ".to_vec();
    body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    body.extend_from_slice(fmt);
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(data);
    if data.len() % 2 == 1 {
        body.push(0);
    }
    body.extend_from_slice(after);
    [&b"RIFF"[..], &(body.len() as u32).to_le_bytes(), &body].concat()
}

/// WAVE `fmt ` body: tag, channels, 8 kHz, bits, and `cbSize` = 0 when
/// `ex` (A-law/µ-law require the 18-byte form).
fn fmt_body(tag: u16, channels: u16, bits: u16, ex: bool) -> Vec<u8> {
    let align = channels * bits / 8;
    let mut f = [tag.to_le_bytes(), channels.to_le_bytes()].concat();
    f.extend_from_slice(&8_000u32.to_le_bytes());
    f.extend_from_slice(&(8_000 * u32::from(align)).to_le_bytes());
    f.extend_from_slice(&align.to_le_bytes());
    f.extend_from_slice(&bits.to_le_bytes());
    if ex {
        f.extend_from_slice(&0u16.to_le_bytes());
    }
    f
}

/// AIFF-C with compression `kind`: `FVER`, `COMM` (44.1 kHz, empty
/// compression name), `SSND`.
fn aifc(kind: &[u8; 4], channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
    let frame = usize::from(channels * bits / 8);
    let mut comm = channels.to_be_bytes().to_vec();
    comm.extend_from_slice(&((data.len() / frame) as u32).to_be_bytes());
    comm.extend_from_slice(&bits.to_be_bytes());
    // 44,100 Hz as an 80-bit IEEE extended: exponent 16383 + 15.
    comm.extend_from_slice(&(16_383u16 + 15).to_be_bytes());
    comm.extend_from_slice(&(44_100u64 << 48).to_be_bytes());
    comm.extend_from_slice(kind);
    comm.extend_from_slice(&[0, 0]); // pascal string "" + pad
    let mut body = b"AIFCFVER".to_vec();
    body.extend_from_slice(&4u32.to_be_bytes());
    body.extend_from_slice(&0xA280_5140u32.to_be_bytes());
    body.extend_from_slice(b"COMM");
    body.extend_from_slice(&(comm.len() as u32).to_be_bytes());
    body.extend_from_slice(&comm);
    body.extend_from_slice(b"SSND");
    body.extend_from_slice(&((data.len() + 8) as u32).to_be_bytes());
    body.extend_from_slice(&[0; 8]);
    body.extend_from_slice(data);
    [&b"FORM"[..], &(body.len() as u32).to_be_bytes(), &body].concat()
}

#[test]
fn every_uncompressed_pcm_variant_takes_the_header_path() {
    let tmp = Tmp::new();
    let ramp: Vec<u8> = (0..4_000u32).map(|i| (i % 251) as u8).collect();
    let floats: Vec<u8> = (0..1_000)
        .flat_map(|i| (0.001f32 * i as f32).to_be_bytes())
        .collect();
    let junk = [&b"JUNK"[..], &4u32.to_le_bytes(), &[0; 4]].concat();
    for (name, bytes, frames) in [
        ("sowt.aifc", aifc(b"sowt", 2, 16, &ramp), 1_000),
        ("fl32.aifc", aifc(b"fl32", 1, 32, &floats), 1_000),
        (
            "u8.wav",
            raw_wav(&fmt_body(1, 2, 8, false), &ramp, &[]),
            2_000,
        ),
        (
            "alaw.wav",
            raw_wav(&fmt_body(6, 1, 8, true), &ramp, &[]),
            4_000,
        ),
        (
            "mulaw.wav",
            raw_wav(&fmt_body(7, 2, 8, true), &ramp, &[]),
            2_000,
        ),
        // Odd-sized 8-bit mono `data`, its pad byte, then another chunk.
        (
            "odd.wav",
            raw_wav(&fmt_body(1, 1, 8, false), &ramp[..1_001], &junk),
            1_001,
        ),
    ] {
        let (d, _) = compare(&tmp, name, &bytes, true).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!d.partial, "{name}");
        assert_eq!(d.source.num_frames, frames, "{name}");
        assert_eq!(d.source.declared_frames, Some(frames), "{name}");
    }
}

fn stereo_wav(frames: usize) -> Vec<u8> {
    let ch = sine(440.0, 44_100, frames, 0.5);
    wav(44_100, SampleFormat::Int16, &[ch.clone(), ch])
}

#[test]
fn truncated_wav_is_counted_and_partial() {
    let tmp = Tmp::new();
    let full = stereo_wav(10_000);
    let wav24 = wav(
        48_000,
        SampleFormat::Int24,
        &[sine(440.0, 48_000, 5_000, 0.5)],
    );
    assert!(takes_header(&tmp.write("full.wav", &full)));
    assert!(takes_header(&tmp.write("full24.wav", &wav24)));
    // Counting sums packet durations; symphonia 0.6.1 delivers a short
    // final PCM packet with its full duration (a short read is not an
    // error), so a cut inside the last packet still counts as complete.
    // The header path must not change that outcome either way.
    for (name, bytes) in [
        ("mid_frame.wav", &full[..full.len() - 1]),
        ("mid_frame24.wav", &wav24[..wav24.len() - 2]),
    ] {
        let (d, _) = check(&tmp, name, bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!d.partial, "{name}: {d:?}");
    }
    for (name, bytes, declared) in [
        ("mid_file.wav", &full[..full.len() / 2], 10_000),
        ("mid_file24.wav", &wav24[..wav24.len() / 3], 5_000),
    ] {
        let (d, _) = check(&tmp, name, bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(d.partial && d.problem.is_some(), "{name}: {d:?}");
        assert_eq!(d.source.declared_frames, Some(declared), "{name}");
        assert!(d.source.num_frames < declared, "{name}");
    }
}

#[test]
fn truncated_aiff_keeps_its_counted_outcome() {
    let tmp = Tmp::new();
    let samples: Vec<i32> = (0..20_000).map(|i| (i % 300) * 50).collect();
    let full = aiff(44_100, 2, 24, &samples);
    assert!(takes_header(&tmp.write("full.aiff", &full)));
    for (name, bytes) in [
        ("mid_frame.aiff", &full[..full.len() - 1]),
        ("mid_file.aiff", &full[..full.len() / 2]),
    ] {
        // symphonia 0.6.1 reads the end of SSND while opening, so a cut AIFF
        // is a decode error on both paths; a result must at least be partial.
        if let Ok((d, _)) = check(&tmp, name, bytes) {
            assert!(d.partial && d.source.num_frames < 10_000, "{name}");
        }
    }
}

#[test]
fn oversized_or_unaligned_declared_chunks_are_counted() {
    let tmp = Tmp::new();
    let full = stereo_wav(1_000);
    let data_at = full.windows(4).position(|w| w == b"data").unwrap();
    let patch = |riff_extra: u32, data_extra: u32, tail: &[u8]| {
        let mut b = full.clone();
        let le = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let riff = le(&b, 4) + riff_extra;
        let data = le(&b, data_at + 4) + data_extra;
        b[4..8].copy_from_slice(&riff.to_le_bytes());
        b[data_at + 4..data_at + 8].copy_from_slice(&data.to_le_bytes());
        b.extend_from_slice(tail);
        b
    };
    // Declares 4,000 more bytes (1,000 frames) than the file holds.
    let (d, _) = check(&tmp, "oversized.wav", &patch(4_000, 4_000, &[])).unwrap();
    assert!(d.partial, "{d:?}");
    assert_eq!(d.source.declared_frames, Some(2_000));
    // Counted, so the last (short) packet counts in full: 1,152 frames.
    assert!((1_000..2_000).contains(&d.source.num_frames), "{d:?}");
    // A data chunk of 4,002 bytes (half a frame extra), all present.
    let (d, _) = check(&tmp, "unaligned.wav", &patch(2, 2, &[0, 0])).unwrap();
    assert!(!d.partial, "{d:?}");
    assert_eq!(d.source.num_frames, 1_000);
    // Streamed/unknown length: data size 0xFFFFFFFF.
    let mut streamed = full.clone();
    streamed[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    streamed[data_at + 4..data_at + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    // (Counted to the end of the file; the short last packet counts in
    // full, see `truncated_wav_is_counted_and_partial`.)
    let (d, _) = check(&tmp, "streamed.wav", &streamed).unwrap();
    assert_eq!(d.source.declared_frames, None);
    assert!(d.source.num_frames >= 1_000);
    // Streamed with a `fact` chunk: symphonia declares the fact length
    // (500) but reads the data to the end of the file. Trusting the
    // declared length here would be wrong; the 0xFFFFFFFF size rejects it.
    let fact = [&b"fact"[..], &4u32.to_le_bytes(), &500u32.to_le_bytes()].concat();
    let with_fact = [&streamed[..data_at], &fact, &streamed[data_at..]].concat();
    let (d, _) = check(&tmp, "streamed_fact.wav", &with_fact).unwrap();
    assert_eq!(d.source.declared_frames, Some(500));
    assert!(d.source.num_frames >= 1_000);
}

/// A byte source that records how many bytes were read from it.
struct Spy(Cursor<Vec<u8>>, Arc<AtomicU64>);

impl Read for Spy {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.0.read(buf)?;
        self.1.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

impl Seek for Spy {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl MediaSource for Spy {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.0.get_ref().len() as u64)
    }
}

#[test]
fn header_path_does_not_read_the_payload() {
    let samples = vec![1_000i32; 2 * 250_000];
    for (name, bytes, max_read) in [
        ("a.wav", stereo_wav(250_000), 16 << 10),
        // symphonia's AIFF demuxer reads the last ≤ 128 KiB of SSND while
        // looking for chunks after it (seek, then read).
        ("a.aiff", aiff(44_100, 2, 16, &samples), 160 << 10),
    ] {
        let read = Arc::new(AtomicU64::new(0));
        let spy = Spy(Cursor::new(bytes.clone()), read.clone());
        let src = open_source(Path::new(name), Box::new(spy)).unwrap();
        let fb = header::pcm_frame_bytes(&src).unwrap();
        assert_eq!(header::verified_frames(src, fb), Some(250_000), "{name}");
        let n = read.load(Ordering::Relaxed);
        assert!(n < max_read, "{name}: read {n} of {} bytes", bytes.len());
    }
}

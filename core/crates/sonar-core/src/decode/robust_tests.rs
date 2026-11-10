//! Corrupt and hostile inputs: every call returns a value (no panic, no
//! hang, bounded output), matching the `expect` contract in `docs/EVAL.md`.

use super::tests::{aiff, is_decode_error, secs, sine, wav, Tmp};
use super::*;
use crate::fixtures::rng::SplitMix64;
use crate::fixtures::wav::SampleFormat;
use crate::Error;

#[test]
fn not_audio_is_an_error() {
    let tmp = Tmp::new();
    let mut rng = SplitMix64::new(7);
    let mut noise = vec![0u8; 4_096];
    rng.fill_bytes(&mut noise);
    let good = wav(44_100, SampleFormat::Int16, &[vec![0.2; 100]]);
    for (name, bytes) in [
        ("empty.wav", Vec::new()),
        ("text.wav", b"definitely not audio\n".to_vec()),
        ("noise.wav", noise),
        ("cut.wav", good[..30].to_vec()),
        ("cut.aiff", aiff(44_100, 1, 16, &[1; 50])[..20].to_vec()),
    ] {
        let r = decode_file(&tmp.write(name, &bytes), &DecodeOptions::default());
        assert!(is_decode_error(&r), "{name}: {r:?}");
    }
}

#[test]
fn absurd_headers_are_rejected_before_allocating() {
    let tmp = Tmp::new();
    let good = wav(44_100, SampleFormat::Int16, &[vec![0.2; 100]]);
    let patch = |at: usize, v: &[u8]| {
        let mut b = good.clone();
        b[at..at + v.len()].copy_from_slice(v);
        b
    };
    for (name, bytes) in [
        ("chans.wav", patch(22, &60_000u16.to_le_bytes())),
        ("rate.wav", patch(24, &4_000_000u32.to_le_bytes())),
        ("lowrate.wav", patch(24, &7u32.to_le_bytes())),
    ] {
        let r = decode_file(&tmp.write(name, &bytes), &DecodeOptions::default());
        assert!(is_decode_error(&r), "{name}: {r:?}");
    }
}

#[test]
fn truncated_stream_is_partial() {
    let tmp = Tmp::new();
    let full = wav(
        44_100,
        SampleFormat::Int16,
        &[sine(440.0, 44_100, 44_100, 0.5)],
    );
    let p = tmp.write("t.wav", &full[..full.len() / 2]);
    let d = decode_file(&p, &DecodeOptions::default()).unwrap();
    assert!(d.partial);
    assert!(d.problem.is_some());
    assert_eq!(d.source.declared_frames, Some(44_100));
    let n = d.source.num_frames;
    assert!(n < 44_100 / 2 && n > 44_100 / 2 - 1_152, "{n} frames");
    assert_eq!(d.samples.len() as u64, (n * 22_050).div_ceil(44_100));
}

/// Seeded byte-level mutations of valid WAV and AIFF files: every call
/// returns a value (panics inside codecs become `Error::Decode`) and the
/// output stays bounded and finite.
#[test]
fn mutated_files_never_panic() {
    let tmp = Tmp::new();
    let bases = [
        (
            "m.wav",
            wav(
                44_100,
                SampleFormat::Int16,
                &[vec![0.3; 3_000], vec![-0.3; 3_000]],
            ),
        ),
        ("m.aiff", aiff(48_000, 1, 24, &vec![100_000; 3_000])),
    ];
    let mut rng = SplitMix64::new(0xDEC0DE);
    let mut outcomes = [0usize; 3];
    let mut dec = Decoder::new();
    for i in 0..400 {
        let (ext, base) = &bases[i % 2];
        let mut b = base.clone();
        let roll = rng.next_u64();
        let at = (rng.next_u64() % 64) as usize;
        match roll % 4 {
            0 => b[at] = rng.next_u64() as u8,
            1 => {
                let v = [0u32, 1, 0x7FFF_FFFF, 0xFFFF_FFFF, rng.next_u64() as u32]
                    [(roll >> 8) as usize % 5];
                let at = at.min(b.len() - 4);
                b[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
            2 => b.truncate((rng.next_u64() as usize) % b.len()),
            _ => {
                for _ in 0..8 {
                    let at = (rng.next_u64() as usize) % b.len();
                    b[at] = rng.next_u64() as u8;
                }
            }
        }
        let p = tmp.write(ext, &b);
        match dec.decode(&p, &secs(1.0)) {
            Ok(d) => {
                assert!(d.samples.len() <= 22_050, "case {i}");
                assert!(d.samples.iter().all(|s| s.is_finite()), "case {i}");
                outcomes[usize::from(d.partial)] += 1;
            }
            // Includes third-party panics turned into errors by the
            // `catch_unwind` guard: with this seed, debug builds hit an
            // integer overflow in symphonia-format-riff 0.6.1
            // (`FormatPcm::make_packet_info`, bits/8 × channels in u16).
            Err(Error::Decode { .. }) => outcomes[2] += 1,
            Err(Error::UnsupportedFormat { .. }) => outcomes[2] += 1,
            Err(e) => panic!("case {i}: unexpected error {e}"),
        }
    }
    // The corpus exercises all three outcomes.
    assert!(outcomes.iter().all(|&n| n > 0), "{outcomes:?}");
}

/// In-memory file whose reads fail (like an unplugged drive) once
/// `limit` bytes have been read in total (also used by `aiff::tests`).
pub(super) struct FailingSource {
    inner: std::io::Cursor<Vec<u8>>,
    read: u64,
    limit: u64,
}

impl FailingSource {
    pub(super) fn new(bytes: Vec<u8>, limit: u64) -> Self {
        Self {
            inner: std::io::Cursor::new(bytes),
            read: 0,
            limit,
        }
    }

    fn boxed(bytes: Vec<u8>, limit: u64) -> Box<dyn symphonia::core::io::MediaSource> {
        Box::new(Self::new(bytes, limit))
    }
}

impl std::io::Read for FailingSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.read >= self.limit {
            return Err(std::io::Error::other("device not configured"));
        }
        let room = usize::try_from(self.limit - self.read).unwrap_or(usize::MAX);
        let take = buf.len().min(room);
        let n = self.inner.read(&mut buf[..take])?;
        self.read += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for FailingSource {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl symphonia::core::io::MediaSource for FailingSource {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.inner.get_ref().len() as u64)
    }
}

#[test]
fn read_failure_is_an_io_error_not_a_partial_result() {
    let path = std::path::Path::new("/Volumes/Gone/loop.wav");
    // 2 s of 16-bit mono at 44.1 kHz = 176 kB; the read fails at 100 kB.
    let bytes = wav(
        44_100,
        SampleFormat::Int16,
        &[sine(440.0, 44_100, 88_200, 0.5)],
    );
    // Fails inside the analysis window (decoding), past it (counting) and
    // while decoding past it for the meta measurement.
    for (keep, metered) in [(1 << 20, false), (0, false), (0, true)] {
        let mut src = source::open_source(path, FailingSource::boxed(bytes.clone(), 100_000))
            .expect("header is readable");
        let (mut planar, mut mono) = (Vec::new(), Vec::new());
        let mut meter = crate::meta::Meter::new();
        meter.reset(44_100, &[1.0], u64::MAX);
        let m = metered.then_some(&mut meter);
        let r = stream::run(path, &mut src, keep, &mut planar, &mut mono, m, None);
        match r {
            Err(Error::Io { path: p, source }) => {
                assert_eq!(p, path);
                assert_eq!(source.kind(), std::io::ErrorKind::Other);
            }
            other => panic!("keep {keep}: expected Error::Io, got {other:?}"),
        }
    }
    // A failure while probing the header is an I/O error too.
    let r = source::open_source(path, FailingSource::boxed(bytes.clone(), 10));
    assert!(matches!(r, Err(Error::Io { .. })), "{:?}", r.err());
    // The same bytes without the failure decode completely.
    let mut src = source::open_source(path, FailingSource::boxed(bytes, u64::MAX)).unwrap();
    let (mut planar, mut mono) = (Vec::new(), Vec::new());
    let out = stream::run(path, &mut src, 1 << 20, &mut planar, &mut mono, None, None).unwrap();
    assert_eq!((out.total_frames, out.partial), (88_200, false));
}

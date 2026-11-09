//! Deliberately broken `.wav` files for decoder robustness tests.
//!
//! Each variant starts from a known-good 16-bit file (44-byte canonical
//! header: `RIFF`@0 size@4 `WAVE`@8 `fmt `@12 len@16 tag@20 channels@22
//! rate@24 byte_rate@28 block_align@32 bits@34 `data`@36 len@40 samples@44)
//! and damages one thing. `expect` is our honest guess of what a robust
//! decoder (e.g. symphonia) does: `Error` only where no sane reading of the
//! bytes yields audio.

use super::manifest::{CorruptTruth, Expect};
use super::rng::SplitMix64;
use super::synth::{additive, Audio};
use super::wav::{encode, SampleFormat, WavSpec};

const OFF_RIFF_SIZE: usize = 4;
const OFF_FORM: usize = 8;
const OFF_FMT_LEN: usize = 16;
const OFF_TAG: usize = 20;
const OFF_CHANNELS: usize = 22;
const OFF_RATE: usize = 24;
const OFF_DATA_ID: usize = 36;
const OFF_DATA_LEN: usize = 40;
const HEADER_LEN: usize = 44;

/// Kinds of damage. Order is the catalog order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptKind {
    /// Zero-byte file.
    Empty,
    /// 4 KiB of PRNG bytes.
    RandomBytes,
    /// Plain UTF-8 text.
    TextFile,
    /// First 30 bytes of a valid file (header cut inside `fmt `).
    TruncatedHeader,
    /// `data` claims 1 s, only 0.25 s present.
    TruncatedData,
    /// RIFF size ~1 MB larger than the file.
    RiffSizeTooLarge,
    /// RIFF size ends before the `data` chunk.
    RiffSizeTooSmall,
    /// `fmt ` chunk only 12 bytes (no block align / bit depth).
    FmtChunkTooSmall,
    /// Undefined format tag 0x7A7A.
    UnsupportedFormatTag,
    /// RIFF form type `AVI ` instead of `WAVE`.
    NotWave,
    /// `RIFF`/`WAVE`/`fmt ` but no `data` chunk.
    MissingDataChunk,
    /// Channel count 0.
    ZeroChannels,
    /// Sample rate 0.
    ZeroSampleRate,
    /// Stereo 16-bit data chunk ending in half a frame.
    PartialFrame,
    /// `LIST` chunk claiming ~4 GiB before `data`.
    HugeChunk,
}

impl CorruptKind {
    /// Every kind, in catalog order.
    pub const ALL: [Self; 15] = [
        Self::Empty,
        Self::RandomBytes,
        Self::TextFile,
        Self::TruncatedHeader,
        Self::TruncatedData,
        Self::RiffSizeTooLarge,
        Self::RiffSizeTooSmall,
        Self::FmtChunkTooSmall,
        Self::UnsupportedFormatTag,
        Self::NotWave,
        Self::MissingDataChunk,
        Self::ZeroChannels,
        Self::ZeroSampleRate,
        Self::PartialFrame,
        Self::HugeChunk,
    ];

    /// Machine-readable reason id (manifest `truth.corrupt.reason`).
    pub fn reason(self) -> &'static str {
        match self {
            Self::Empty => "empty_file",
            Self::RandomBytes => "random_bytes",
            Self::TextFile => "text_file",
            Self::TruncatedHeader => "truncated_header",
            Self::TruncatedData => "truncated_data",
            Self::RiffSizeTooLarge => "riff_size_too_large",
            Self::RiffSizeTooSmall => "riff_size_too_small",
            Self::FmtChunkTooSmall => "fmt_chunk_too_small",
            Self::UnsupportedFormatTag => "unsupported_format_tag",
            Self::NotWave => "not_wave_form",
            Self::MissingDataChunk => "missing_data_chunk",
            Self::ZeroChannels => "zero_channels",
            Self::ZeroSampleRate => "zero_sample_rate",
            Self::PartialFrame => "partial_frame",
            Self::HugeChunk => "huge_chunk_size",
        }
    }

    /// Required decoder behaviour.
    pub fn expect(self) -> Expect {
        match self {
            // Real audio bytes are present and the header is self-consistent
            // enough that lenient decoders (ignoring RIFF size, dropping the
            // partial frame, stopping at EOF) legitimately return audio.
            Self::TruncatedData
            | Self::RiffSizeTooLarge
            | Self::RiffSizeTooSmall
            | Self::PartialFrame => Expect::ErrorOrPartial,
            _ => Expect::Error,
        }
    }

    /// Human-readable description.
    pub fn description(self) -> &'static str {
        match self {
            Self::Empty => "zero-byte file with .wav extension",
            Self::RandomBytes => "4096 random bytes (seeded) with .wav extension",
            Self::TextFile => "UTF-8 text file with .wav extension",
            Self::TruncatedHeader => "valid WAV cut after 30 bytes, inside the fmt chunk",
            Self::TruncatedData => {
                "data chunk claims 1 s of 16-bit mono but only 0.25 s is present"
            }
            Self::RiffSizeTooLarge => "RIFF size field is 1,000,000 bytes larger than the file",
            Self::RiffSizeTooSmall => "RIFF size field ends after the fmt chunk, before data",
            Self::FmtChunkTooSmall => "fmt chunk is 12 bytes (block align and bit depth missing)",
            Self::UnsupportedFormatTag => "undefined format tag 0x7A7A with PCM-looking data",
            Self::NotWave => "RIFF form type is 'AVI ' instead of 'WAVE'",
            Self::MissingDataChunk => "RIFF/WAVE with fmt chunk but no data chunk",
            Self::ZeroChannels => "fmt chunk declares 0 channels",
            Self::ZeroSampleRate => "fmt chunk declares a 0 Hz sample rate",
            Self::PartialFrame => "stereo 16-bit data chunk ends with half a frame (2 extra bytes)",
            Self::HugeChunk => "LIST chunk claiming 0xFFFFFF00 bytes before the data chunk",
        }
    }

    /// Manifest truth.
    pub fn truth(self) -> CorruptTruth {
        CorruptTruth {
            reason: self.reason().to_owned(),
            expect: self.expect(),
        }
    }

    /// File bytes.
    pub fn build(self, rng: &mut SplitMix64) -> Vec<u8> {
        match self {
            Self::Empty => Vec::new(),
            Self::RandomBytes => {
                let mut b = vec![0u8; 4096];
                rng.fill_bytes(&mut b);
                b
            }
            Self::TextFile => {
                b"This is not audio. Sonar fixture: a text file with a .wav extension.\n".to_vec()
            }
            Self::TruncatedHeader => {
                let mut b = base(1);
                b.truncate(30);
                b
            }
            Self::TruncatedData => {
                let mut b = base(1);
                put_u32(&mut b, OFF_DATA_LEN, 88_200); // 1 s · 44.1 kHz · 2 bytes
                put_u32(&mut b, OFF_RIFF_SIZE, 36 + 88_200);
                b
            }
            Self::RiffSizeTooLarge => {
                let mut b = base(1);
                let real = (b.len() - 8) as u32;
                put_u32(&mut b, OFF_RIFF_SIZE, real + 1_000_000);
                b
            }
            Self::RiffSizeTooSmall => {
                let mut b = base(1);
                put_u32(&mut b, OFF_RIFF_SIZE, 4 + 24);
                b
            }
            Self::FmtChunkTooSmall => {
                let b = base(1);
                let mut out = b[..OFF_TAG + 12].to_vec(); // tag..byte_rate
                put_u32(&mut out, OFF_FMT_LEN, 12);
                out.extend_from_slice(&b[OFF_DATA_ID..]);
                let riff = (out.len() - 8) as u32;
                put_u32(&mut out, OFF_RIFF_SIZE, riff);
                out
            }
            Self::UnsupportedFormatTag => patched(OFF_TAG, &0x7A7Au16.to_le_bytes()),
            Self::NotWave => patched(OFF_FORM, b"AVI "),
            Self::MissingDataChunk => {
                let mut b = base(1);
                b.truncate(OFF_DATA_ID);
                put_u32(&mut b, OFF_RIFF_SIZE, (OFF_DATA_ID - 8) as u32);
                b
            }
            Self::ZeroChannels => patched(OFF_CHANNELS, &0u16.to_le_bytes()),
            Self::ZeroSampleRate => patched(OFF_RATE, &0u32.to_le_bytes()),
            Self::PartialFrame => {
                let mut b = base(2);
                b.extend_from_slice(&[0x34, 0x12]);
                let data_len = (b.len() - HEADER_LEN) as u32;
                let riff = (b.len() - 8) as u32;
                put_u32(&mut b, OFF_DATA_LEN, data_len);
                put_u32(&mut b, OFF_RIFF_SIZE, riff);
                b
            }
            Self::HugeChunk => {
                let b = base(1);
                let mut out = b[..OFF_DATA_ID].to_vec();
                out.extend_from_slice(b"LIST");
                out.extend_from_slice(&0xFFFF_FF00u32.to_le_bytes());
                out.extend_from_slice(b"INFO");
                out.extend_from_slice(&b[OFF_DATA_ID..]);
                let riff = (out.len() - 8) as u32;
                put_u32(&mut out, OFF_RIFF_SIZE, riff);
                out
            }
        }
    }
}

/// Valid 0.25 s, 440 Hz, 44.1 kHz, 16-bit PCM file with a 44-byte header.
fn base(channels: u16) -> Vec<u8> {
    let spec = WavSpec::new(44_100, channels, SampleFormat::Int16);
    let tone = additive(11_025, 44_100, 440.0, &[0.5], |_| 1.0);
    encode(&spec, &Audio::from_mono(tone, channels).interleave()).bytes
}

fn patched(at: usize, bytes: &[u8]) -> Vec<u8> {
    let mut b = base(1);
    b[at..at + bytes.len()].copy_from_slice(bytes);
    b
}

fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn build(k: CorruptKind) -> Vec<u8> {
        k.build(&mut SplitMix64::new(1))
    }

    #[test]
    fn base_file_is_valid_and_canonical() {
        let b = base(1);
        assert_eq!(b.len(), HEADER_LEN + 22_050);
        assert_eq!(&b[OFF_DATA_ID..OFF_DATA_ID + 4], b"data");
        assert!(hound::WavReader::new(Cursor::new(b)).is_ok());
    }

    #[test]
    fn hound_rejects_every_must_error_file() {
        for k in CorruptKind::ALL {
            let bytes = build(k);
            let parsed = hound::WavReader::new(Cursor::new(bytes))
                .and_then(|mut r| r.samples::<i32>().collect::<Result<Vec<_>, _>>());
            if k.expect() == Expect::Error {
                assert!(parsed.is_err(), "{k:?} parsed by hound");
            }
        }
    }

    #[test]
    fn reasons_are_unique_and_snake_case() {
        let mut seen = std::collections::BTreeSet::new();
        for k in CorruptKind::ALL {
            assert!(seen.insert(k.reason()), "duplicate {}", k.reason());
            assert!(k
                .reason()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }

    #[test]
    fn specific_damage() {
        assert!(build(CorruptKind::Empty).is_empty());
        assert_eq!(build(CorruptKind::RandomBytes).len(), 4096);
        assert_eq!(build(CorruptKind::TruncatedHeader).len(), 30);
        let pf = build(CorruptKind::PartialFrame);
        let data_len = u32::from_le_bytes([pf[40], pf[41], pf[42], pf[43]]);
        assert_eq!(data_len % 4, 2);
        let fmt = build(CorruptKind::FmtChunkTooSmall);
        assert_eq!(&fmt[32..36], b"data");
        let riff = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
        assert_eq!(riff as usize, fmt.len() - 8);
    }
}

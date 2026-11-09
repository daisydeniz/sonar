//! Minimal RIFF/WAVE writer for fixtures.
//!
//! Hand-written (rather than `hound`) so the corrupt-file builder can reuse
//! the exact header layout and patch individual fields. Layout follows the
//! Microsoft RIFF/WAVE spec: `RIFF` size `WAVE`, then `fmt ` (16 bytes for
//! PCM, 18 for IEEE float, 40 for `WAVE_FORMAT_EXTENSIBLE`), a `fact` chunk
//! for non-PCM data, and `data` (padded to an even length).
//!
//! Integer quantisation: `q = round(x · (2^(b−1) − 1))`, clamped to the
//! b-bit range; levels are then measured as `q / 2^(b−1)` (the convention
//! most decoders, e.g. symphonia, use when converting to float).

/// `WAVE_FORMAT_PCM`.
pub const TAG_PCM: u16 = 0x0001;
/// `WAVE_FORMAT_IEEE_FLOAT`.
pub const TAG_IEEE_FLOAT: u16 = 0x0003;
/// `WAVE_FORMAT_EXTENSIBLE`.
pub const TAG_EXTENSIBLE: u16 = 0xFFFE;

/// Tail shared by the `KSDATAFORMAT_SUBTYPE_*` GUIDs (bytes 2..16).
const GUID_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// Sample encodings the fixture set covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// 16-bit signed integer PCM.
    Int16,
    /// 24-bit signed integer PCM (3 bytes per sample).
    Int24,
    /// 32-bit IEEE float.
    Float32,
}

impl SampleFormat {
    /// Bits per sample as written in the header.
    pub fn bits(self) -> u16 {
        match self {
            Self::Int16 => 16,
            Self::Int24 => 24,
            Self::Float32 => 32,
        }
    }

    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        usize::from(self.bits() / 8)
    }

    /// `true` for IEEE float.
    pub fn is_float(self) -> bool {
        matches!(self, Self::Float32)
    }
}

/// Everything that determines a WAV header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavSpec {
    /// Frames per second.
    pub sample_rate: u32,
    /// Interleaved channel count (1 or 2 in the fixture set).
    pub channels: u16,
    /// Sample encoding.
    pub sample_format: SampleFormat,
    /// Write `WAVE_FORMAT_EXTENSIBLE` instead of the plain format tag.
    pub extensible: bool,
}

impl WavSpec {
    /// Plain (non-extensible) spec.
    pub const fn new(sample_rate: u32, channels: u16, sample_format: SampleFormat) -> Self {
        Self {
            sample_rate,
            channels,
            sample_format,
            extensible: false,
        }
    }

    /// Same spec written with `WAVE_FORMAT_EXTENSIBLE`.
    pub const fn with_extensible(mut self) -> Self {
        self.extensible = true;
        self
    }

    /// Bytes per interleaved frame.
    pub fn block_align(&self) -> u16 {
        self.channels * self.sample_format.bits() / 8
    }

    /// Format tag written in the `fmt ` chunk.
    pub fn format_tag(&self) -> u16 {
        if self.extensible {
            TAG_EXTENSIBLE
        } else if self.sample_format.is_float() {
            TAG_IEEE_FLOAT
        } else {
            TAG_PCM
        }
    }
}

/// Peak and RMS of the quantised samples, linear full scale (1.0 = 0 dBFS).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Levels {
    /// Largest absolute sample over all channels.
    pub peak: f64,
    /// Root mean square over all samples of all channels.
    pub rms: f64,
}

impl Levels {
    /// `true` when every sample is exactly zero (digital silence).
    pub fn is_silent(&self) -> bool {
        self.peak == 0.0
    }

    /// Peak in dBFS (`20·log10(peak)`), `None` for digital silence.
    pub fn peak_dbfs(&self) -> Option<f64> {
        to_dbfs(self.peak)
    }

    /// RMS in dBFS (`20·log10(rms)`, a full-scale sine reads −3.01 dBFS),
    /// `None` for digital silence.
    pub fn rms_dbfs(&self) -> Option<f64> {
        to_dbfs(self.rms)
    }
}

fn to_dbfs(x: f64) -> Option<f64> {
    (x > 0.0).then(|| 20.0 * x.log10())
}

/// A complete WAV file in memory plus what was measured while encoding.
#[derive(Debug, Clone)]
pub struct Encoded {
    /// File bytes.
    pub bytes: Vec<u8>,
    /// Levels of the samples as written.
    pub levels: Levels,
    /// Frames written.
    pub num_frames: u64,
}

/// Encodes interleaved `samples` (nominal range −1..1) as a WAV file.
/// A trailing partial frame is dropped; non-finite samples become 0.
pub fn encode(spec: &WavSpec, samples: &[f32]) -> Encoded {
    let ch = usize::from(spec.channels.max(1));
    let num_frames = samples.len() / ch;
    let samples = &samples[..num_frames * ch];
    let data_len = samples.len() * spec.sample_format.bytes();

    let mut bytes = Vec::with_capacity(data_len + 80);
    write_header(&mut bytes, spec, num_frames as u64, data_len as u64);
    let (mut peak, mut sum_sq) = (0.0f64, 0.0f64);
    for &x in samples {
        let x = if x.is_finite() { x } else { 0.0 };
        let level = match spec.sample_format {
            SampleFormat::Int16 => {
                let q = quantize(x, 16);
                bytes.extend_from_slice(&(q as i16).to_le_bytes());
                f64::from(q) / 32_768.0
            }
            SampleFormat::Int24 => {
                let q = quantize(x, 24);
                bytes.extend_from_slice(&q.to_le_bytes()[..3]);
                f64::from(q) / 8_388_608.0
            }
            SampleFormat::Float32 => {
                bytes.extend_from_slice(&x.to_le_bytes());
                f64::from(x)
            }
        };
        peak = peak.max(level.abs());
        sum_sq += level * level;
    }
    if data_len % 2 == 1 {
        bytes.push(0); // RIFF pad byte
    }
    let rms = if samples.is_empty() {
        0.0
    } else {
        (sum_sq / samples.len() as f64).sqrt()
    };
    Encoded {
        bytes,
        levels: Levels { peak, rms },
        num_frames: num_frames as u64,
    }
}

/// `round(x · (2^(bits−1) − 1))` clamped to the signed `bits` range.
fn quantize(x: f32, bits: u32) -> i32 {
    let max = f64::from((1i32 << (bits - 1)) - 1);
    (f64::from(x) * max).round().clamp(-max - 1.0, max) as i32
}

/// Appends a RIFF/WAVE header for `num_frames` frames whose `data` chunk is
/// `data_len` bytes. Sizes beyond 4 GiB saturate (never reached by fixtures).
pub fn write_header(out: &mut Vec<u8>, spec: &WavSpec, num_frames: u64, data_len: u64) {
    let fmt = fmt_body(spec);
    let has_fact = spec.sample_format.is_float();
    let pad = data_len % 2;
    let riff_len = 4 + (8 + fmt.len() as u64) + if has_fact { 12 } else { 0 } + 8 + data_len + pad;

    out.extend_from_slice(b"RIFF");
    put_u32(out, riff_len);
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    put_u32(out, fmt.len() as u64);
    out.extend_from_slice(&fmt);
    if has_fact {
        out.extend_from_slice(b"fact");
        put_u32(out, 4);
        put_u32(out, num_frames);
    }
    out.extend_from_slice(b"data");
    put_u32(out, data_len);
}

fn put_u32(out: &mut Vec<u8>, v: u64) {
    let v = u32::try_from(v).unwrap_or(u32::MAX);
    out.extend_from_slice(&v.to_le_bytes());
}

/// Body of the `fmt ` chunk (without id/size).
pub fn fmt_body(spec: &WavSpec) -> Vec<u8> {
    let bits = spec.sample_format.bits();
    let block_align = spec.block_align();
    let mut b = Vec::with_capacity(40);
    b.extend_from_slice(&spec.format_tag().to_le_bytes());
    b.extend_from_slice(&spec.channels.to_le_bytes());
    b.extend_from_slice(&spec.sample_rate.to_le_bytes());
    let byte_rate = spec.sample_rate.saturating_mul(u32::from(block_align));
    b.extend_from_slice(&byte_rate.to_le_bytes());
    b.extend_from_slice(&block_align.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    if spec.extensible {
        b.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        b.extend_from_slice(&bits.to_le_bytes()); // wValidBitsPerSample
        let mask: u32 = if spec.channels == 1 { 0x4 } else { 0x3 }; // FC / FL|FR
        b.extend_from_slice(&mask.to_le_bytes());
        let sub = if spec.sample_format.is_float() {
            TAG_IEEE_FLOAT
        } else {
            TAG_PCM
        };
        b.extend_from_slice(&sub.to_le_bytes());
        b.extend_from_slice(&GUID_TAIL);
    } else if spec.sample_format.is_float() {
        b.extend_from_slice(&0u16.to_le_bytes()); // cbSize
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const ALL_FORMATS: [SampleFormat; 3] = [
        SampleFormat::Int16,
        SampleFormat::Int24,
        SampleFormat::Float32,
    ];

    fn ramp(frames: usize, ch: u16) -> Vec<f32> {
        (0..frames * usize::from(ch))
            .map(|i| (i as f32 * 0.01).sin() * 0.8)
            .collect()
    }

    #[test]
    fn pcm16_mono_header_bytes() {
        let spec = WavSpec::new(44_100, 1, SampleFormat::Int16);
        let enc = encode(&spec, &[0.0, 1.0, -1.0]);
        let b = &enc.bytes;
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([b[4], b[5], b[6], b[7]]), 36 + 6);
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!(&b[16..20], &16u32.to_le_bytes());
        assert_eq!(&b[20..22], &1u16.to_le_bytes()); // PCM
        assert_eq!(&b[22..24], &1u16.to_le_bytes()); // mono
        assert_eq!(&b[24..28], &44_100u32.to_le_bytes());
        assert_eq!(&b[28..32], &88_200u32.to_le_bytes());
        assert_eq!(&b[32..34], &2u16.to_le_bytes());
        assert_eq!(&b[34..36], &16u16.to_le_bytes());
        assert_eq!(&b[36..44], b"data\x06\x00\x00\x00");
        assert_eq!(&b[44..], &[0, 0, 0xFF, 0x7F, 0x01, 0x80]);
        assert_eq!(b.len(), 50);
    }

    #[test]
    fn every_format_reads_back_with_hound() {
        for fmt in ALL_FORMATS {
            for ch in [1u16, 2] {
                for ext in [false, true] {
                    let mut spec = WavSpec::new(48_000, ch, fmt);
                    if ext {
                        spec = spec.with_extensible();
                    }
                    let input = ramp(101, ch);
                    let enc = encode(&spec, &input);
                    let mut r = hound::WavReader::new(Cursor::new(enc.bytes.clone()))
                        .unwrap_or_else(|e| panic!("{spec:?}: {e}"));
                    let hs = r.spec();
                    assert_eq!(hs.channels, ch);
                    assert_eq!(hs.sample_rate, 48_000);
                    assert_eq!(hs.bits_per_sample, fmt.bits());
                    assert_eq!(r.duration(), 101);
                    let back: Vec<f64> = match fmt {
                        SampleFormat::Float32 => {
                            r.samples::<f32>().map(|s| f64::from(s.unwrap())).collect()
                        }
                        _ => {
                            let scale = f64::from(1u32 << (fmt.bits() - 1));
                            r.samples::<i32>()
                                .map(|s| f64::from(s.unwrap()) / scale)
                                .collect()
                        }
                    };
                    assert_eq!(back.len(), input.len());
                    let tol = if fmt == SampleFormat::Int16 {
                        1e-4
                    } else {
                        1e-6
                    };
                    for (a, b) in input.iter().zip(&back) {
                        assert!((f64::from(*a) - b).abs() < tol, "{spec:?}: {a} vs {b}");
                    }
                    let peak = back.iter().fold(0.0f64, |m, x| m.max(x.abs()));
                    assert_eq!(enc.levels.peak, peak, "{spec:?}");
                }
            }
        }
    }

    #[test]
    fn odd_data_length_is_padded() {
        let spec = WavSpec::new(44_100, 1, SampleFormat::Int24);
        let enc = encode(&spec, &[0.5]);
        assert_eq!(enc.bytes.len(), 44 + 3 + 1);
        let riff = u32::from_le_bytes([enc.bytes[4], enc.bytes[5], enc.bytes[6], enc.bytes[7]]);
        assert_eq!(riff as usize, enc.bytes.len() - 8);
        assert!(hound::WavReader::new(Cursor::new(enc.bytes)).is_ok());
    }

    #[test]
    fn levels_of_full_scale_and_silence() {
        let spec = WavSpec::new(44_100, 1, SampleFormat::Float32);
        let sine: Vec<f32> = (0..44_100)
            .map(|i| (std::f64::consts::TAU * 441.0 * f64::from(i) / 44_100.0).sin() as f32)
            .collect();
        let lv = encode(&spec, &sine).levels;
        assert!(lv.peak_dbfs().is_some_and(|p| p.abs() < 1e-6));
        assert!(lv.rms_dbfs().is_some_and(|r| (r + 3.0103).abs() < 1e-3));
        let silent = encode(&spec, &[0.0; 64]).levels;
        assert!(silent.is_silent());
        assert_eq!(silent.peak_dbfs(), None);
        let empty = encode(&spec, &[]);
        assert_eq!(empty.num_frames, 0);
        assert!(empty.levels.is_silent());
    }

    #[test]
    fn quantize_clamps_and_rounds() {
        assert_eq!(quantize(2.0, 16), 32_767);
        assert_eq!(quantize(-2.0, 16), -32_768);
        assert_eq!(quantize(0.5, 24), 4_194_304);
        assert_eq!(quantize(0.0, 24), 0);
    }

    #[test]
    fn partial_frames_and_nan_are_tolerated() {
        let spec = WavSpec::new(44_100, 2, SampleFormat::Int16);
        let enc = encode(&spec, &[f32::NAN, 0.25, 0.5]);
        assert_eq!(enc.num_frames, 1);
        assert_eq!(&enc.bytes[44..46], &[0, 0]);
    }
}

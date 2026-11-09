//! Minimal MP3 writer: digital silence as MPEG audio Layer III frames with
//! an exact gapless length.
//!
//! Frames follow ISO/IEC 11172-3 §2.4.1 (MPEG-1: 32/44.1/48 kHz, 1,152
//! samples per frame) and ISO/IEC 13818-3 (MPEG-2 "LSF": 16/22.05/24 kHz,
//! 576 samples). Every frame is 64 kbit/s, no CRC, padding bit 0, and has
//! all-zero side information: `main_data_begin` = 0 and `part2_3_length` =
//! 0 in every granule, so no scale factors or Huffman data are coded and
//! the decoded spectrum — hence the output — is exactly zero.
//!
//! The first frame is a Xing "Info" tag (CBR) with frame count, byte count,
//! a linear TOC and quality, followed by a LAME tag (Gabriel Bouvigne, "Mp3
//! Info Tag rev 1 specifications"): encoder delay [`ENCODER_DELAY`] and the
//! padding, as 12-bit fields, music length/CRC and the tag CRC-16 over the
//! frame's first bytes. The frame count excludes the tag frame. A gapless
//! decoder discards `ENCODER_DELAY + 529` (the Layer III decoder delay)
//! frames at the start and `padding − 529` at the end, so
//! `audio_frames · samples_per_frame − delay − padding` = the requested
//! length exactly. The encoder string is `LAME3.100`: readers (symphonia,
//! FFmpeg) only honour the delay/padding fields when it starts with
//! `LAME`, so it serves as the tag-layout marker.

use super::bits::{crc16_arc, BitWriter};

/// Encoder delay written to the LAME tag (frames; LAME's usual value).
pub const ENCODER_DELAY: u64 = 576;
/// Decoder delay of the Layer III synthesis filterbank (frames) that LAME
/// tag readers add to the encoder delay and subtract from the padding.
pub const DECODER_DELAY: u64 = 529;
/// Bit rate of every frame (kbit/s).
const KBPS: u32 = 64;
/// LAME tag version string (9 bytes).
const ENCODER: &[u8; 9] = b"LAME3.100";

/// Frame geometry for one sample rate / channel count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    /// MPEG-1 (else MPEG-2 LSF).
    mpeg1: bool,
    /// Sampling-frequency index (2 bits).
    rate_index: u64,
    /// Bit-rate index (4 bits) of [`KBPS`].
    bitrate_index: u64,
    /// Bytes per frame including the 4-byte header.
    frame_bytes: usize,
    /// Samples per channel per frame.
    frame_len: u64,
    /// Side-information bytes after the header.
    side_info: usize,
    /// Mono (else stereo).
    mono: bool,
}

fn layout(sample_rate: u32, channels: u16) -> Option<Layout> {
    let (mpeg1, rate_index) = match sample_rate {
        44_100 => (true, 0),
        48_000 => (true, 1),
        32_000 => (true, 2),
        22_050 => (false, 0),
        24_000 => (false, 1),
        16_000 => (false, 2),
        _ => return None,
    };
    let mono = match channels {
        1 => true,
        2 => false,
        _ => return None,
    };
    // Layer III frame size: 144·bitrate/rate (MPEG-1), 72·bitrate/rate (LSF).
    let coef = if mpeg1 { 144 } else { 72 };
    let frame_bytes = (coef * KBPS * 1_000 / sample_rate) as usize;
    let side_info = match (mpeg1, mono) {
        (true, true) => 17,
        (true, false) => 32,
        (false, true) => 9,
        (false, false) => 17,
    };
    Some(Layout {
        mpeg1,
        rate_index,
        bitrate_index: if mpeg1 { 5 } else { 8 },
        frame_bytes,
        frame_len: if mpeg1 { 1_152 } else { 576 },
        side_info,
        mono,
    })
}

/// The 4-byte frame header (§2.4.1.3).
fn header(l: &Layout) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.put(0x7FF, 11); // sync
    w.put(if l.mpeg1 { 0b11 } else { 0b10 }, 2); // version
    w.put(0b01, 2); // Layer III
    w.put(1, 1); // protection_bit: no CRC
    w.put(l.bitrate_index, 4);
    w.put(l.rate_index, 2);
    w.put(0, 1); // padding
    w.put(0, 1); // private
    w.put(if l.mono { 0b11 } else { 0b00 }, 2); // mode
    w.put(0, 2); // mode extension
    w.put(0, 1); // copyright
    w.put(1, 1); // original
    w.put(0, 2); // emphasis
    w.into_bytes()
}

/// Encodes `frames` frames of digital silence. `None` for rates other than
/// 16/22.05/24/32/44.1/48 kHz or channel counts other than 1 and 2.
pub fn encode_silence(sample_rate: u32, channels: u16, frames: u64) -> Option<Vec<u8>> {
    let l = layout(sample_rate, channels)?;
    // Enough frames to cover delay + content + at least the decoder delay
    // of padding (readers subtract 529 from the padding field).
    let needed = frames + ENCODER_DELAY + DECODER_DELAY;
    let audio_frames = needed.div_ceil(l.frame_len);
    let padding = audio_frames * l.frame_len - ENCODER_DELAY - frames;

    let mut frame = header(&l);
    frame.resize(l.frame_bytes, 0);
    let audio: Vec<u8> = frame.repeat(usize::try_from(audio_frames).ok()?);
    let total_bytes = u32::try_from(audio.len() + l.frame_bytes).ok()?;
    let mut out = info_frame(&l, audio_frames, total_bytes, padding, crc16_arc(&audio))?;
    out.extend_from_slice(&audio);
    Some(out)
}

/// The Xing "Info" + LAME tag frame.
fn info_frame(
    l: &Layout,
    audio_frames: u64,
    total_bytes: u32,
    padding: u64,
    music_crc: u16,
) -> Option<Vec<u8>> {
    let mut f = header(l);
    f.resize(4 + l.side_info, 0); // zero side info marks a tag frame
    f.extend_from_slice(b"Info");
    f.extend_from_slice(&0x0Fu32.to_be_bytes()); // frames, bytes, TOC, quality
    f.extend_from_slice(&u32::try_from(audio_frames).ok()?.to_be_bytes());
    f.extend_from_slice(&total_bytes.to_be_bytes());
    f.extend((0..100u32).map(|i| (i * 256 / 100) as u8)); // linear TOC (CBR)
    f.extend_from_slice(&0u32.to_be_bytes()); // quality

    // LAME tag (36 bytes).
    f.extend_from_slice(ENCODER);
    f.push(0x01); // tag revision 0, VBR method 1 = CBR
    f.push(0); // lowpass: unknown
    f.extend_from_slice(&[0; 4]); // peak amplitude: unknown
    f.extend_from_slice(&[0; 4]); // radio / audiophile replay gain: none
    f.push(0); // encoding flags, ATH type
    f.push(KBPS as u8); // CBR bit rate
    if ENCODER_DELAY >= 1 << 12 || padding >= 1 << 12 {
        return None;
    }
    let trim = (ENCODER_DELAY << 12) | padding;
    f.extend_from_slice(&trim.to_be_bytes()[5..]);
    f.push(0); // misc: noise shaping, stereo mode, source rate
    f.push(0); // MP3 gain
    f.extend_from_slice(&[0; 2]); // preset, surround
    f.extend_from_slice(&total_bytes.to_be_bytes()); // music length
    f.extend_from_slice(&music_crc.to_be_bytes());
    let tag_crc = crc16_arc(&f);
    f.extend_from_slice(&tag_crc.to_be_bytes());
    if f.len() > l.frame_bytes {
        return None;
    }
    f.resize(l.frame_bytes, 0);
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_match_known_values() {
        // MPEG-1 L3, 64 kbit/s, 44.1 kHz, stereo, original.
        let l = layout(44_100, 2).unwrap();
        assert_eq!(header(&l), [0xFF, 0xFB, 0x50, 0x04]);
        assert_eq!((l.frame_bytes, l.frame_len), (208, 1_152));
        // MPEG-2 L3, 64 kbit/s, 22.05 kHz, mono.
        let l = layout(22_050, 1).unwrap();
        assert_eq!(header(&l), [0xFF, 0xF3, 0x80, 0xC4]);
        assert_eq!((l.frame_bytes, l.frame_len, l.side_info), (208, 576, 9));
        assert_eq!(layout(48_000, 2).unwrap().frame_bytes, 192);
        assert!(layout(8_000, 1).is_none() && layout(44_100, 3).is_none());
    }

    #[test]
    fn lame_tag_round_trips_length() {
        let frames = 44_100;
        let b = encode_silence(44_100, 2, frames).unwrap();
        let l = layout(44_100, 2).unwrap();
        assert_eq!(b.len() % l.frame_bytes, 0);
        let tag = 4 + l.side_info;
        assert_eq!(&b[tag..tag + 4], b"Info");
        let n = u64::from(u32::from_be_bytes([
            b[tag + 8],
            b[tag + 9],
            b[tag + 10],
            b[tag + 11],
        ]));
        assert_eq!(n as usize + 1, b.len() / l.frame_bytes);
        let lame = tag + 120;
        assert_eq!(&b[lame..lame + 9], ENCODER);
        let t = &b[lame + 21..lame + 24];
        let delay = (u64::from(t[0]) << 4) | u64::from(t[1] >> 4);
        let padding = (u64::from(t[1] & 0x0F) << 8) | u64::from(t[2]);
        assert_eq!(delay, ENCODER_DELAY);
        assert!(padding >= DECODER_DELAY);
        assert_eq!(n * l.frame_len - delay - padding, frames);
        // Tag CRC over everything before it.
        let crc = u16::from_be_bytes([b[lame + 34], b[lame + 35]]);
        assert_eq!(crc, crc16_arc(&b[..lame + 34]));
        // Audio frames: header + zeros.
        let second = &b[l.frame_bytes..2 * l.frame_bytes];
        assert_eq!(&second[..4], &header(&l)[..]);
        assert!(second[4..].iter().all(|&x| x == 0));
    }

    #[test]
    fn rejects_unsupported_parameters() {
        assert!(encode_silence(11_025, 1, 100).is_none());
        assert!(encode_silence(44_100, 0, 100).is_none());
    }
}

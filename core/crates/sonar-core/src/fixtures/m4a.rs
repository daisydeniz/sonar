//! Minimal M4A writer: Apple Lossless (ALAC) in an ISO base media file
//! (ISO/IEC 14496-12).
//!
//! ALAC frames follow Apple's ALAC format (open-sourced reference,
//! `ALACBitUtilities`/`ALACEncoder`, Apache-2.0; nothing is copied from it):
//! one element per frame — SCE (mono) or CPE (stereo) — written as an
//! *escape* (uncompressed) element: tag, instance, 12 unused bits,
//! partial-frame flag (+ 32-bit sample count), shift 0, escape flag 1, then
//! interleaved `bit_depth`-bit samples, then the END tag. Escape frames are
//! valid ALAC (encoders emit them for incompressible audio) and decode
//! bit-exactly. The 24-byte `ALACSpecificConfig` ("magic cookie") sits in
//! an `alac` box inside the `alac` sample entry.
//!
//! The box layout is in [`mp4box`](super::mp4box). Media timescale = sample
//! rate, so `stts` durations are frames and their sum is the exact length;
//! no edit list (ALAC has no priming).

use super::bits::BitWriter;
use super::mp4box::{audio_stsd, file, full_box, stts, Track};
use super::pcm::Pcm;

/// ALAC frame length (samples per channel) of every frame but the last.
pub const FRAME_LENGTH: usize = 4096;

/// ALAC element tags.
const ID_SCE: u64 = 0;
const ID_CPE: u64 = 1;
const ID_END: u64 = 7;

/// Encodes `pcm` (1 or 2 channels, 16 or 24 bits) as an `.m4a` file.
pub fn encode(pcm: &Pcm) -> Vec<u8> {
    let ch = usize::from(pcm.channels.max(1));
    let frames = pcm.frames();
    let packets: Vec<Vec<u8>> = (0..frames)
        .step_by(FRAME_LENGTH)
        .map(|start| {
            let len = (frames - start).min(FRAME_LENGTH);
            alac_frame(pcm, &pcm.samples[start * ch..(start + len) * ch], len)
        })
        .collect();

    let frames32 = u32::try_from(frames).unwrap_or(u32::MAX);
    let total_bytes: usize = packets.iter().map(Vec::len).sum();
    let max_frame = packets.iter().map(Vec::len).max().unwrap_or(0) as u32;
    let avg_bit_rate = if frames == 0 {
        0
    } else {
        (total_bytes as u64 * 8 * u64::from(pcm.sample_rate) / frames as u64) as u32
    };
    let cookie = full_box(b"alac", 0, &magic_cookie(pcm, max_frame, avg_bit_rate));
    let last = frames.saturating_sub(packets.len().saturating_sub(1) * FRAME_LENGTH);
    let track = Track {
        sample_rate: pcm.sample_rate,
        media_frames: frames32,
        movie_frames: frames32,
        stsd: audio_stsd(b"alac", pcm.channels, pcm.bits, pcm.sample_rate, &cookie),
        stts: stts(packets.len(), FRAME_LENGTH as u32, last as u32),
        edts: Vec::new(),
        udta: Vec::new(),
    };
    file(&packets, &track)
}

/// One escape-coded ALAC frame of `len` frames (interleaved `samples`).
fn alac_frame(pcm: &Pcm, samples: &[i32], len: usize) -> Vec<u8> {
    let bits = u32::from(pcm.bits);
    let mut w = BitWriter::new();
    w.put(if pcm.channels == 1 { ID_SCE } else { ID_CPE }, 3);
    w.put(0, 4); // element instance tag
    w.put(0, 12); // unused, must be 0
    let partial = len != FRAME_LENGTH;
    w.put(u64::from(partial), 1);
    w.put(0, 2); // bytes shifted
    w.put(1, 1); // escape: uncompressed samples
    if partial {
        w.put(len as u64, 32);
    }
    for &s in samples {
        w.put_signed(i64::from(s), bits);
    }
    w.put(ID_END, 3);
    w.into_bytes()
}

/// `ALACSpecificConfig` (24 bytes, big-endian).
fn magic_cookie(pcm: &Pcm, max_frame_bytes: u32, avg_bit_rate: u32) -> Vec<u8> {
    let mut c = Vec::with_capacity(24);
    c.extend_from_slice(&(FRAME_LENGTH as u32).to_be_bytes());
    c.push(0); // compatible version
    c.push(pcm.bits as u8);
    c.extend_from_slice(&[40, 10, 14]); // pb, mb, kb (Apple defaults)
    c.push(pcm.channels as u8);
    c.extend_from_slice(&255u16.to_be_bytes()); // max run
    c.extend_from_slice(&max_frame_bytes.to_be_bytes());
    c.extend_from_slice(&avg_bit_rate.to_be_bytes());
    c.extend_from_slice(&pcm.sample_rate.to_be_bytes());
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(channels: u16, bits: u16, frames: usize) -> Pcm {
        let samples = (0..frames * usize::from(channels))
            .map(|i| (i as i32 % 200) - 100)
            .collect();
        Pcm {
            sample_rate: 44_100,
            channels,
            bits,
            samples,
        }
    }

    #[test]
    fn escape_frame_layout() {
        let p = pcm(2, 16, 2);
        let f = alac_frame(&p, &p.samples, 2);
        // 23-bit header: CPE(001) inst(0000) unused(12×0) partial(1)
        // shift(00) escape(1), then the 32-bit count from bit 23:
        // 0010 0000 | 0000 0000 | 0001 001c | ccc…
        assert_eq!(&f[..3], &[0x20, 0x00, 0x12]);
        assert_eq!(u32::from_be_bytes([f[3], f[4], f[5], f[6]]) >> 1, 2);
        // 23 + 32 + 4 × 16 sample bits + END (3) = 122 bits → 16 bytes.
        assert_eq!(f.len(), 16);
        // A full frame has no count field: samples follow bit 23.
        let p = pcm(1, 24, FRAME_LENGTH);
        let f = alac_frame(&p, &p.samples, FRAME_LENGTH);
        assert_eq!((f[0], f[1], f[2] & 0xFE), (0x00, 0x00, 0x02));
        assert_eq!(f.len(), (23 + FRAME_LENGTH * 24 + 3).div_ceil(8));
    }

    #[test]
    fn file_starts_with_ftyp_then_mdat_and_has_a_moov() {
        let b = encode(&pcm(1, 16, 10));
        assert_eq!(&b[4..8], b"ftyp");
        let ftyp_len = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        assert_eq!(&b[ftyp_len + 4..ftyp_len + 8], b"mdat");
        assert!(b.windows(4).any(|w| w == b"moov"));
    }
}

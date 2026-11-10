//! Header length for the length-only probe (the job pipeline's meta stage,
//! ADR-015): the exact frame count of an uncompressed PCM WAVE or AIFF file
//! taken from its container header, **without reading the audio payload**.
//!
//! Counting packets ([`super::stream`]) reads every byte of a PCM file to
//! learn what its header already states; on a cold library that read is
//! the whole cost of the meta stage. This module only answers "is the
//! declared length provably what counting would find?"; whenever the answer
//! is not a clear yes, the caller counts packets exactly as before.
//!
//! The header length is used only when **all** of these hold:
//! 1. the caller is a length-only probe: zero analysis window and zero
//!    meter window (decided in [`super::Decoder`]);
//! 2. the container is WAVE or AIFF/AIFF-C and there is no gapless trim;
//! 3. the codec is uncompressed PCM with one frame per block (integer
//!    8/16/24/32-bit, IEEE float 32/64, A-law, µ-law: [`sample_bytes`]).
//!    ADPCM, compressed AIFF-C and every other codec are counted;
//! 4. the demuxer declared a length `N`;
//! 5. the chunk header right before the payload, re-read from the file,
//!    agrees with it: WAVE `data` + `ckSize` (Microsoft/IBM *Multimedia
//!    Programming Interface and Data Specifications 1.0*, 1991, RIFF
//!    chunks and the WAVE `data` chunk; `ckSize` is the payload byte count,
//!    0xFFFFFFFF = unknown length of a streamed file, never trusted — this
//!    is also the RF64 placeholder); AIFF `SSND` with `offset` = 0 and
//!    `blockSize` = 0 (Apple *Audio Interchange File Format* 1.3, 1989, and
//!    *AIFF-C* draft, 1991: payload = `ckDataSize` − 8 − `offset`; a
//!    non-zero offset is rare and counted);
//! 6. the payload is a whole number of frames (frame = sample bytes ×
//!    channels, the size the demuxer reads; the header's `nBlockAlign` is
//!    used by neither path) and payload / frame == `N`;
//! 7. data start + payload ≤ file length: every declared byte is present.
//!
//! Why the result equals the counted one: symphonia's RIFF demuxer (0.6.1,
//! `symphonia-format-riff` `common::next_packet`) emits packets of whole
//! frames from the data start up to data start + payload and reads each
//! packet's bytes. With (7) every read succeeds, so counting yields exactly
//! payload / frame = `N` frames and no `partial`. A truncated file or an
//! oversized declared chunk fails (7) and is counted (readable length,
//! `partial`, problem text — unchanged). A zero-window probe never decodes
//! packets, so for PCM the payload's content cannot change the outcome.
//! `header_tests` checks header == counted on every fixture.
//!
//! Not on this path, by design: FLAC (RFC 9639 STREAMINFO "total samples"
//! is exact when non-zero, but the payload is compressed and only reading
//! the frames reveals a truncated or damaged stream, which counting reports
//! as `partial`), MP4/ALAC/AAC (sample tables: same reason), MP3 and Ogg
//! (no exact header length).

use std::io::{Seek, SeekFrom};

use symphonia::core::codecs::audio::well_known as codec_ids;
use symphonia::core::codecs::audio::AudioCodecId;
use symphonia::core::formats::well_known::{FORMAT_ID_AIFF, FORMAT_ID_WAVE};
use symphonia::core::io::{MediaSource, MediaSourceStream, ReadBytes, SeekBuffered};

use super::source::Opened;
use super::stream::MAX_PACKETS;

/// Frames per packet of symphonia's RIFF demuxer for PCM (0.6.1
/// `common::MAX_FRAMES_PER_PACKET`); only used to keep the header path
/// within the counting path's [`MAX_PACKETS`] limit.
const RIFF_PACKET_FRAMES: u64 = 1_152;

/// Bytes per frame when `src` is eligible for the header length (rules 2–4
/// above), `None` when its length must be counted. Does not read the file.
pub(super) fn pcm_frame_bytes(src: &Opened) -> Option<u64> {
    let format = src.format.format_info().format;
    if format != FORMAT_ID_WAVE && format != FORMAT_ID_AIFF {
        return None;
    }
    if src.gapless.is_some() || src.declared_frames.is_none() {
        return None;
    }
    let channels = u64::try_from(src.channels).ok()?;
    sample_bytes(src.codec_id)?.checked_mul(channels)
}

/// The declared length of `src` if the payload chunk header confirms it
/// and the file holds every declared byte (rules 5–7). Consumes the
/// demuxer (only its byte stream is needed): on `None` the caller must
/// reopen the file and count packets. Reads at most 16 header bytes, which
/// the demuxer has normally just buffered; never reads the payload.
pub(super) fn verified_frames(src: Opened, frame_bytes: u64) -> Option<u64> {
    let declared = src.declared_frames?;
    let format = src.format.format_info().format;
    let mut mss = src.format.into_inner();
    // The demuxer stands at the first packet, i.e. the payload start.
    let data_start = mss.pos();
    let file_len = mss.byte_len()?;
    let payload = if format == FORMAT_ID_WAVE {
        let mut h = [0u8; 8];
        read_at(&mut mss, data_start.checked_sub(8)?, &mut h)?;
        wave_payload(&h)?
    } else if format == FORMAT_ID_AIFF {
        let mut h = [0u8; 16];
        read_at(&mut mss, data_start.checked_sub(16)?, &mut h)?;
        aiff_payload(&h)?
    } else {
        return None;
    };
    exact_frames(declared, payload, frame_bytes, data_start, file_len)
}

/// Rules 6–7 on plain numbers.
fn exact_frames(
    declared: u64,
    payload: u64,
    frame_bytes: u64,
    data_start: u64,
    file_len: u64,
) -> Option<u64> {
    if frame_bytes == 0 || !payload.is_multiple_of(frame_bytes) {
        return None;
    }
    let frames = payload / frame_bytes;
    let present = data_start.checked_add(payload)? <= file_len;
    // Counting stops after MAX_PACKETS reads (unreachable for 32-bit chunk
    // sizes; kept so both paths share every limit).
    let within_limit = frames.div_ceil(RIFF_PACKET_FRAMES) < MAX_PACKETS;
    (frames == declared && present && within_limit).then_some(frames)
}

/// Payload bytes from a WAVE `data` chunk header (`"data"`, `ckSize` LE).
fn wave_payload(h: &[u8; 8]) -> Option<u64> {
    if &h[..4] != b"data" {
        return None;
    }
    let size = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
    (size != u32::MAX).then_some(u64::from(size))
}

/// Payload bytes from an AIFF `SSND` chunk header (`"SSND"`, `ckDataSize`,
/// `offset`, `blockSize`, all BE); only `offset` = `blockSize` = 0.
fn aiff_payload(h: &[u8; 16]) -> Option<u64> {
    let be = |i: usize| u32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    if &h[..4] != b"SSND" || be(8) != 0 || be(12) != 0 {
        return None;
    }
    be(4).checked_sub(8).map(u64::from)
}

/// Bytes per sample of the uncompressed PCM codecs symphonia's RIFF
/// demuxer produces (one frame per block); `None` for anything else.
pub(super) fn sample_bytes(codec: AudioCodecId) -> Option<u64> {
    use codec_ids::*;
    match codec {
        CODEC_ID_PCM_U8 | CODEC_ID_PCM_S8 | CODEC_ID_PCM_ALAW | CODEC_ID_PCM_MULAW => Some(1),
        CODEC_ID_PCM_S16LE | CODEC_ID_PCM_S16BE => Some(2),
        CODEC_ID_PCM_S24LE | CODEC_ID_PCM_S24BE => Some(3),
        CODEC_ID_PCM_S32LE | CODEC_ID_PCM_S32BE | CODEC_ID_PCM_F32LE | CODEC_ID_PCM_F32BE => {
            Some(4)
        }
        CODEC_ID_PCM_F64LE | CODEC_ID_PCM_F64BE => Some(8),
        _ => None,
    }
}

/// Reads `buf.len()` bytes at `at`: from the stream's buffer when it still
/// holds them, else with one seek + read.
fn read_at(mss: &mut MediaSourceStream<'_>, at: u64, buf: &mut [u8]) -> Option<()> {
    if mss.seek_buffered(at) != at {
        mss.seek(SeekFrom::Start(at)).ok()?;
    }
    mss.read_buf_exact(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_frames_requires_every_rule() {
        // 100 stereo 16-bit frames at offset 44 in a 444-byte file.
        assert_eq!(exact_frames(100, 400, 4, 44, 444), Some(100));
        assert_eq!(
            exact_frames(100, 400, 4, 44, 1_000),
            Some(100),
            "trailing chunks"
        );
        assert_eq!(exact_frames(100, 400, 4, 44, 443), None, "one byte missing");
        assert_eq!(
            exact_frames(100, 401, 4, 44, 445),
            None,
            "not frame-aligned"
        );
        assert_eq!(exact_frames(99, 400, 4, 44, 444), None, "declared mismatch");
        assert_eq!(exact_frames(0, 0, 4, 44, 44), Some(0), "empty payload");
        assert_eq!(exact_frames(1, 4, 0, 44, 444), None, "zero frame size");
        assert_eq!(
            exact_frames(1, 4, 4, u64::MAX - 1, u64::MAX),
            None,
            "overflow"
        );
        let huge = MAX_PACKETS * RIFF_PACKET_FRAMES;
        assert_eq!(
            exact_frames(huge, huge, 1, 0, u64::MAX),
            None,
            "packet limit"
        );
    }

    #[test]
    fn chunk_headers_are_parsed_strictly() {
        let wave = |tag: &[u8; 4], size: u32| {
            let mut h = [0u8; 8];
            h[..4].copy_from_slice(tag);
            h[4..].copy_from_slice(&size.to_le_bytes());
            h
        };
        assert_eq!(wave_payload(&wave(b"data", 1_000)), Some(1_000));
        assert_eq!(
            wave_payload(&wave(b"data", u32::MAX)),
            None,
            "streamed/RF64"
        );
        assert_eq!(wave_payload(&wave(b"LIST", 1_000)), None);

        let aiff = |tag: &[u8; 4], size: u32, offset: u32, block: u32| {
            let mut h = [0u8; 16];
            h[..4].copy_from_slice(tag);
            h[4..8].copy_from_slice(&size.to_be_bytes());
            h[8..12].copy_from_slice(&offset.to_be_bytes());
            h[12..].copy_from_slice(&block.to_be_bytes());
            h
        };
        assert_eq!(aiff_payload(&aiff(b"SSND", 1_008, 0, 0)), Some(1_000));
        assert_eq!(aiff_payload(&aiff(b"SSND", 8, 0, 0)), Some(0));
        assert_eq!(aiff_payload(&aiff(b"SSND", 7, 0, 0)), None);
        assert_eq!(aiff_payload(&aiff(b"SSND", 1_008, 4, 0)), None, "offset");
        assert_eq!(
            aiff_payload(&aiff(b"SSND", 1_008, 0, 8)),
            None,
            "block size"
        );
        assert_eq!(aiff_payload(&aiff(b"COMM", 1_008, 0, 0)), None);
    }

    #[test]
    fn only_uncompressed_pcm_has_a_sample_size() {
        assert_eq!(sample_bytes(codec_ids::CODEC_ID_PCM_S24LE), Some(3));
        assert_eq!(sample_bytes(codec_ids::CODEC_ID_PCM_F64BE), Some(8));
        assert_eq!(sample_bytes(codec_ids::CODEC_ID_PCM_MULAW), Some(1));
        for c in [
            codec_ids::CODEC_ID_ADPCM_IMA_WAV,
            codec_ids::CODEC_ID_ADPCM_MS,
            codec_ids::CODEC_ID_FLAC,
            codec_ids::CODEC_ID_ALAC,
        ] {
            assert_eq!(sample_bytes(c), None);
        }
    }
}

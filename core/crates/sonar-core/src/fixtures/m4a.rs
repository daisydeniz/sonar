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
//! Layout: `ftyp`, `mdat` (all frames), `moov` (`mvhd`, `trak` → `tkhd`,
//! `mdia` → `mdhd`, `hdlr` `soun`, `minf` → `smhd`, `dinf`, `stbl` →
//! `stsd`, `stts`, `stsc`, `stsz`, `stco`). Media timescale = sample
//! rate, so `stts` durations are frames and their sum is the exact length;
//! no edit list (ALAC has no priming).

use super::bits::BitWriter;
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

    let ftyp = boxed(
        b"ftyp",
        &[&b"M4A "[..], &[0; 4], b"M4A ", b"mp42", b"isom"].concat(),
    );
    let mdat_body: Vec<u8> = packets.concat();
    let mdat = boxed(b"mdat", &mdat_body);
    let chunk_offset = (ftyp.len() + 8) as u32;
    let moov = moov(pcm, &packets, chunk_offset);
    [ftyp, mdat, moov].concat()
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

fn moov(pcm: &Pcm, packets: &[Vec<u8>], chunk_offset: u32) -> Vec<u8> {
    let rate = pcm.sample_rate;
    let frames = u32::try_from(pcm.frames()).unwrap_or(u32::MAX);
    let total_bytes: usize = packets.iter().map(Vec::len).sum();
    let max_frame = packets.iter().map(Vec::len).max().unwrap_or(0) as u32;
    let avg_bit_rate = if frames == 0 {
        0
    } else {
        (total_bytes as u64 * 8 * u64::from(rate) / u64::from(frames)) as u32
    };

    // Matrix: identity in 16.16 / 2.30 fixed point.
    let matrix: Vec<u8> = [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    let mvhd = full_box(
        b"mvhd",
        0,
        &[
            &u32s(&[0, 0, rate, frames, 0x0001_0000])[..],
            &0x0100u16.to_be_bytes(),
            &[0; 10],
            &matrix,
            &[0; 24],
            &2u32.to_be_bytes(), // next track id
        ]
        .concat(),
    );
    let tkhd = full_box(
        b"tkhd",
        7, // enabled | in movie | in preview
        &[
            &u32s(&[0, 0, 1, 0, frames, 0, 0])[..],
            &[0, 0, 0, 0], // layer, alternate group
            &0x0100u16.to_be_bytes(),
            &[0, 0],
            &matrix,
            &[0; 8], // width, height
        ]
        .concat(),
    );
    let mdhd = full_box(
        b"mdhd",
        0,
        &[
            &u32s(&[0, 0, rate, frames])[..],
            &0x55C4u16.to_be_bytes(),
            &[0, 0],
        ]
        .concat(),
    );
    let hdlr = full_box(
        b"hdlr",
        0,
        &[&[0; 4][..], b"soun", &[0; 12], b"SoundHandler\0"].concat(),
    );
    let smhd = full_box(b"smhd", 0, &[0; 4]);
    let url = full_box(b"url ", 1, &[]); // self-contained
    let dref = full_box(b"dref", 0, &[&1u32.to_be_bytes()[..], &url].concat());
    let dinf = boxed(b"dinf", &dref);
    let stbl = boxed(
        b"stbl",
        &[
            stsd(pcm, max_frame, avg_bit_rate),
            stts(packets.len(), pcm.frames()),
            full_box(b"stsc", 0, &u32s(&[1, 1, packets.len() as u32, 1])),
            stsz(packets),
            full_box(b"stco", 0, &u32s(&[1, chunk_offset])),
        ]
        .concat(),
    );
    let minf = boxed(b"minf", &[smhd, dinf, stbl].concat());
    let mdia = boxed(b"mdia", &[mdhd, hdlr, minf].concat());
    let trak = boxed(b"trak", &[tkhd, mdia].concat());
    boxed(b"moov", &[mvhd, trak].concat())
}

/// `stsd` with one `alac` audio sample entry (version 0) + `alac` config.
fn stsd(pcm: &Pcm, max_frame: u32, avg_bit_rate: u32) -> Vec<u8> {
    // 16.16 fixed rate; rates ≥ 65536 Hz do not fit (the cookie is used).
    let rate_fixed = if pcm.sample_rate < 65_536 {
        pcm.sample_rate << 16
    } else {
        0
    };
    let cookie = full_box(b"alac", 0, &magic_cookie(pcm, max_frame, avg_bit_rate));
    let entry = boxed(
        b"alac",
        &[
            &[0; 6][..],
            &1u16.to_be_bytes(), // data reference index
            &[0; 8],             // version, revision, vendor
            &pcm.channels.to_be_bytes(),
            &pcm.bits.to_be_bytes(),
            &[0; 4], // compression id, packet size
            &rate_fixed.to_be_bytes(),
            &cookie,
        ]
        .concat(),
    );
    full_box(b"stsd", 0, &[&1u32.to_be_bytes()[..], &entry].concat())
}

/// `stts`: full frames of [`FRAME_LENGTH`], then the shorter last one.
fn stts(packets: usize, frames: usize) -> Vec<u8> {
    let last = frames - (packets.saturating_sub(1)) * FRAME_LENGTH;
    let mut runs = Vec::new();
    if last == FRAME_LENGTH {
        runs.push((packets as u32, FRAME_LENGTH as u32));
    } else if packets > 0 {
        if packets > 1 {
            runs.push((packets as u32 - 1, FRAME_LENGTH as u32));
        }
        runs.push((1, last as u32));
    }
    let mut body = u32s(&[runs.len() as u32]);
    for (count, delta) in runs {
        body.extend_from_slice(&u32s(&[count, delta]));
    }
    full_box(b"stts", 0, &body)
}

fn stsz(packets: &[Vec<u8>]) -> Vec<u8> {
    let mut body = u32s(&[0, packets.len() as u32]);
    for p in packets {
        body.extend_from_slice(&(p.len() as u32).to_be_bytes());
    }
    full_box(b"stsz", 0, &body)
}

/// Big-endian u32 sequence.
fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A box: 32-bit size, type, body.
fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(8 + body.len()).unwrap_or(u32::MAX);
    [&size.to_be_bytes()[..], kind, body].concat()
}

/// A full box: version 0 + 24-bit `flags`, then the body.
fn full_box(kind: &[u8; 4], flags: u32, body: &[u8]) -> Vec<u8> {
    boxed(
        kind,
        &[&(flags & 0xFF_FFFF).to_be_bytes()[..], body].concat(),
    )
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
    fn stts_runs_cover_every_frame() {
        let total = |b: &[u8]| -> u64 {
            let n = u32::from_be_bytes([b[12], b[13], b[14], b[15]]) as usize;
            (0..n)
                .map(|i| {
                    let o = 16 + 8 * i;
                    let c = u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
                    let d = u32::from_be_bytes([b[o + 4], b[o + 5], b[o + 6], b[o + 7]]);
                    u64::from(c) * u64::from(d)
                })
                .sum()
        };
        assert_eq!(total(&stts(11, 44_100)), 44_100);
        assert_eq!(total(&stts(2, 2 * FRAME_LENGTH)), 8_192);
        assert_eq!(total(&stts(1, 100)), 100);
    }

    #[test]
    fn file_starts_with_ftyp_then_mdat_at_the_chunk_offset() {
        let b = encode(&pcm(1, 16, 10));
        assert_eq!(&b[4..8], b"ftyp");
        let ftyp_len = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        assert_eq!(&b[ftyp_len + 4..ftyp_len + 8], b"mdat");
        assert!(b.windows(4).any(|w| w == b"moov"));
    }
}

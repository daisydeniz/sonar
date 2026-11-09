//! Minimal Ogg Vorbis writer: digital silence with exact granule positions.
//!
//! Vorbis I (Xiph.Org "Vorbis I specification"): identification, comment
//! and setup headers (§4.2) and audio packets (§4.3) in Ogg pages
//! (RFC 3533; Vorbis-in-Ogg mapping, spec appendix A).
//!
//! The setup header is the smallest valid one: one 2-entry codebook (both
//! codewords 1 bit long, no VQ lookup), one floor 1 with no partitions, one
//! residue 0 that covers no coefficients, one mapping, and one mode using
//! short blocks of 2^[`BS0_EXP`] samples. Each audio packet is the single
//! byte `0x00`: packet type 0, then a cleared "nonzero" flag per channel,
//! i.e. the floor is unused and the channel decodes to exact zeros (§4.3.2).
//!
//! Length: a decoder returns nothing for the first audio packet and
//! `PACKET_FRAMES` (half a short block) for each later one, so
//! `ceil(frames / PACKET_FRAMES) + 1` packets are written. Each page's
//! granule position is the number of frames decoded through its last
//! packet; the last page (EOS) carries `frames`, which trims the excess of
//! the final packet (spec §A.2, end-of-stream trimming).

use super::bits::{crc32_ogg, LsbWriter};

/// Short-block size exponent (256 samples; the only block size used).
pub const BS0_EXP: u32 = 8;
/// Long-block size exponent (2048 samples; declared, unused).
const BS1_EXP: u32 = 11;
/// Frames each audio packet after the first adds (two half-overlapping
/// short blocks: 256/4 + 256/4).
pub const PACKET_FRAMES: u64 = 1 << (BS0_EXP - 1);
/// Audio packets per page (≤ 255 one-byte packets fit one segment table).
const PACKETS_PER_PAGE: usize = 64;
/// Bitstream serial number ("SONR").
const SERIAL: u32 = 0x534F_4E52;
/// Comment-header vendor string.
const VENDOR: &[u8] = b"Sonar fixtures";

/// Page header flags (RFC 3533 §6).
const BOS: u8 = 0x02;
const EOS: u8 = 0x04;

/// Encodes `frames` frames of digital silence at `sample_rate` with
/// `channels` (1–7) channels. `None` for invalid parameters. (Each audio
/// packet is one byte: a 1-bit packet type plus one 1-bit floor-unused
/// flag per channel, so at most 7 channels fit.)
pub fn encode_silence(sample_rate: u32, channels: u16, frames: u64) -> Option<Vec<u8>> {
    if sample_rate == 0 || !(1..=7).contains(&channels) {
        return None;
    }
    let ch = u8::try_from(channels).ok()?;
    let mut out = Vec::new();
    let mut seq = 0u32;
    let mut page = |flags: u8, granule: u64, packets: &[&[u8]], out: &mut Vec<u8>| {
        out.extend_from_slice(&ogg_page(flags, granule, seq, packets));
        seq += 1;
    };
    page(BOS, 0, &[&ident_header(sample_rate, ch)], &mut out);
    page(0, 0, &[&comment_header(), &setup_header()], &mut out);

    let n_packets = usize::try_from(frames.div_ceil(PACKET_FRAMES) + 1).ok()?;
    let audio = [0u8]; // see module docs
    let mut done = 0usize;
    while done < n_packets {
        let k = (n_packets - done).min(PACKETS_PER_PAGE);
        done += k;
        let last = done == n_packets;
        // Frames decoded through this page's last packet (first packet: 0).
        let decoded = (done as u64 - 1) * PACKET_FRAMES;
        let (flags, granule) = if last { (EOS, frames) } else { (0, decoded) };
        let packets = vec![&audio[..]; k];
        page(flags, granule, &packets, &mut out);
    }
    Some(out)
}

/// Identification header (§4.2.2), 30 bytes.
fn ident_header(sample_rate: u32, channels: u8) -> Vec<u8> {
    let mut h = vec![1];
    h.extend_from_slice(b"vorbis");
    h.extend_from_slice(&0u32.to_le_bytes()); // vorbis_version
    h.push(channels);
    h.extend_from_slice(&sample_rate.to_le_bytes());
    h.extend_from_slice(&[0; 12]); // bitrate maximum, nominal, minimum: unset
    h.push(((BS1_EXP << 4) | BS0_EXP) as u8);
    h.push(1); // framing flag
    h
}

/// Comment header (§5.2): vendor string, no user comments.
fn comment_header() -> Vec<u8> {
    let mut h = vec![3];
    h.extend_from_slice(b"vorbis");
    h.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
    h.extend_from_slice(VENDOR);
    h.extend_from_slice(&0u32.to_le_bytes()); // user comment count
    h.push(1); // framing flag
    h
}

/// Setup header (§4.2.4), see module docs.
fn setup_header() -> Vec<u8> {
    let mut w = LsbWriter::new();
    // Codebooks (§3.2.1).
    w.put(0, 8); // count − 1
    w.put(0x56_4342, 24); // sync "BCV"
    w.put(1, 16); // dimensions
    w.put(2, 24); // entries
    w.put(0, 1); // not length-ordered
    w.put(0, 1); // not sparse
    w.put(0, 5); // entry 0: length 1
    w.put(0, 5); // entry 1: length 1
    w.put(0, 4); // lookup type 0: scalar only
                 // Time-domain transforms: one placeholder.
    w.put(0, 6);
    w.put(0, 16);
    // Floors: one floor 1 (§7.2.2) with no partitions.
    w.put(0, 6);
    w.put(1, 16); // floor type 1
    w.put(0, 5); // partitions
    w.put(0, 2); // multiplier − 1
    w.put(BS1_EXP - 1, 4); // rangebits: X list {0, 2^(bs1−1)}
                           // Residues: one residue 0 (§8.6.1) covering no coefficients.
    w.put(0, 6);
    w.put(0, 16); // residue type 0
    w.put(0, 24); // begin
    w.put(0, 24); // end
    w.put(0, 24); // partition size − 1
    w.put(0, 6); // classifications − 1
    w.put(0, 8); // classbook
    w.put(0, 3); // cascade low bits
    w.put(0, 1); // no high bits
                 // Mappings: one mapping 0, one submap, no coupling.
    w.put(0, 6);
    w.put(0, 16); // mapping type 0
    w.put(0, 1); // single submap
    w.put(0, 1); // no coupling
    w.put(0, 2); // reserved
    w.put(0, 8); // submap time config (unused)
    w.put(0, 8); // floor 0
    w.put(0, 8); // residue 0
                 // Modes: one short-block mode.
    w.put(0, 6);
    w.put(0, 1); // blockflag: short
    w.put(0, 16); // window type
    w.put(0, 16); // transform type
    w.put(0, 8); // mapping 0
    w.put(1, 1); // framing flag
    let mut h = vec![5];
    h.extend_from_slice(b"vorbis");
    h.extend_from_slice(&w.into_bytes());
    h
}

/// One Ogg page holding whole `packets` (RFC 3533 §6). Every packet must
/// end on this page and the lacing values must fit in 255 segments.
fn ogg_page(flags: u8, granule: u64, seq: u32, packets: &[&[u8]]) -> Vec<u8> {
    let mut lacing = Vec::new();
    for p in packets {
        lacing.extend(std::iter::repeat_n(255u8, p.len() / 255));
        lacing.push((p.len() % 255) as u8);
    }
    let mut page = b"OggS".to_vec();
    page.push(0); // stream structure version
    page.push(flags);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&SERIAL.to_le_bytes());
    page.extend_from_slice(&seq.to_le_bytes());
    page.extend_from_slice(&[0; 4]); // CRC, filled below
    debug_assert!(lacing.len() <= 255, "Ogg page needs ≤ 255 segments");
    page.push(lacing.len().min(255) as u8);
    page.extend_from_slice(&lacing);
    for p in packets {
        page.extend_from_slice(p);
    }
    let crc = crc32_ogg(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits a byte stream into pages: `(flags, granule, seq, payload)`.
    fn pages(b: &[u8]) -> Vec<(u8, u64, u32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            assert_eq!(&b[i..i + 4], b"OggS");
            let n = usize::from(b[i + 26]);
            let body: usize = b[i + 27..i + 27 + n].iter().map(|&x| usize::from(x)).sum();
            let len = 27 + n + body;
            let mut page = b[i..i + len].to_vec();
            let crc = u32::from_le_bytes([page[22], page[23], page[24], page[25]]);
            page[22..26].fill(0);
            assert_eq!(crc32_ogg(&page), crc);
            let granule = u64::from_le_bytes(b[i + 6..i + 14].try_into().unwrap());
            let seq = u32::from_le_bytes(b[i + 18..i + 22].try_into().unwrap());
            out.push((b[i + 5], granule, seq, b[i + 27 + n..i + len].to_vec()));
            i += len;
        }
        out
    }

    #[test]
    fn headers_have_spec_layout() {
        let id = ident_header(44_100, 2);
        assert_eq!(id.len(), 30);
        assert_eq!(&id[..7], b"\x01vorbis");
        assert_eq!(id[28], 0xB8); // blocksize_1 = 2^11, blocksize_0 = 2^8
        let setup = setup_header();
        assert_eq!(&setup[..7], b"\x05vorbis");
        // Codebook sync pattern right after the 8-bit count (LSB-first).
        assert_eq!(&setup[8..11], &[0x42, 0x43, 0x56]);
    }

    #[test]
    fn granules_count_frames_and_trim_the_last_packet() {
        let frames = 12_345;
        let b = encode_silence(48_000, 1, frames).unwrap();
        let p = pages(&b);
        assert_eq!(p[0].0, BOS);
        assert_eq!((p[0].1, p[1].1), (0, 0));
        let audio = &p[2..];
        let packets: usize = audio.iter().map(|x| x.3.len()).sum();
        assert_eq!(packets as u64, frames.div_ceil(PACKET_FRAMES) + 1);
        let last = audio.last().unwrap();
        assert_eq!((last.0, last.1), (EOS, frames));
        assert!((packets as u64 - 1) * PACKET_FRAMES - frames < PACKET_FRAMES);
        for (i, page) in p.iter().enumerate() {
            assert_eq!(page.2, i as u32);
        }
        // Non-final granules: frames decoded through the page.
        let first = &audio[0];
        assert_eq!(first.1, (first.3.len() as u64 - 1) * PACKET_FRAMES);
    }

    #[test]
    fn rejects_invalid_parameters() {
        assert!(encode_silence(0, 1, 10).is_none());
        assert!(encode_silence(44_100, 0, 10).is_none());
        assert!(encode_silence(44_100, 8, 10).is_none());
        assert!(encode_silence(44_100, 7, 10).is_some());
    }
}

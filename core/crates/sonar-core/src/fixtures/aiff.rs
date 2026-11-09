//! Minimal AIFF writer (Apple "Audio Interchange File Format" 1.3):
//! `FORM` `AIFF` with a `COMM` chunk (channels, frames, bits, 80-bit IEEE
//! 754 extended sample rate) and an `SSND` chunk of big-endian signed PCM.

use super::pcm::Pcm;

/// Encodes `pcm` (8–32 bits; stored in whole bytes) as an AIFF file.
pub fn encode(pcm: &Pcm) -> Vec<u8> {
    let bytes_per_sample = usize::from(pcm.bits.div_ceil(8));
    let mut data = Vec::with_capacity(pcm.samples.len() * bytes_per_sample);
    for &s in &pcm.samples {
        data.extend_from_slice(&s.to_be_bytes()[4 - bytes_per_sample..]);
    }
    let pad = data.len() % 2;
    let comm_len = 18u32;
    let ssnd_len = 8 + data.len();
    let form_len = 4 + (8 + comm_len as usize) + (8 + ssnd_len) + pad;

    let mut out = Vec::with_capacity(8 + form_len);
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&be32(form_len));
    out.extend_from_slice(b"AIFFCOMM");
    out.extend_from_slice(&comm_len.to_be_bytes());
    out.extend_from_slice(&pcm.channels.to_be_bytes());
    out.extend_from_slice(&be32(pcm.frames()));
    out.extend_from_slice(&pcm.bits.to_be_bytes());
    out.extend_from_slice(&extended(pcm.sample_rate));
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&be32(ssnd_len));
    out.extend_from_slice(&[0; 8]); // offset, block size
    out.extend_from_slice(&data);
    if pad == 1 {
        out.push(0);
    }
    out
}

/// Saturating big-endian u32 (fixture sizes never reach 4 GiB).
fn be32(v: usize) -> [u8; 4] {
    u32::try_from(v).unwrap_or(u32::MAX).to_be_bytes()
}

/// 80-bit IEEE 754 extended encoding of a positive integer rate (exact).
fn extended(rate: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if rate == 0 {
        return out;
    }
    let e = 31 - rate.leading_zeros();
    out[..2].copy_from_slice(&(16_383 + e as u16).to_be_bytes());
    out[2..].copy_from_slice(&(u64::from(rate) << (63 - e)).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_encodes_common_rates() {
        // 44100 = 1.3458… × 2^15.
        assert_eq!(extended(44_100), [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        assert_eq!(extended(48_000)[..4], [0x40, 0x0E, 0xBB, 0x80]);
        assert_eq!(extended(0), [0; 10]);
    }

    #[test]
    fn layout_of_a_24_bit_mono_file() {
        let pcm = Pcm {
            sample_rate: 48_000,
            channels: 1,
            bits: 24,
            samples: vec![1, -2, 0x12_3456],
        };
        let b = encode(&pcm);
        assert_eq!(&b[..4], b"FORM");
        assert_eq!(b.len(), 8 + 4 + 26 + 16 + 9 + 1);
        assert_eq!(
            u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize,
            b.len() - 8
        );
        assert_eq!(&b[22..26], &3u32.to_be_bytes()); // frames
        assert_eq!(&b[54..63], &[0, 0, 1, 0xFF, 0xFF, 0xFE, 0x12, 0x34, 0x56]);
        assert_eq!(b[63], 0); // pad byte
    }
}

//! Minimal lossless FLAC writer (RFC 9639).
//!
//! Stream: `fLaC`, one STREAMINFO block (total samples set, MD5 left zero
//! = "unknown", §8.2), then fixed-blocksize frames of [`BLOCK_SIZE`]
//! samples (the last one shorter). Channels are coded independently. Per
//! subframe the cheapest of CONSTANT, VERBATIM and FIXED orders 0–4
//! (§9.2.5) is chosen; residuals use one Rice partition (§9.2.7) with a
//! 4-bit parameter, or a 5-bit one when the best parameter exceeds 14.
//! Frame headers carry a CRC-8 and frames a CRC-16 (§9.1.8, §9.3).
//! Decoding must reproduce the input samples exactly.

use super::bits::{crc16, crc8, BitWriter};
use super::pcm::Pcm;

/// Samples per channel in every frame but the last.
pub const BLOCK_SIZE: usize = 4096;

/// Encodes `pcm` (4–24 bits, 1–8 channels) as a FLAC file. (At 32 bits the
/// FIXED-predictor residuals could exceed the i32 range RFC 9639 requires;
/// [`Pcm::quantize`] caps at 24 bits.)
pub fn encode(pcm: &Pcm) -> Vec<u8> {
    let mut out = b"fLaC".to_vec();
    out.extend_from_slice(&stream_info(pcm));
    let planes = pcm.planes();
    let frames = pcm.frames();
    for (n, start) in (0..frames).step_by(BLOCK_SIZE).enumerate() {
        let len = (frames - start).min(BLOCK_SIZE);
        let block: Vec<&[i32]> = planes.iter().map(|p| &p[start..start + len]).collect();
        out.extend_from_slice(&frame(pcm, n as u64, &block));
    }
    out
}

/// Metadata block header (last block, type 0) + 34-byte STREAMINFO.
fn stream_info(pcm: &Pcm) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.put(1, 1); // last metadata block
    w.put(0, 7); // STREAMINFO
    w.put(34, 24);
    w.put(BLOCK_SIZE as u64, 16); // min block size
    w.put(BLOCK_SIZE as u64, 16); // max block size
    w.put(0, 24); // min frame size: unknown
    w.put(0, 24); // max frame size: unknown
    w.put(u64::from(pcm.sample_rate), 20);
    w.put(u64::from(pcm.channels) - 1, 3);
    w.put(u64::from(pcm.bits) - 1, 5);
    let total = pcm.frames() as u64;
    w.put(total >> 32, 4);
    w.put(total & 0xFFFF_FFFF, 32);
    for _ in 0..4 {
        w.put(0, 32); // MD5: unknown
    }
    w.into_bytes()
}

/// One frame: header, one subframe per channel, CRC-16 footer.
fn frame(pcm: &Pcm, number: u64, block: &[&[i32]]) -> Vec<u8> {
    let len = block.first().map_or(0, |b| b.len());
    let mut w = BitWriter::new();
    w.put(0b11_1111_1111_1110, 14); // sync
    w.put(0, 1); // reserved
    w.put(0, 1); // fixed blocking strategy
    w.put(0b0111, 4); // block size: 16-bit (size − 1) at end of header
    w.put(rate_code(pcm.sample_rate), 4);
    w.put(u64::from(pcm.channels) - 1, 4); // independent channels
    w.put(size_code(pcm.bits), 3);
    w.put(0, 1); // reserved
    for b in utf8_number(number) {
        w.put(u64::from(b), 8);
    }
    w.put(len as u64 - 1, 16);
    let mut bytes = w.into_bytes();
    bytes.push(crc8(&bytes));

    let mut w = BitWriter::new();
    for samples in block {
        subframe(&mut w, samples, u32::from(pcm.bits));
    }
    bytes.extend_from_slice(&w.into_bytes());
    let crc = crc16(&bytes);
    bytes.extend_from_slice(&crc.to_be_bytes());
    bytes
}

/// Sample-rate code (§9.1.2); 0 = "from STREAMINFO".
fn rate_code(rate: u32) -> u64 {
    match rate {
        88_200 => 0b0001,
        176_400 => 0b0010,
        192_000 => 0b0011,
        8_000 => 0b0100,
        16_000 => 0b0101,
        22_050 => 0b0110,
        24_000 => 0b0111,
        32_000 => 0b1000,
        44_100 => 0b1001,
        48_000 => 0b1010,
        96_000 => 0b1011,
        _ => 0,
    }
}

/// Bit-depth code (§9.1.4); 0 = "from STREAMINFO".
fn size_code(bits: u16) -> u64 {
    match bits {
        8 => 0b001,
        12 => 0b010,
        16 => 0b100,
        20 => 0b101,
        24 => 0b110,
        32 => 0b111,
        _ => 0,
    }
}

/// UTF-8-style coded frame number (§9.1.5, up to 36 bits).
fn utf8_number(n: u64) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    // Continuation bytes carry 6 bits each; the lead byte carries the rest.
    let extra = match n {
        0..=0x7FF => 1,
        0x800..=0xFFFF => 2,
        0x1_0000..=0x1F_FFFF => 3,
        0x20_0000..=0x3FF_FFFF => 4,
        0x400_0000..=0x7FFF_FFFF => 5,
        _ => 6,
    };
    let lead_marker = !(0xFFu8 >> (extra + 1));
    let mut out = vec![lead_marker | (n >> (6 * extra)) as u8];
    for i in (0..extra).rev() {
        out.push(0x80 | ((n >> (6 * i)) & 0x3F) as u8);
    }
    out
}

/// Subframe type chosen for one channel block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Constant,
    Verbatim,
    Fixed { order: usize, rice: u32 },
}

fn subframe(w: &mut BitWriter, x: &[i32], bps: u32) {
    let kind = choose(x, bps);
    w.put(0, 1); // zero padding bit
    match kind {
        Kind::Constant => {
            w.put(0b00_0000, 6);
            w.put(0, 1); // no wasted bits
            w.put_signed(i64::from(x.first().copied().unwrap_or(0)), bps);
        }
        Kind::Verbatim => {
            w.put(0b00_0001, 6);
            w.put(0, 1);
            for &s in x {
                w.put_signed(i64::from(s), bps);
            }
        }
        Kind::Fixed { order, rice } => {
            w.put(0b00_1000 | order as u64, 6);
            w.put(0, 1);
            for &s in &x[..order] {
                w.put_signed(i64::from(s), bps);
            }
            let wide = rice > 14;
            w.put(u64::from(wide), 2); // 0b00: 4-bit, 0b01: 5-bit params
            w.put(0, 4); // partition order 0
            w.put(u64::from(rice), if wide { 5 } else { 4 });
            for u in residuals(x, order).map(zigzag) {
                w.put_unary(u >> rice);
                w.put(u & ((1u64 << rice) - 1), rice);
            }
        }
    }
}

/// Cheapest subframe type for `x` by exact bit count. Predictor orders
/// stay below the block length so every FIXED subframe has residuals.
fn choose(x: &[i32], bps: u32) -> Kind {
    if x.windows(2).all(|p| p[0] == p[1]) {
        return Kind::Constant;
    }
    let bps = u64::from(bps);
    let mut best = (bps * x.len() as u64, Kind::Verbatim);
    for order in 0..=4.min(x.len() - 1) {
        let (rice, bits) = best_rice(x, order);
        let total = 4 + 4 + 5 + order as u64 * bps + bits;
        if total < best.0 {
            best = (total, Kind::Fixed { order, rice });
        }
    }
    best.1
}

/// Best Rice parameter (0..=30) for the order-`order` residual and its
/// residual cost in bits.
fn best_rice(x: &[i32], order: usize) -> (u32, u64) {
    let n = (x.len() - order) as u64;
    let sum: u64 = residuals(x, order).map(zigzag).sum();
    let mean = sum / n.max(1);
    let guess = 63 - (mean + 1).leading_zeros();
    let cost = |k: u32| -> u64 {
        n * (u64::from(k) + 1) + residuals(x, order).map(|r| zigzag(r) >> k).sum::<u64>()
    };
    (guess.saturating_sub(1)..=(guess + 1).min(30))
        .map(|k| (k, cost(k)))
        .min_by_key(|&(_, c)| c)
        .unwrap_or((0, cost(0)))
}

/// Fixed-predictor residuals of order 0–4 (RFC 9639 §9.2.5).
fn residuals(x: &[i32], order: usize) -> impl Iterator<Item = i64> + '_ {
    (order..x.len()).map(move |i| {
        let s = |k: usize| i64::from(x[i - k]);
        match order {
            0 => s(0),
            1 => s(0) - s(1),
            2 => s(0) - 2 * s(1) + s(2),
            3 => s(0) - 3 * s(1) + 3 * s(2) - s(3),
            _ => s(0) - 4 * s(1) + 6 * s(2) - 4 * s(3) + s(4),
        }
    })
}

/// Folds a signed residual to unsigned: 0, −1, 1, −2 … → 0, 1, 2, 3 …
fn zigzag(r: i64) -> u64 {
    ((r << 1) ^ (r >> 63)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_numbers() {
        assert_eq!(utf8_number(0), [0]);
        assert_eq!(utf8_number(0x7F), [0x7F]);
        assert_eq!(utf8_number(0x80), [0xC2, 0x80]);
        assert_eq!(utf8_number(0x7FF), [0xDF, 0xBF]);
        assert_eq!(utf8_number(0x800), [0xE0, 0xA0, 0x80]);
        assert_eq!(utf8_number(0x10_000), [0xF0, 0x90, 0x80, 0x80]);
    }

    #[test]
    fn residual_orders_and_zigzag() {
        let x = [1, 4, 9, 16, 25, 36];
        let r2: Vec<i64> = residuals(&x, 2).collect();
        assert_eq!(r2, [2, 2, 2, 2]);
        let r3: Vec<i64> = residuals(&x, 3).collect();
        assert_eq!(r3, [0, 0, 0]);
        assert_eq!([0, -1, 1, -2, 2].map(zigzag), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn subframe_choice() {
        assert_eq!(choose(&[7; 64], 16), Kind::Constant);
        let ramp: Vec<i32> = (0..64).map(|i| i * 100).collect();
        assert!(matches!(
            choose(&ramp, 16),
            Kind::Fixed { order: 2, rice: 0 }
        ));
        // Two samples cannot beat verbatim.
        assert_eq!(choose(&[1, -30_000], 16), Kind::Verbatim);
    }

    #[test]
    fn stream_header_fields() {
        let pcm = Pcm {
            sample_rate: 44_100,
            channels: 2,
            bits: 16,
            samples: vec![0; 2 * 5_000],
        };
        let b = encode(&pcm);
        assert_eq!(&b[..8], b"fLaC\x80\x00\x00\x22");
        // Rate 44100 (20 bits) | ch−1 = 1 (3) | bps−1 = 15 (5) | total 5000 (36).
        assert_eq!(
            &b[18..26],
            &[0x0A, 0xC4, 0x42, 0xF0, 0x00, 0x00, 0x13, 0x88]
        );
        // First frame header directly after STREAMINFO.
        assert_eq!(&b[42..44], &[0xFF, 0xF8]);
    }
}

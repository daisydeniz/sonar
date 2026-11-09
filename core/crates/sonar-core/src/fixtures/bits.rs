//! Bit packing and checksums shared by the hand-written codec writers.
//!
//! - [`BitWriter`]: MSB-first packing (FLAC, MPEG audio, ALAC, MP4).
//! - [`LsbWriter`]: LSB-first packing (Vorbis I spec §2.1.4).
//! - CRCs: FLAC frame CRC-8 / CRC-16 (RFC 9639 §9.1.8, §9.3), the LAME
//!   tag CRC-16/ARC, and the Ogg page CRC-32 (RFC 3533 §6).
//!
//! All CRCs are computed bit by bit: fixture files are small, so a table
//! would only add code.

/// MSB-first bit writer: the first bit written becomes the most
/// significant bit of the first byte.
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    bytes: Vec<u8>,
    /// Pending bits, right-aligned.
    acc: u64,
    /// Number of pending bits (< 8 between calls).
    pending: u32,
}

impl BitWriter {
    /// Empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends the low `bits` bits of `value` (`bits` ≤ 32), MSB first.
    pub fn put(&mut self, value: u64, bits: u32) {
        debug_assert!(bits <= 32);
        if bits == 0 {
            return;
        }
        let mask = (1u64 << bits) - 1;
        self.acc = (self.acc << bits) | (value & mask);
        self.pending += bits;
        while self.pending >= 8 {
            self.pending -= 8;
            self.bytes.push((self.acc >> self.pending) as u8);
        }
        self.acc &= (1u64 << self.pending) - 1;
    }

    /// Appends `value` as a two's-complement field of `bits` bits.
    pub fn put_signed(&mut self, value: i64, bits: u32) {
        self.put(value as u64, bits);
    }

    /// Appends `q` zero bits followed by a one (unary code).
    pub fn put_unary(&mut self, mut q: u64) {
        while q >= 32 {
            self.put(0, 32);
            q -= 32;
        }
        self.put(1, q as u32 + 1);
    }

    /// Pads with zero bits to the next byte boundary.
    pub fn align(&mut self) {
        if self.pending > 0 {
            self.put(0, 8 - self.pending);
        }
    }

    /// Bits written so far.
    pub fn bit_len(&self) -> u64 {
        self.bytes.len() as u64 * 8 + u64::from(self.pending)
    }

    /// Byte-aligns and returns the bytes.
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.align();
        self.bytes
    }
}

/// LSB-first bit writer (Vorbis): the first bit written becomes the least
/// significant bit of the first byte; multi-bit values are written least
/// significant bit first.
#[derive(Debug, Default, Clone)]
pub struct LsbWriter {
    bytes: Vec<u8>,
    /// Bits used in the last byte (0 = start a new byte).
    used: u32,
}

impl LsbWriter {
    /// Empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends the low `bits` bits of `value` (`bits` ≤ 32), LSB first.
    pub fn put(&mut self, value: u32, bits: u32) {
        debug_assert!(bits <= 32);
        for i in 0..bits {
            if self.used == 0 {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                if let Some(last) = self.bytes.last_mut() {
                    *last |= 1 << self.used;
                }
            }
            self.used = (self.used + 1) % 8;
        }
    }

    /// Returns the bytes (the last byte is zero-padded).
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// CRC-8, polynomial 0x07, initial value 0 (FLAC frame header).
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// CRC-16, polynomial 0x8005, initial value 0, MSB first (FLAC frame
/// footer).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// CRC-16/ARC: reflected polynomial 0x8005 (0xA001), initial value 0
/// (the LAME tag CRC).
pub fn crc16_arc(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// CRC-32, polynomial 0x04C11DB7, initial value 0, no reflection, no
/// final XOR (Ogg page checksum, RFC 3533 §6).
pub fn crc32_ogg(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msb_writer_packs_across_bytes() {
        let mut w = BitWriter::new();
        w.put(0b101, 3);
        w.put(0b11111, 5);
        w.put(0xABC, 12);
        assert_eq!(w.bit_len(), 20);
        assert_eq!(w.into_bytes(), [0b1011_1111, 0xAB, 0xC0]);
    }

    #[test]
    fn msb_writer_signed_and_unary() {
        let mut w = BitWriter::new();
        w.put_signed(-1, 4);
        w.put_unary(3);
        assert_eq!(w.into_bytes(), [0b1111_0001]);
        let mut w = BitWriter::new();
        w.put_unary(40);
        assert_eq!(w.bit_len(), 41);
        assert_eq!(w.into_bytes(), [0, 0, 0, 0, 0, 0x80]);
    }

    #[test]
    fn lsb_writer_packs_low_bits_first() {
        let mut w = LsbWriter::new();
        w.put(1, 1);
        w.put(0b10, 2);
        w.put(0x1F, 5);
        w.put(0x3, 2);
        assert_eq!(w.into_bytes(), [0b1111_1101, 0b11]);
    }

    // Check values: the standard "123456789" vectors of each CRC model.
    #[test]
    fn crc_check_values() {
        let s = b"123456789";
        assert_eq!(crc8(s), 0xF4); // CRC-8/SMBUS
        assert_eq!(crc16(s), 0xFEE8); // CRC-16/UMTS (BUYPASS)
        assert_eq!(crc16_arc(s), 0xBB3D); // CRC-16/ARC
        assert_eq!(crc32_ogg(s), 0x89A1_897F); // CRC-32/MPEG-2 without init/xorout
    }
}

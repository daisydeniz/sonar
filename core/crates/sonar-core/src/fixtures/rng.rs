//! Deterministic pseudo-random numbers for fixture generation.
//!
//! Hand-rolled on purpose (instead of the `rand` crate) so that fixture bytes
//! can never drift with a dependency upgrade.
//!
//! - [`SplitMix64`]: Steele, Lea & Flood, "Fast Splittable Pseudorandom Number
//!   Generators", OOPSLA 2014; constants from Vigna's reference
//!   `splitmix64.c` (<https://prng.di.unimi.it/splitmix64.c>).
//! - [`fnv1a64`]: Fowler–Noll–Vo FNV-1a, 64-bit offset basis and prime.

/// SplitMix64 generator: a 64-bit Weyl sequence passed through a bijective
/// mixing function. Every seed (including 0) yields a full-period stream.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Creates a generator from a raw seed.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Independent stream for one named item (e.g. a fixture path), so the
    /// content of one fixture does not depend on catalog order.
    pub fn for_stream(seed: u64, name: &str) -> Self {
        let base = Self::new(seed).next_u64();
        Self::new(base ^ fnv1a64(name.as_bytes()))
    }

    /// Next 64 uniformly distributed bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform `f64` in `[0, 1)` built from the top 53 bits.
    pub fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (1u64 << 53) as f64;
        (self.next_u64() >> 11) as f64 * SCALE
    }

    /// Uniform `f64` in `[-1, 1)` (white-noise sample).
    pub fn next_bipolar(&mut self) -> f64 {
        self.next_f64() * 2.0 - 1.0
    }

    /// Fills `buf` with random bytes (little-endian words).
    pub fn fill_bytes(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
}

/// 64-bit FNV-1a hash (offset basis `0xcbf29ce484222325`, prime
/// `0x100000001b3`).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_matches_reference_vectors() {
        // Cross-checked against an independent Python implementation of
        // Vigna's splitmix64.c.
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(r.next_u64(), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(r.next_u64(), 0x06c4_5d18_8009_454f);
        let mut r = SplitMix64::new(1_234_567);
        assert_eq!(r.next_u64(), 0x599e_d017_fb08_fc85);
        assert_eq!(r.next_u64(), 0x2c73_f084_5854_0fa5);
        assert_eq!(r.next_u64(), 0x883e_bce5_a3f2_7c77);
    }

    #[test]
    fn fnv1a64_matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn floats_stay_in_range_and_are_roughly_uniform() {
        let mut r = SplitMix64::new(42);
        let n = 20_000;
        let mut sum = 0.0;
        for _ in 0..n {
            let x = r.next_bipolar();
            assert!((-1.0..1.0).contains(&x));
            sum += x;
        }
        assert!((sum / f64::from(n)).abs() < 0.02, "mean {}", sum / 20_000.0);
    }

    #[test]
    fn streams_are_deterministic_and_distinct() {
        let a1 = SplitMix64::for_stream(7, "noise/noise_000.wav").next_u64();
        let a2 = SplitMix64::for_stream(7, "noise/noise_000.wav").next_u64();
        let b = SplitMix64::for_stream(7, "noise/noise_001.wav").next_u64();
        let c = SplitMix64::for_stream(8, "noise/noise_000.wav").next_u64();
        assert_eq!(a1, a2);
        assert_ne!(a1, b);
        assert_ne!(a1, c);
    }

    #[test]
    fn fill_bytes_handles_partial_words() {
        let mut buf = [0u8; 13];
        SplitMix64::new(0).fill_bytes(&mut buf);
        assert_eq!(&buf[..8], &0xe220_a839_7b1d_cdafu64.to_le_bytes());
        assert_eq!(&buf[8..], &0x6e78_9e6a_a1b9_65f4u64.to_le_bytes()[..5]);
    }
}

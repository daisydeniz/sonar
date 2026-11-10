//! 12-bin chroma (pitch-class profile) from an STFT power spectrogram.
//!
//! Each bin between [`CHROMA_FMIN`] and [`CHROMA_FMAX`] has a continuous
//! pitch class `p = (69 + 12·log2(f / 440)) mod 12` (0 = C, equal
//! temperament, A4 = 440 Hz); its magnitude `|X|` is split linearly between
//! the two nearest classes `⌊p⌋` and `⌊p⌋ + 1`. This is the pitch-class
//! profile of T. Fujishima, "Realtime chord recognition of musical sound: a
//! system using Common Lisp Music" (ICMC 1999), as also used by M. Bartsch
//! & G. Wakefield, "Audio thumbnailing of popular music using chroma-based
//! representations" (IEEE Trans. Multimedia 7(1), 2005), with soft instead
//! of hard bin assignment because bins are coarse (10.8 Hz) at low
//! frequencies. Below 100 Hz a semitone is narrower than a bin, so those
//! bins are left out (bass notes still contribute through their harmonics).

/// Lowest bin frequency mapped to chroma (Hz).
pub const CHROMA_FMIN: f32 = 100.0;
/// Highest bin frequency mapped to chroma (Hz).
pub const CHROMA_FMAX: f32 = 5000.0;
/// Pitch classes (C, C♯, …, B).
pub const N_CHROMA: usize = 12;

/// Precomputed bin → pitch-class weights for one `(sample_rate, n_fft)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromaMap {
    sample_rate: u32,
    n_fft: usize,
    first_bin: usize,
    /// Per bin from `first_bin`: (lower class, weight of the lower class).
    entries: Vec<(u8, f32)>,
}

impl ChromaMap {
    /// Map for spectrogram frames of an `n_fft`-point STFT at `sample_rate`.
    pub fn new(sample_rate: u32, n_fft: usize) -> Self {
        let df = sample_rate as f32 / n_fft.max(1) as f32;
        let n_bins = n_fft / 2 + 1;
        let first_bin = ((CHROMA_FMIN / df).ceil() as usize).max(1);
        let mut entries = Vec::new();
        for k in first_bin..n_bins {
            let f = k as f32 * df;
            if f > CHROMA_FMAX {
                break;
            }
            let p = (69.0 + 12.0 * (f / 440.0).log2()).rem_euclid(12.0);
            let lo = p.floor();
            entries.push(((lo as u8) % 12, 1.0 - (p - lo)));
        }
        Self {
            sample_rate,
            n_fft,
            first_bin,
            entries,
        }
    }

    /// Sample rate the map was built for.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// FFT size the map was built for.
    pub fn n_fft(&self) -> usize {
        self.n_fft
    }

    /// Chroma of one power-spectrum frame (`n_fft / 2 + 1` bins), L1
    /// normalized; `false` (and `out` all zero) when the frame has no energy
    /// in the chroma range.
    pub fn frame(&self, power: &[f32], out: &mut [f32; N_CHROMA]) -> bool {
        *out = [0.0; N_CHROMA];
        let bins = power.get(self.first_bin..).unwrap_or(&[]);
        for (&p, &(lo, w)) in bins.iter().zip(&self.entries) {
            let m = p.max(0.0).sqrt();
            let lo = usize::from(lo);
            out[lo] += w * m;
            out[(lo + 1) % N_CHROMA] += (1.0 - w) * m;
        }
        let sum: f32 = out.iter().sum();
        if sum > 0.0 && sum.is_finite() {
            for c in out.iter_mut() {
                *c /= sum;
            }
            true
        } else {
            *out = [0.0; N_CHROMA];
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tests::{sine, SR};
    use crate::dsp::{Stft, N_FFT};

    fn mean_chroma(x: &[f32]) -> [f32; N_CHROMA] {
        let spec = Stft::new(N_FFT, 512)
            .expect("stft")
            .power(x, SR)
            .expect("power");
        let map = ChromaMap::new(SR, N_FFT);
        let mut acc = [0.0f32; N_CHROMA];
        let mut frame = [0.0f32; N_CHROMA];
        for p in spec.frames() {
            if map.frame(p, &mut frame) {
                for (a, c) in acc.iter_mut().zip(frame) {
                    *a += c;
                }
            }
        }
        acc
    }

    fn argmax(v: &[f32]) -> usize {
        (0..v.len()).fold(0, |b, i| if v[i] > v[b] { i } else { b })
    }

    #[test]
    fn sines_land_on_their_pitch_class() {
        // A4 → 9, C5 (523.25 Hz) → 0, E4 (329.63 Hz) → 4, G3 (196 Hz) → 7.
        for (f, pc) in [(440.0, 9), (523.251, 0), (329.628, 4), (195.998, 7)] {
            let c = mean_chroma(&sine(f, 0.5, SR as usize));
            assert_eq!(argmax(&c), pc, "{f} Hz: {c:?}");
        }
    }

    #[test]
    fn frames_are_l1_normalized_and_silence_is_empty() {
        let map = ChromaMap::new(SR, N_FFT);
        assert_eq!((map.sample_rate(), map.n_fft()), (SR, N_FFT));
        let mut out = [1.0f32; N_CHROMA];
        assert!(!map.frame(&vec![0.0; N_FFT / 2 + 1], &mut out));
        assert_eq!(out, [0.0; N_CHROMA]);
        assert!(!map.frame(&[], &mut out));
        assert!(map.frame(&vec![1.0; N_FFT / 2 + 1], &mut out));
        assert!((out.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(!map.frame(&vec![f32::INFINITY; N_FFT / 2 + 1], &mut out));
    }
}

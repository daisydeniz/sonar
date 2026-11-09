//! Integer PCM: the exact samples a lossless fixture stores. The lossless
//! writers (AIFF, FLAC, ALAC) encode a [`Pcm`]; decoders must return these
//! values bit-exactly (as `q / 2^(bits−1)` in float).

use super::wav::{quantize, Levels, WavSpec};

/// Interleaved signed integer samples plus their format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm {
    /// Frames per second.
    pub sample_rate: u32,
    /// Interleaved channel count (≥ 1).
    pub channels: u16,
    /// Bits per sample (16 or 24 in the fixture set).
    pub bits: u16,
    /// Interleaved samples, each within the signed `bits` range.
    pub samples: Vec<i32>,
}

impl Pcm {
    /// Quantises interleaved `samples` (nominal −1..1) for `spec` with the
    /// WAV writer's rule (`round(x·(2^(b−1)−1))`, clamped; non-finite → 0).
    /// Float specs are stored as 24-bit integers. A trailing partial frame
    /// is dropped.
    pub fn quantize(spec: &WavSpec, samples: &[f32]) -> Self {
        let bits = spec.sample_format.bits().min(24);
        let ch = usize::from(spec.channels.max(1));
        let usable = samples.len() / ch * ch;
        let samples = samples[..usable]
            .iter()
            .map(|&x| quantize(if x.is_finite() { x } else { 0.0 }, u32::from(bits)))
            .collect();
        Self {
            sample_rate: spec.sample_rate,
            channels: spec.channels.max(1),
            bits,
            samples,
        }
    }

    /// Number of frames.
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// Samples of channel `c` (0-based), in time order.
    pub fn channel(&self, c: usize) -> impl Iterator<Item = i32> + '_ {
        let ch = usize::from(self.channels.max(1));
        self.samples.iter().skip(c).step_by(ch).copied()
    }

    /// One `Vec` per channel.
    pub fn planes(&self) -> Vec<Vec<i32>> {
        (0..usize::from(self.channels.max(1)))
            .map(|c| self.channel(c).collect())
            .collect()
    }

    /// Full-scale divisor `2^(bits−1)`.
    pub fn scale(&self) -> f64 {
        f64::from(1u32 << (self.bits - 1))
    }

    /// Peak/RMS with ints scaled by `1/2^(bits−1)` (as in the WAV writer).
    pub fn levels(&self) -> Levels {
        let scale = self.scale();
        let (mut peak, mut sum_sq) = (0.0f64, 0.0f64);
        for &q in &self.samples {
            let v = f64::from(q) / scale;
            peak = peak.max(v.abs());
            sum_sq += v * v;
        }
        let rms = if self.samples.is_empty() {
            0.0
        } else {
            (sum_sq / self.samples.len() as f64).sqrt()
        };
        Levels { peak, rms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::wav::SampleFormat;

    #[test]
    fn quantize_matches_wav_rule_and_splits_channels() {
        let spec = WavSpec::new(48_000, 2, SampleFormat::Int16);
        let pcm = Pcm::quantize(&spec, &[1.0, -1.0, 0.5, f32::NAN, 0.25]);
        assert_eq!(pcm.samples, [32_767, -32_767, 16_384, 0]);
        assert_eq!(pcm.frames(), 2);
        assert_eq!(pcm.planes(), [vec![32_767, 16_384], vec![-32_767, 0]]);
        let lv = pcm.levels();
        assert_eq!(lv.peak, 32_767.0 / 32_768.0);
    }

    #[test]
    fn float_specs_become_24_bit() {
        let spec = WavSpec::new(44_100, 1, SampleFormat::Float32);
        let pcm = Pcm::quantize(&spec, &[0.5]);
        assert_eq!((pcm.bits, pcm.samples[0]), (24, 4_194_304));
        assert_eq!(pcm.scale(), 8_388_608.0);
    }
}

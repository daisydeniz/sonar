//! Band-limited, deterministic sample-rate conversion to the analysis rate.
//!
//! Uses rubato's synchronous FFT resampler (`rubato::Fft`): each block is
//! transformed, its spectrum truncated/zero-extended to the target length
//! and multiplied by a Blackman–Harris anti-aliasing window, then inverse
//! transformed and overlap-added (classic fast-convolution / spectral
//! resampling, cf. J. O. Smith, *Spectral Audio Signal Processing*,
//! "Overlap-Add STFT Processing"). Fixed ratio, no randomness: the same
//! input gives bit-identical output on the same platform.
//!
//! Alignment: rubato's `process_all_into_buffer` trims the resampler's
//! startup delay and flushes the tail with silence, so output frame `k`
//! corresponds to input time `k / to_rate` (verified by the tests below)
//! and the output length is `ceil(len · to / from)`.

use std::collections::HashMap;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Requested chunk size (input frames per `process` call).
const CHUNK: usize = 4096;
/// Sub-chunks per chunk: FFT blocks of ~1024 input frames. Larger blocks
/// move the anti-aliasing cutoff closer to Nyquist: measured cutoffs for
/// 44.1/48/96/192 kHz → 22.05 kHz are 10.7/10.8/10.5/10.0 kHz (0.97–0.91 of
/// the 11.025 kHz target Nyquist); the delay (trimmed anyway) stays < 15 ms.
const SUB_CHUNKS: usize = 4;
/// Distinct (from, to) pairs kept alive; libraries rarely mix more rates.
const MAX_CACHED: usize = 8;

/// Converts mono buffers between sample rates, caching one resampler (FFT
/// plans and scratch buffers) per rate pair so a worker thread can reuse
/// them across files.
#[derive(Default)]
pub struct MonoResampler {
    cache: HashMap<(u32, u32), Fft<f32>>,
}

impl MonoResampler {
    /// Resamples `input` (mono, `from` Hz) to `to` Hz and returns at most
    /// `max_out` frames. Returns an error message if rubato rejects the
    /// rates (both must be > 0).
    pub fn resample(
        &mut self,
        input: &[f32],
        from: u32,
        to: u32,
        max_out: usize,
    ) -> Result<Vec<f32>, String> {
        if from == to {
            return Ok(input[..input.len().min(max_out)].to_vec());
        }
        if input.is_empty() {
            return Ok(Vec::new());
        }
        let resampler = self.get(from, to)?;
        resampler.reset();
        let len = input.len();
        let mut out = vec![0.0f32; resampler.process_all_needed_output_len(len)];
        let produced = {
            let src = InterleavedSlice::new(input, 1, len).map_err(|e| e.to_string())?;
            let out_frames = out.len();
            let mut dst =
                InterleavedSlice::new_mut(&mut out, 1, out_frames).map_err(|e| e.to_string())?;
            let (_, produced) = resampler
                .process_all_into_buffer(&src, &mut dst, len, None)
                .map_err(|e| format!("resampling {from} -> {to} Hz: {e}"))?;
            produced
        };
        out.truncate(produced.min(max_out));
        Ok(out)
    }

    /// Input frames the resampler transforms at a time for `from → to`
    /// (its FFT block; 0 when no resampling happens). Output near the end
    /// of the input depends on up to one block of later input, so callers
    /// that trim the output decode at least this much past the trim point.
    pub fn input_block(&mut self, from: u32, to: u32) -> Result<usize, String> {
        if from == to {
            return Ok(0);
        }
        Ok(self.get(from, to)?.fft_size_in())
    }

    /// Cached resampler for the rate pair (built on first use).
    fn get(&mut self, from: u32, to: u32) -> Result<&mut Fft<f32>, String> {
        if self.cache.len() >= MAX_CACHED && !self.cache.contains_key(&(from, to)) {
            self.cache.clear();
        }
        Ok(match self.cache.entry((from, to)) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(
                build(from, to).map_err(|e| format!("resampler setup {from} -> {to} Hz: {e}"))?,
            ),
        })
    }
}

/// Mono FFT resampler with [`SUB_CHUNKS`] blocks per chunk (rubato's
/// `Fft::new` picks ~256-frame blocks; ~1024 puts the cutoff closer to
/// Nyquist).
fn build(from: u32, to: u32) -> Result<Fft<f32>, rubato::ResamplerConstructionError> {
    Fft::<f32>::new_custom(
        from as usize,
        to as usize,
        CHUNK,
        SUB_CHUNKS,
        1,
        rubato::WindowFunction::BlackmanHarris2,
        FixedSync::Input,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn sine(freq: f64, rate: u32, frames: usize, amp: f64) -> Vec<f32> {
        (0..frames)
            .map(|i| (amp * (TAU * freq * i as f64 / f64::from(rate)).sin()) as f32)
            .collect()
    }

    /// Magnitude of the DFT bin at `freq` (Goertzel-style direct sum),
    /// normalised so a sine of amplitude `a` gives `a`.
    fn tone_amp(x: &[f32], freq: f64, rate: u32) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, s) in x.iter().enumerate() {
            let ph = TAU * freq * i as f64 / f64::from(rate);
            re += f64::from(*s) * ph.cos();
            im += f64::from(*s) * ph.sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f64
    }

    #[test]
    fn same_rate_is_a_capped_copy() {
        let mut r = MonoResampler::default();
        let x = vec![0.1, 0.2, 0.3];
        assert_eq!(r.resample(&x, 22_050, 22_050, 10).unwrap(), x);
        assert_eq!(r.resample(&x, 22_050, 22_050, 2).unwrap(), [0.1, 0.2]);
        assert!(r.resample(&[], 44_100, 22_050, 10).unwrap().is_empty());
    }

    #[test]
    fn output_length_is_ceil_of_ratio() {
        let mut r = MonoResampler::default();
        for (from, len) in [
            (44_100u32, 44_100usize),
            (48_000, 48_000),
            (96_000, 12_345),
            (8_000, 8_001),
            (44_101, 5_000),
            (48_000, 1),
            (88_200, 3),
        ] {
            let x = vec![0.0f32; len];
            let y = r.resample(&x, from, 22_050, usize::MAX).unwrap();
            let want = (len as u64 * 22_050).div_ceil(u64::from(from)) as usize;
            assert_eq!(y.len(), want, "{from} Hz, {len} frames");
        }
    }

    #[test]
    fn max_out_caps_output() {
        let mut r = MonoResampler::default();
        let y = r
            .resample(&vec![0.0; 48_000], 48_000, 22_050, 1_000)
            .unwrap();
        assert_eq!(y.len(), 1_000);
    }

    #[test]
    fn sine_keeps_frequency_amplitude_and_phase() {
        let mut r = MonoResampler::default();
        for from in [44_100u32, 48_000, 96_000, 16_000] {
            let x = sine(440.0, from, from as usize, 0.5);
            let y = r.resample(&x, from, 22_050, usize::MAX).unwrap();
            assert_eq!(y.len(), 22_050);
            let want = sine(440.0, 22_050, 22_050, 0.5);
            // Ignore 20 ms at each edge (band-limited flush of a hard cut).
            let mid = 441..22_050 - 441;
            let err = y[mid.clone()]
                .iter()
                .zip(&want[mid.clone()])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(err < 1e-4, "{from} Hz: max error {err}");
            let amp = tone_amp(&y[mid.clone()], 440.0, 22_050);
            assert!((amp - 0.5).abs() < 1e-3, "{from} Hz: amplitude {amp}");
            let off = tone_amp(&y[mid], 450.0, 22_050);
            assert!(off < 0.02, "{from} Hz: energy off-frequency {off}");
        }
    }

    #[test]
    fn click_stays_time_aligned() {
        let mut r = MonoResampler::default();
        for from in [44_100u32, 48_000, 96_000] {
            let mut x = vec![0.0f32; from as usize];
            let at = from as usize / 2; // 0.5 s
            x[at] = 1.0;
            let y = r.resample(&x, from, 22_050, usize::MAX).unwrap();
            let peak = y
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(i, _)| i)
                .unwrap();
            let want = 22_050 / 2;
            assert!(peak.abs_diff(want) <= 1, "{from} Hz: peak at {peak}");
        }
    }

    #[test]
    fn content_above_target_nyquist_is_removed() {
        let mut r = MonoResampler::default();
        // 15 kHz would alias to 7.05 kHz at 22.05 kHz without filtering.
        let x = sine(15_000.0, 48_000, 48_000, 0.5);
        let y = r.resample(&x, 48_000, 22_050, usize::MAX).unwrap();
        let alias = tone_amp(&y[441..21_609], 7_050.0, 22_050);
        assert!(alias < 1e-6, "alias amplitude {alias}");
    }

    #[test]
    fn deterministic_and_cache_reuse_is_clean() {
        let mut r = MonoResampler::default();
        let x = sine(1_000.0, 44_100, 10_000, 0.7);
        let a = r.resample(&x, 44_100, 22_050, usize::MAX).unwrap();
        let _ = r.resample(&sine(3.0, 44_100, 777, 1.0), 44_100, 22_050, usize::MAX);
        let b = r.resample(&x, 44_100, 22_050, usize::MAX).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn input_block_reports_fft_size() {
        let mut r = MonoResampler::default();
        assert_eq!(r.input_block(22_050, 22_050).unwrap(), 0);
        assert_eq!(r.input_block(44_100, 22_050).unwrap(), 1_024);
        assert_eq!(r.input_block(48_000, 22_050).unwrap(), 1_280);
        assert_eq!(r.input_block(8_000, 22_050).unwrap(), 1_120);
        assert!(r.input_block(0, 22_050).is_err());
    }

    #[test]
    fn invalid_rate_is_an_error_not_a_panic() {
        let mut r = MonoResampler::default();
        assert!(r.resample(&[0.0; 8], 0, 22_050, 8).is_err());
    }
}

//! CLAP audio front end (ADR-026): mono 48 kHz samples -> the
//! `[1001][64]` log-mel input of the HTSAT audio tower. It reproduces the
//! non-fusion path of the HF `ClapFeatureExtractor` (Wu et al. 2023,
//! arXiv:2211.06687) with one deliberate difference: a clip longer than
//! 10 s is cropped at its start, not at a random offset, so an embedding is
//! reproducible.
//!
//! - Clip: 480,000 samples (10 s). Longer input: the first 480,000 samples.
//!   Shorter input: "repeatpad", tiled `floor(480000 / len)` times, then
//!   zero-padded. Empty input is an error.
//! - STFT: 1024-point FFT, periodic Hann, hop 480, centred with reflect
//!   padding of 512 per side (1001 frames), power spectrum.
//! - Mel: 64 Slaney bands, 50-14,000 Hz ([`super::mel`]).
//! - `10 * log10(max(mel, 1e-10))`, no further normalisation (the model has
//!   its own BatchNorm). Time-major layout, 1.8 ms per clip on one core.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

use super::mel::{filterbank, Band};
use crate::{Error, Result};

/// Sample rate the audio tower expects (Hz).
pub const SAMPLE_RATE: u32 = 48_000;
/// FFT size.
pub const N_FFT: usize = 1024;
/// FFT bins of a real transform of [`N_FFT`] points.
pub const N_BINS: usize = N_FFT / 2 + 1;
/// STFT hop (samples).
pub const HOP: usize = 480;
/// Mel bands.
pub const N_MELS: usize = 64;
/// Clip length (samples): 10 s at 48 kHz.
pub const CLIP_SAMPLES: usize = 480_000;
/// STFT frames of a clip: `CLIP_SAMPLES / HOP + 1`.
pub const FRAMES: usize = CLIP_SAMPLES / HOP + 1;
/// Values of one clip's feature matrix (`FRAMES * N_MELS`).
pub const FEATURE_LEN: usize = FRAMES * N_MELS;
/// Lowest mel band edge (Hz).
pub const FMIN: f64 = 50.0;
/// Highest mel band edge (Hz).
pub const FMAX: f64 = 14_000.0;
/// Floor of the power before the logarithm (-100 dB).
const POWER_FLOOR: f32 = 1e-10;

/// Reusable front end: FFT plan, window, filterbank and scratch buffers.
/// One per thread; not shared.
pub struct FrontEnd {
    bands: Vec<Band>,
    window: Vec<f32>,
    fft: Arc<dyn RealToComplex<f32>>,
    clip: Vec<f32>,
    padded: Vec<f32>,
    frame: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    power: Vec<f32>,
}

impl std::fmt::Debug for FrontEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrontEnd").finish_non_exhaustive()
    }
}

impl Default for FrontEnd {
    fn default() -> Self {
        Self::new()
    }
}

impl FrontEnd {
    /// Builds the plan, window and filterbank.
    pub fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT);
        let window = (0..N_FFT)
            .map(|n| (0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / N_FFT as f64).cos()) as f32)
            .collect();
        Self {
            bands: filterbank(),
            window,
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            clip: vec![0.0; CLIP_SAMPLES],
            padded: vec![0.0; CLIP_SAMPLES + N_FFT],
            frame: vec![0.0; N_FFT],
            power: vec![0.0; N_BINS],
        }
    }

    /// Features of mono 48 kHz `wave` as a new vector of [`FEATURE_LEN`].
    ///
    /// # Errors
    /// [`Error::Embed`] if `wave` is empty.
    pub fn features(&mut self, wave: &[f32]) -> Result<Vec<f32>> {
        let mut out = vec![0.0; FEATURE_LEN];
        self.compute(wave, &mut out)?;
        Ok(out)
    }

    /// Writes the [`FEATURE_LEN`] log-mel values (dB, time-major) of mono
    /// 48 kHz `wave` into `out`. Non-finite samples count as silence.
    ///
    /// # Errors
    /// [`Error::Embed`] if `wave` is empty or `out` has the wrong length.
    pub fn compute(&mut self, wave: &[f32], out: &mut [f32]) -> Result<()> {
        if wave.is_empty() {
            return Err(embed_err("empty audio cannot be embedded"));
        }
        if out.len() != FEATURE_LEN {
            return Err(embed_err("feature buffer has the wrong length"));
        }
        assemble_clip(wave, &mut self.clip);
        // Centre reflect padding (numpy "reflect": the edge sample is not
        // repeated).
        let half = N_FFT / 2;
        let last = CLIP_SAMPLES - 1;
        for (i, p) in self.padded.iter_mut().enumerate() {
            let idx = if i < half {
                half - i
            } else if i - half > last {
                2 * last - (i - half)
            } else {
                i - half
            };
            *p = self.clip[idx];
        }
        for (t, row) in out.as_chunks_mut::<N_MELS>().0.iter_mut().enumerate() {
            let start = t * HOP;
            for ((f, &x), &w) in self
                .frame
                .iter_mut()
                .zip(&self.padded[start..start + N_FFT])
                .zip(&self.window)
            {
                *f = x * w;
            }
            self.fft
                .process_with_scratch(&mut self.frame, &mut self.spectrum, &mut self.scratch)
                .map_err(|e| embed_err(format!("FFT failed: {e}")))?;
            for (p, c) in self.power.iter_mut().zip(&self.spectrum) {
                *p = c.re * c.re + c.im * c.im;
            }
            for (v, band) in row.iter_mut().zip(&self.bands) {
                let powers = &self.power[band.first..band.first + band.weights.len()];
                let acc: f32 = band.weights.iter().zip(powers).map(|(w, p)| w * p).sum();
                *v = 10.0 * acc.max(POWER_FLOOR).log10();
            }
        }
        Ok(())
    }
}

/// Deterministic crop / "repeatpad" of `wave` into the 10 s `clip`.
/// `wave` must not be empty. Non-finite samples become 0.
pub(super) fn assemble_clip(wave: &[f32], clip: &mut [f32]) {
    debug_assert_eq!(clip.len(), CLIP_SAMPLES);
    clip.fill(0.0);
    let n = wave.len().min(CLIP_SAMPLES);
    let repeats = CLIP_SAMPLES / n;
    for r in 0..repeats {
        let dst = &mut clip[r * n..(r + 1) * n];
        for (d, &s) in dst.iter_mut().zip(wave) {
            *d = if s.is_finite() { s } else { 0.0 };
        }
    }
}

fn embed_err(reason: impl Into<String>) -> Error {
    Error::Embed {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, secs: f32) -> Vec<f32> {
        let n = (secs * SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    /// Mean dB per band over all frames.
    fn band_means(feats: &[f32]) -> Vec<f32> {
        (0..N_MELS)
            .map(|m| feats.iter().skip(m).step_by(N_MELS).sum::<f32>() / FRAMES as f32)
            .collect()
    }

    fn argmax(v: &[f32]) -> usize {
        (0..v.len()).fold(0, |b, i| if v[i] > v[b] { i } else { b })
    }

    #[test]
    fn geometry_matches_the_spec() {
        assert_eq!(FRAMES, 1001);
        assert_eq!(FEATURE_LEN, 1001 * 64);
        let mut fe = FrontEnd::new();
        let feats = fe.features(&sine(440.0, 1.0)).expect("features");
        assert_eq!(feats.len(), 1001 * 64);
        assert!(feats.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn silence_is_the_minus_100_db_floor() {
        let mut fe = FrontEnd::new();
        let feats = fe.features(&vec![0.0; 1000]).expect("features");
        assert!(feats.iter().all(|&v| (v + 100.0).abs() < 1e-4));
    }

    #[test]
    fn a_sine_lands_in_the_mel_band_that_contains_it() {
        let bands = filterbank();
        let mut fe = FrontEnd::new();
        let mut last = None;
        // Exact FFT bin centres (k * 46.875 Hz): 468.75, 1500, 4218.75, 9000.
        for (bin, hz) in [
            (10usize, 468.75),
            (32, 1500.0),
            (90, 4218.75),
            (192, 9000.0),
        ] {
            let means = band_means(&fe.features(&sine(hz, 10.0)).expect("features"));
            let got = argmax(&means);
            let want = (0..N_MELS)
                .max_by(|&a, &b| {
                    let w = |m: usize| {
                        let band = &bands[m];
                        bin.checked_sub(band.first)
                            .and_then(|j| band.weights.get(j))
                            .copied()
                            .unwrap_or(0.0)
                    };
                    w(a).total_cmp(&w(b))
                })
                .expect("bands");
            assert!(
                got.abs_diff(want) <= 1,
                "{hz} Hz: band {got}, expected {want}"
            );
            // Strictly above the previous (lower) tone's band, and a clear
            // peak over bands far away.
            assert!(
                last.is_none_or(|l| got > l),
                "{hz} Hz: band {got} after {last:?}"
            );
            last = Some(got);
            let far = means[(got + 20) % N_MELS].min(means[(got + N_MELS - 20) % N_MELS]);
            assert!(means[got] > far + 30.0, "{hz} Hz: {} vs {far}", means[got]);
        }
    }

    #[test]
    fn layout_is_time_major_and_frames_follow_the_signal() {
        // 2 s of tone, then silence: first ~200 frames hot, the last cold.
        let mut fe = FrontEnd::new();
        let mut wave = sine(1500.0, 2.0);
        wave.resize(CLIP_SAMPLES, 0.0);
        let feats = fe.features(&wave).expect("features");
        let frame_max = |t: usize| {
            feats[t * N_MELS..(t + 1) * N_MELS]
                .iter()
                .fold(f32::MIN, |a, &b| a.max(b))
        };
        assert!(frame_max(50) > -20.0, "{}", frame_max(50));
        assert!(frame_max(900) < -90.0, "{}", frame_max(900));
    }

    #[test]
    fn repeatpad_tiles_then_zero_pads() {
        let wave: Vec<f32> = (1..=100_000).map(|i| i as f32).collect();
        let mut clip = vec![-1.0; CLIP_SAMPLES];
        assemble_clip(&wave, &mut clip);
        for r in 0..4 {
            assert_eq!(&clip[r * 100_000..(r + 1) * 100_000], &wave[..], "tile {r}");
        }
        assert!(clip[400_000..].iter().all(|&v| v == 0.0));

        // A clip that divides the length evenly leaves no padding.
        let even = vec![0.25; 120_000];
        assemble_clip(&even, &mut clip);
        assert!(clip.iter().all(|&v| v == 0.25));

        // Longer than 10 s: the first 10 s, deterministically.
        let long: Vec<f32> = (0..CLIP_SAMPLES + 5_000).map(|i| i as f32).collect();
        assemble_clip(&long, &mut clip);
        assert_eq!(&clip[..], &long[..CLIP_SAMPLES]);
    }

    #[test]
    fn tiled_input_equals_explicitly_tiled_input() {
        let mut fe = FrontEnd::new();
        let short = sine(700.0, 0.5);
        let tiled: Vec<f32> = short
            .iter()
            .cycle()
            .take(short.len() * (CLIP_SAMPLES / short.len()))
            .copied()
            .collect();
        let a = fe.features(&short).expect("short");
        let b = fe.features(&tiled).expect("tiled");
        assert_eq!(a, b);
    }

    #[test]
    fn empty_input_is_rejected_and_bad_samples_are_silence() {
        let mut fe = FrontEnd::new();
        assert!(matches!(fe.features(&[]), Err(Error::Embed { .. })));
        let mut out = vec![0.0; 10];
        assert!(matches!(
            fe.compute(&[0.1], &mut out),
            Err(Error::Embed { .. })
        ));
        let feats = fe.features(&[f32::NAN, f32::INFINITY, 0.0]).expect("nan");
        assert!(feats.iter().all(|&v| (v + 100.0).abs() < 1e-4));
    }
}

//! Signal building blocks for fixtures: buffers, oscillators, envelopes,
//! noise, one-pole filters and mixing. Everything is deterministic given
//! the [`SplitMix64`] stream passed in.

use std::f64::consts::{FRAC_PI_4, TAU};

use super::rng::SplitMix64;

/// Number of frames for `secs` seconds at `sample_rate`, rounded to nearest.
pub fn frames_for(secs: f64, sample_rate: u32) -> usize {
    (secs * f64::from(sample_rate)).round().max(0.0) as usize
}

/// dB → linear amplitude: `10^(dB/20)`.
pub fn db_to_amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Planar multi-channel buffer (one `Vec` per channel, equal lengths).
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// Channel data.
    pub channels: Vec<Vec<f32>>,
}

impl Audio {
    /// `frames` of digital silence on `channels` channels.
    pub fn silent(channels: u16, frames: usize) -> Self {
        Self {
            channels: vec![vec![0.0; frames]; usize::from(channels.max(1))],
        }
    }

    /// Copies a mono signal to every channel.
    pub fn from_mono(mono: Vec<f32>, channels: u16) -> Self {
        let n = usize::from(channels.max(1));
        let mut chans = vec![mono.clone(); n - 1];
        chans.push(mono);
        Self { channels: chans }
    }

    /// Frames per channel.
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// Largest absolute sample.
    pub fn peak(&self) -> f32 {
        self.channels
            .iter()
            .flatten()
            .fold(0.0f32, |m, x| m.max(x.abs()))
    }

    /// Scales so the absolute peak equals `dbfs`; silence is left untouched.
    pub fn normalize_peak(&mut self, dbfs: f64) {
        let peak = self.peak();
        if peak > 0.0 {
            let g = (db_to_amp(dbfs) / f64::from(peak)) as f32;
            self.channels.iter_mut().flatten().for_each(|x| *x *= g);
        }
    }

    /// Interleaved samples (frame-major), as stored in a WAV `data` chunk.
    pub fn interleave(&self) -> Vec<f32> {
        let frames = self.frames();
        let mut out = Vec::with_capacity(frames * self.channels.len());
        for i in 0..frames {
            out.extend(self.channels.iter().map(|c| c[i]));
        }
        out
    }
}

/// Additive oscillator `Σ a_k · sin(k·θ)`, `θ = 2π·f0·t`, times `env(t)`.
///
/// Partials at or above 0.45·fs are dropped (no aliasing). Harmonics use the
/// Chebyshev recurrence `sin(kθ) = 2·cos θ·sin((k−1)θ) − sin((k−2)θ)`, so a
/// frame costs one `sin_cos` regardless of partial count. Phase is computed
/// from the frame index (no accumulated drift), so pitch is exact.
pub fn additive(
    frames: usize,
    sample_rate: u32,
    f0: f64,
    partials: &[f64],
    env: impl Fn(f64) -> f64,
) -> Vec<f32> {
    let sr = f64::from(sample_rate);
    let usable = partials
        .iter()
        .enumerate()
        .take_while(|(k, _)| (*k as f64 + 1.0) * f0 < 0.45 * sr)
        .count();
    let amps = &partials[..usable];
    (0..frames)
        .map(|i| {
            let t = i as f64 / sr;
            let (s1, c) = (TAU * (f0 * t).fract()).sin_cos();
            let (mut prev, mut cur, mut acc) = (0.0, s1, 0.0);
            for (k, a) in amps.iter().enumerate() {
                if k > 0 {
                    let next = 2.0 * c * cur - prev;
                    prev = cur;
                    cur = next;
                }
                acc += a * cur;
            }
            (acc * env(t)) as f32
        })
        .collect()
}

/// Band-limited sawtooth partial amplitudes `1/k`, k = 1..=n (Fourier
/// series of a sawtooth up to the sign convention).
pub fn saw_partials(n: usize) -> Vec<f64> {
    (1..=n).map(|k| 1.0 / k as f64).collect()
}

/// Linear attack over `attack` s, then exponential decay `e^(−(t−a)/τ)`.
pub fn attack_decay(t: f64, attack: f64, tau: f64) -> f64 {
    if t < attack {
        t / attack
    } else {
        (-(t - attack) / tau).exp()
    }
}

/// Linear fade-in and fade-out of `fade` s at both ends of a sound lasting
/// `total` s (removes start/end clicks). `fade` is capped at `total/2`.
pub fn fades(t: f64, total: f64, fade: f64) -> f64 {
    let f = fade.min(total / 2.0);
    if f <= 0.0 {
        return 1.0;
    }
    (t / f).min((total - t) / f).clamp(0.0, 1.0)
}

/// Uniform white noise in `[-1, 1)`.
pub fn white(frames: usize, rng: &mut SplitMix64) -> Vec<f32> {
    (0..frames).map(|_| rng.next_bipolar() as f32).collect()
}

/// Paul Kellet's "refined" pink-noise filter (music-dsp mailing list,
/// 1999, archived as `pink.txt` at musicdsp.org): seven first-order
/// sections approximating a −3 dB/octave slope within ±0.05 dB above
/// 9.2 Hz at 44.1 kHz.
#[derive(Debug, Clone, Default)]
pub struct PinkFilter {
    b: [f64; 7],
}

impl PinkFilter {
    /// Filters one white sample into one pink sample (gain ≈ 1/0.11 above
    /// unity; callers normalise).
    pub fn process(&mut self, white: f64) -> f64 {
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + white * 0.055_517_9;
        b[1] = 0.99332 * b[1] + white * 0.075_075_9;
        b[2] = 0.96900 * b[2] + white * 0.153_852_0;
        b[3] = 0.86650 * b[3] + white * 0.310_485_6;
        b[4] = 0.55000 * b[4] + white * 0.532_952_2;
        b[5] = -0.7616 * b[5] - white * 0.016_898_0;
        let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + white * 0.5362;
        b[6] = white * 0.115_926;
        pink
    }
}

/// Pink noise (white noise through [`PinkFilter`]), roughly ±1.
pub fn pink(frames: usize, rng: &mut SplitMix64) -> Vec<f32> {
    let mut f = PinkFilter::default();
    (0..frames)
        .map(|_| (f.process(rng.next_bipolar()) * 0.11) as f32)
        .collect()
}

/// One-pole high-pass in place: `x − LP(x)` with the low-pass
/// `y[n] = (1−a)·x[n] + a·y[n−1]`, `a = e^(−2π·fc/fs)`.
pub fn highpass(buf: &mut [f32], cutoff_hz: f64, sample_rate: u32) {
    let a = (-TAU * cutoff_hz / f64::from(sample_rate)).exp();
    let mut lp = 0.0f64;
    for x in buf.iter_mut() {
        let v = f64::from(*x);
        lp = (1.0 - a) * v + a * lp;
        *x = (v - lp) as f32;
    }
}

/// Adds `src · gain` into `dst` from `offset`. Samples past the end wrap to
/// the start when `wrap` (seamless loops), otherwise they are dropped.
pub fn mix_into(dst: &mut [f32], src: &[f32], offset: usize, gain: f32, wrap: bool) {
    let n = dst.len();
    if n == 0 {
        return;
    }
    for (i, s) in src.iter().enumerate() {
        let mut j = offset + i;
        if j >= n {
            if !wrap {
                break;
            }
            j %= n;
        }
        dst[j] += s * gain;
    }
}

/// Equal-power pan gains `(left, right)` for `pan` in −1 (left)..1 (right):
/// `θ = (pan+1)·π/4`, gains `(cos θ, sin θ)`.
pub fn pan_gains(pan: f64) -> (f32, f32) {
    let theta = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (theta.cos() as f32, theta.sin() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frequency estimate from rising zero crossings (sign changes − → +).
    fn zero_crossing_hz(x: &[f32], sample_rate: u32) -> f64 {
        let ups: Vec<usize> = x
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
            .map(|(i, _)| i)
            .collect();
        let (Some(first), Some(last)) = (ups.first(), ups.last()) else {
            return 0.0;
        };
        (ups.len() - 1) as f64 * f64::from(sample_rate) / (last - first) as f64
    }

    #[test]
    fn sine_frequency_by_zero_crossings() {
        for (f, sr) in [(440.0, 44_100), (55.0, 48_000), (2093.0, 96_000)] {
            let x = additive(sr as usize, sr, f, &[1.0], |_| 1.0);
            let est = zero_crossing_hz(&x, sr);
            assert!((est - f).abs() / f < 1e-3, "{f} Hz @ {sr}: got {est}");
        }
    }

    #[test]
    fn additive_matches_direct_sum() {
        let p = saw_partials(5);
        let x = additive(512, 44_100, 300.0, &p, |_| 1.0);
        for (i, v) in x.iter().enumerate() {
            let th = TAU * 300.0 * i as f64 / 44_100.0;
            let direct: f64 = p
                .iter()
                .enumerate()
                .map(|(k, a)| a * ((k as f64 + 1.0) * th).sin())
                .sum();
            assert!((f64::from(*v) - direct).abs() < 1e-5);
        }
    }

    #[test]
    fn partials_above_nyquist_are_dropped() {
        // f0 = 10 kHz at 44.1 kHz: only k = 1 is below 0.45·fs.
        let a = additive(256, 44_100, 10_000.0, &saw_partials(8), |_| 1.0);
        let b = additive(256, 44_100, 10_000.0, &[1.0], |_| 1.0);
        assert_eq!(a, b);
    }

    #[test]
    fn envelopes() {
        assert_eq!(attack_decay(0.0, 0.01, 0.1), 0.0);
        assert!((attack_decay(0.11, 0.01, 0.1) - (-1.0f64).exp()).abs() < 1e-12);
        assert_eq!(fades(0.0, 1.0, 0.1), 0.0);
        assert_eq!(fades(0.5, 1.0, 0.1), 1.0);
        assert!(fades(0.95, 1.0, 0.1) < 0.51);
        assert_eq!(fades(0.3, 1.0, 0.0), 1.0);
    }

    #[test]
    fn pink_has_less_high_frequency_energy_than_white() {
        // For white noise, var(Δx)/var(x) ≈ 2; a −3 dB/oct spectrum is far lower.
        fn diff_ratio(x: &[f32]) -> f64 {
            let e: f64 = x.iter().map(|v| f64::from(*v).powi(2)).sum();
            let d: f64 = x.windows(2).map(|w| f64::from(w[1] - w[0]).powi(2)).sum();
            d / e
        }
        let mut rng = SplitMix64::new(1);
        let w = white(44_100, &mut rng);
        let p = pink(44_100, &mut rng);
        assert!(
            (diff_ratio(&w) - 2.0).abs() < 0.1,
            "white {}",
            diff_ratio(&w)
        );
        assert!(diff_ratio(&p) < 0.5, "pink {}", diff_ratio(&p));
    }

    #[test]
    fn highpass_removes_dc() {
        let mut x = vec![1.0f32; 48_000];
        highpass(&mut x, 1_000.0, 48_000);
        assert!(x[47_999].abs() < 1e-6);
    }

    #[test]
    fn mix_wraps_or_drops() {
        let mut d = [0.0f32; 4];
        mix_into(&mut d, &[1.0, 2.0, 3.0], 2, 1.0, true);
        assert_eq!(d, [3.0, 0.0, 1.0, 2.0]);
        let mut d = [0.0f32; 4];
        mix_into(&mut d, &[1.0, 2.0, 3.0], 2, 2.0, false);
        assert_eq!(d, [0.0, 0.0, 2.0, 4.0]);
    }

    #[test]
    fn audio_helpers() {
        let mut a = Audio::from_mono(vec![0.5, -0.25], 2);
        assert_eq!(a.interleave(), vec![0.5, 0.5, -0.25, -0.25]);
        a.normalize_peak(0.0);
        assert!((a.peak() - 1.0).abs() < 1e-6);
        let (l, r) = pan_gains(0.0);
        assert!((l - r).abs() < 1e-6 && (l * l + r * r - 1.0).abs() < 1e-6);
        assert_eq!(frames_for(0.01, 44_100), 441);
        assert_eq!(Audio::silent(1, 0).frames(), 0);
    }
}

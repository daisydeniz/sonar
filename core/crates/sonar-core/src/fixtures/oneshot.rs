//! Synthetic drum one-shots: kick (pitch-swept sine), snare (tone body +
//! noise) and hi-hats (high-passed noise). Used directly as one-shot
//! fixtures and as the voices of [`drums`](super::drums) loops.

use super::manifest::Category;
use super::rng::SplitMix64;
use super::synth::{additive, attack_decay, frames_for, highpass, white};
use std::f64::consts::TAU;

/// Kick: sine with an exponential pitch sweep
/// `f(t) = f_end + (f_start − f_end)·e^(−t/τ_f)` and amplitude `e^(−t/τ_a)`.
/// Phase is the closed-form integral
/// `φ(t) = 2π·[f_end·t + (f_start − f_end)·τ_f·(1 − e^(−t/τ_f))]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickParams {
    /// Start frequency (Hz).
    pub f_start: f64,
    /// End frequency (Hz).
    pub f_end: f64,
    /// Pitch sweep time constant (s).
    pub sweep_tau: f64,
    /// Amplitude decay time constant (s).
    pub decay_tau: f64,
    /// Length (s).
    pub secs: f64,
}

/// Snare: decaying sine body plus high-passed decaying white noise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnareParams {
    /// Body frequency (Hz).
    pub body_hz: f64,
    /// Body decay time constant (s).
    pub body_tau: f64,
    /// Noise decay time constant (s).
    pub noise_tau: f64,
    /// Noise high-pass cutoff (Hz).
    pub noise_hp_hz: f64,
    /// Length (s).
    pub secs: f64,
}

/// Hi-hat: white noise through two one-pole high-passes, decaying.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatParams {
    /// High-pass cutoff (Hz).
    pub hp_hz: f64,
    /// Decay time constant (s).
    pub decay_tau: f64,
    /// Length (s).
    pub secs: f64,
}

/// A drum one-shot voice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DrumHit {
    /// Kick drum.
    Kick(KickParams),
    /// Snare drum.
    Snare(SnareParams),
    /// Closed hi-hat.
    HatClosed(HatParams),
    /// Open hi-hat.
    HatOpen(HatParams),
}

/// Kick voice used in loops.
pub const LOOP_KICK: KickParams = KickParams {
    f_start: 150.0,
    f_end: 50.0,
    sweep_tau: 0.03,
    decay_tau: 0.12,
    secs: 0.45,
};
/// Snare voice used in loops.
pub const LOOP_SNARE: SnareParams = SnareParams {
    body_hz: 185.0,
    body_tau: 0.06,
    noise_tau: 0.09,
    noise_hp_hz: 1_500.0,
    secs: 0.3,
};
/// Closed hat used in loops.
pub const LOOP_HAT_CLOSED: HatParams = HatParams {
    hp_hz: 7_000.0,
    decay_tau: 0.02,
    secs: 0.08,
};
/// Open hat used in loops.
pub const LOOP_HAT_OPEN: HatParams = HatParams {
    hp_hz: 6_000.0,
    decay_tau: 0.12,
    secs: 0.35,
};

impl DrumHit {
    /// Category truth for this voice.
    pub fn category(&self) -> Category {
        match self {
            Self::Kick(_) => Category::Kick,
            Self::Snare(_) => Category::Snare,
            Self::HatClosed(_) => Category::HatClosed,
            Self::HatOpen(_) => Category::HatOpen,
        }
    }

    /// Renders the hit (mono, unnormalised).
    pub fn render(&self, sample_rate: u32, rng: &mut SplitMix64) -> Vec<f32> {
        match *self {
            Self::Kick(p) => kick(sample_rate, &p),
            Self::Snare(p) => snare(sample_rate, &p, rng),
            Self::HatClosed(p) | Self::HatOpen(p) => hat(sample_rate, &p, rng),
        }
    }
}

fn kick(sample_rate: u32, p: &KickParams) -> Vec<f32> {
    let sr = f64::from(sample_rate);
    let df = p.f_start - p.f_end;
    (0..frames_for(p.secs, sample_rate))
        .map(|i| {
            let t = i as f64 / sr;
            let phase = TAU * (p.f_end * t + df * p.sweep_tau * (1.0 - (-t / p.sweep_tau).exp()));
            (phase.sin() * attack_decay(t, 0.001, p.decay_tau)) as f32
        })
        .collect()
}

fn snare(sample_rate: u32, p: &SnareParams, rng: &mut SplitMix64) -> Vec<f32> {
    let n = frames_for(p.secs, sample_rate);
    let body = additive(n, sample_rate, p.body_hz, &[1.0, 0.0, 0.3], |t| {
        attack_decay(t, 0.001, p.body_tau)
    });
    let mut noise = white(n, rng);
    highpass(&mut noise, p.noise_hp_hz, sample_rate);
    let sr = f64::from(sample_rate);
    body.iter()
        .zip(&noise)
        .enumerate()
        .map(|(i, (b, w))| {
            let env = attack_decay(i as f64 / sr, 0.001, p.noise_tau) as f32;
            0.6 * b + 0.8 * w * env
        })
        .collect()
}

fn hat(sample_rate: u32, p: &HatParams, rng: &mut SplitMix64) -> Vec<f32> {
    let mut x = white(frames_for(p.secs, sample_rate), rng);
    highpass(&mut x, p.hp_hz, sample_rate);
    highpass(&mut x, p.hp_hz, sample_rate);
    let sr = f64::from(sample_rate);
    for (i, v) in x.iter_mut().enumerate() {
        *v *= attack_decay(i as f64 / sr, 0.0005, p.decay_tau) as f32;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_sweeps_down_and_decays() {
        let k = kick(44_100, &LOOP_KICK);
        assert_eq!(k.len(), frames_for(LOOP_KICK.secs, 44_100));
        let head = k[..4_410].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let tail = k[k.len() - 4_410..]
            .iter()
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(head > 0.5 && tail < 0.1 * head);
    }

    #[test]
    fn hats_are_bright_and_noise_is_seeded() {
        let a = DrumHit::HatClosed(LOOP_HAT_CLOSED).render(48_000, &mut SplitMix64::new(1));
        let b = DrumHit::HatClosed(LOOP_HAT_CLOSED).render(48_000, &mut SplitMix64::new(1));
        let c = DrumHit::HatClosed(LOOP_HAT_CLOSED).render(48_000, &mut SplitMix64::new(2));
        assert_eq!(a, b);
        assert_ne!(a, c);
        // High-passed noise: sign flips on most adjacent samples.
        let flips = a.windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        assert!(flips * 2 > a.len(), "flips {flips} of {}", a.len());
    }

    #[test]
    fn categories() {
        assert_eq!(DrumHit::Snare(LOOP_SNARE).category(), Category::Snare);
        assert_eq!(
            DrumHit::HatOpen(LOOP_HAT_OPEN).category(),
            Category::HatOpen
        );
    }
}

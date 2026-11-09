//! Synthetic drum loops (patterns over the [`oneshot`](super::oneshot)
//! voices) and metronome click loops. Loops are rendered on an exact grid (no
//! swing/humanisation) so the tempo truth is exact, and hits that ring past
//! the loop end wrap to the start so the loop is seamless.

use super::oneshot::{DrumHit, LOOP_HAT_CLOSED, LOOP_HAT_OPEN, LOOP_KICK, LOOP_SNARE};
use super::rng::SplitMix64;
use super::synth::{additive, attack_decay, frames_for, mix_into, pan_gains, Audio};

/// 4/4 drum pattern on a 16-step (sixteenth-note) grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    /// House: kick every beat, snare on 2 & 4, open hats on the off-beats.
    FourOnFloor,
    /// Rock/pop: kick 1, 3, "and" of 3; snare 2 & 4; closed hats on 8ths.
    Backbeat,
    /// Break / two-step (also DnB at 160–175): kick 1 and "and" of 3,
    /// snare 2 & 4, closed 8ths, ghost snares.
    Breakbeat,
    /// Half-time feel: kick on 1 and the "a" of 1 (step 3), snare on 3,
    /// closed 16ths (perceived tempo may be half of the truth — counts as an
    /// octave error in eval).
    HalfTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Voice {
    Kick,
    Snare,
    Closed,
    Open,
}

impl Voice {
    /// Render order (fixed: it determines which PRNG draws each voice gets).
    const ALL: [Self; 4] = [Self::Kick, Self::Snare, Self::Closed, Self::Open];

    /// One-shot used for this voice and its pan (−1 left … 1 right).
    fn hit_and_pan(self) -> (DrumHit, f64) {
        match self {
            Self::Kick => (DrumHit::Kick(LOOP_KICK), 0.0),
            Self::Snare => (DrumHit::Snare(LOOP_SNARE), -0.1),
            Self::Closed => (DrumHit::HatClosed(LOOP_HAT_CLOSED), 0.4),
            Self::Open => (DrumHit::HatOpen(LOOP_HAT_OPEN), 0.4),
        }
    }
}

impl Pattern {
    /// `(step 0..16, voice, velocity)` hits of one bar.
    fn hits(self) -> Vec<(u32, Voice, f32)> {
        use Voice::*;
        let mut h = Vec::new();
        match self {
            Self::FourOnFloor => {
                h.extend([0, 4, 8, 12].map(|s| (s, Kick, 1.0)));
                h.extend([4, 12].map(|s| (s, Snare, 0.8)));
                h.extend([2, 6, 10, 14].map(|s| (s, Open, 0.5)));
            }
            Self::Backbeat => {
                h.extend([0, 8, 10].map(|s| (s, Kick, 1.0)));
                h.extend([4, 12].map(|s| (s, Snare, 0.9)));
                h.extend((0..16).step_by(2).map(|s| (s, Closed, 0.5)));
            }
            Self::Breakbeat => {
                h.extend([0, 10].map(|s| (s, Kick, 1.0)));
                h.extend([4, 12].map(|s| (s, Snare, 0.9)));
                h.extend([7, 15].map(|s| (s, Snare, 0.25)));
                h.extend((0..16).step_by(2).map(|s| (s, Closed, 0.45)));
            }
            Self::HalfTime => {
                h.extend([0, 3].map(|s| (s, Kick, 1.0)));
                h.push((8, Snare, 1.0));
                h.extend((0..16).map(|s| (s, Closed, if s % 2 == 0 { 0.45 } else { 0.3 })));
            }
        }
        h
    }
}

/// Drum loop of `bars` 4/4 bars at `bpm`; length is exactly
/// `round(bars·4·60/bpm·fs)` frames. Hats are panned right in stereo.
pub fn drum_loop(
    sample_rate: u32,
    channels: u16,
    bpm: f64,
    bars: u32,
    pattern: Pattern,
    rng: &mut SplitMix64,
) -> Audio {
    render_hits(sample_rate, channels, bpm, bars, &pattern.hits(), rng)
}

/// Renders one bar of `(step, voice, velocity)` hits, repeated `bars` times.
fn render_hits(
    sample_rate: u32,
    channels: u16,
    bpm: f64,
    bars: u32,
    hits: &[(u32, Voice, f32)],
    rng: &mut SplitMix64,
) -> Audio {
    let step_s = 60.0 / bpm / 4.0;
    let total = frames_for(f64::from(bars * 16) * step_s, sample_rate);
    let mut out = Audio::silent(channels, total);
    let voices = Voice::ALL.map(|v| {
        let (hit, pan) = v.hit_and_pan();
        (v, hit.render(sample_rate, rng), pan)
    });
    for bar in 0..bars {
        for &(step, voice, vel) in hits {
            let Some((_, sample, pan)) = voices.iter().find(|(v, _, _)| *v == voice) else {
                continue;
            };
            let at = frames_for(f64::from(bar * 16 + step) * step_s, sample_rate);
            place(&mut out, sample, at, vel, *pan);
        }
    }
    out
}

/// Metronome loop: one 25 ms sine click per beat, accented (1760 Hz,
/// louder) on beat 1 and 880 Hz otherwise. Length is exactly
/// `round(bars·beats_per_bar·60/bpm·fs)` frames.
pub fn click_loop(
    sample_rate: u32,
    channels: u16,
    bpm: f64,
    bars: u32,
    beats_per_bar: u32,
) -> Audio {
    let beat_s = 60.0 / bpm;
    let beats = bars * beats_per_bar;
    let total = frames_for(f64::from(beats) * beat_s, sample_rate);
    let mut out = Audio::silent(channels, total);
    let n = frames_for(0.025, sample_rate);
    let env = |t: f64| attack_decay(t, 0.0005, 0.006);
    let accent = additive(n, sample_rate, 1_760.0, &[1.0], env);
    let normal = additive(n, sample_rate, 880.0, &[1.0], env);
    for b in 0..beats {
        let (click, gain) = if b % beats_per_bar == 0 {
            (&accent, 1.0)
        } else {
            (&normal, 0.6)
        };
        let at = frames_for(f64::from(b) * beat_s, sample_rate);
        place(&mut out, click, at, gain, 0.0);
    }
    out
}

/// Mixes a mono hit into every channel (equal-power pan in stereo), wrapping.
fn place(out: &mut Audio, sample: &[f32], at: usize, gain: f32, pan: f64) {
    let (l, r) = pan_gains(pan);
    let n = out.channels.len();
    for (c, ch) in out.channels.iter_mut().enumerate() {
        let g = match (n, c) {
            (1, _) => gain,
            (_, 0) => gain * l,
            _ => gain * r,
        };
        mix_into(ch, sample, at, g, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn onsets(x: &[f32], threshold: f32) -> Vec<usize> {
        // First frame of each region above threshold, after ≥ 20 ms below it.
        let mut out = Vec::new();
        let mut last_loud: Option<usize> = None;
        for (i, v) in x.iter().enumerate() {
            if v.abs() > threshold {
                if last_loud.is_none_or(|l| i - l > 882) {
                    out.push(i);
                }
                last_loud = Some(i);
            }
        }
        out
    }

    #[test]
    fn loop_length_is_exact() {
        let mut rng = SplitMix64::new(3);
        for (bpm, bars, sr) in [
            (122.5, 4u32, 44_100u32),
            (70.0, 2, 48_000),
            (175.0, 4, 96_000),
        ] {
            let a = drum_loop(sr, 2, bpm, bars, Pattern::Backbeat, &mut rng);
            let exact = f64::from(bars * 4) * 60.0 / bpm * f64::from(sr);
            assert!(
                (a.frames() as f64 - exact).abs() <= 0.5,
                "{bpm}: {}",
                a.frames()
            );
        }
        let c = click_loop(44_100, 1, 95.0, 4, 3);
        let exact = 12.0 * 60.0 / 95.0 * 44_100.0;
        assert!((c.frames() as f64 - exact).abs() <= 0.5);
    }

    #[test]
    fn clicks_land_on_the_beat_grid() {
        let c = click_loop(44_100, 1, 120.0, 1, 4);
        let found = onsets(&c.channels[0], 0.01);
        assert_eq!(found.len(), 4, "{found:?}");
        for (got, want) in found.iter().zip([0, 22_050, 44_100, 66_150]) {
            assert!(got - want < 44, "onset {got} vs beat {want}"); // < 1 ms
        }
    }

    #[test]
    fn kicks_land_on_the_beat_grid() {
        let (sr, bpm, bars) = (44_100u32, 122.5, 2u32);
        let kicks: Vec<_> = Pattern::FourOnFloor
            .hits()
            .into_iter()
            .filter(|h| h.1 == Voice::Kick)
            .collect();
        let a = render_hits(sr, 1, bpm, bars, &kicks, &mut SplitMix64::new(4));
        let found = onsets(&a.channels[0], 0.05);
        assert_eq!(found.len(), (bars * 4) as usize, "{found:?}");
        for (k, got) in found.iter().enumerate() {
            let want = (k as f64 * 60.0 / bpm * f64::from(sr)).round() as usize;
            assert!(*got >= want && got - want < 44, "kick {k}: {got} vs {want}");
            // < 1 ms
        }
    }

    #[test]
    fn stereo_loop_pans_hats() {
        let a = drum_loop(
            44_100,
            2,
            120.0,
            1,
            Pattern::Backbeat,
            &mut SplitMix64::new(9),
        );
        assert_ne!(a.channels[0], a.channels[1]);
    }
}

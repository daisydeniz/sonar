//! Renders a catalog [`Content`] to audio and its (signal-independent)
//! truth. Level truth (`peak_dbfs`, `rms_dbfs`, `is_silent`) is added later
//! from the quantised samples actually written.

use std::f64::consts::TAU;

use super::drums::{click_loop, drum_loop};
use super::manifest::{Category, Kind, NoiseColor, Truth};
use super::music::{chord_loop, midi_to_hz, note_name};
use super::rng::SplitMix64;
use super::spec::Content;
use super::synth::{additive, attack_decay, db_to_amp, fades, frames_for, pink, white, Audio};
use super::wav::WavSpec;

/// Beats per bar of every drum and chord loop (4/4).
const FOUR_FOUR: u32 = 4;

/// Renders `content` for `spec` (rate, channel count). Noise and drum
/// content draws from `rng`; everything else is fully determined by the
/// content parameters.
pub fn render(content: &Content, spec: &WavSpec, rng: &mut SplitMix64) -> (Audio, Truth) {
    let sr = spec.sample_rate;
    let ch = spec.channels;
    let mono = |x: Vec<f32>| Audio::from_mono(x, ch);
    match *content {
        Content::Sine { midi, secs, kind } => {
            let n = frames_for(secs, sr);
            let total = n as f64 / f64::from(sr);
            let fade = (secs / 4.0).min(0.01);
            let x = additive(n, sr, hz(midi), &[1.0], |t| fades(t, total, fade));
            (mono(x), note_truth(midi, kind, Category::Sine))
        }
        Content::Pluck { midi, secs } => {
            let n = frames_for(secs, sr);
            let total = n as f64 / f64::from(sr);
            let partials = [1.0, 0.5, 0.33, 0.25, 0.2, 0.16];
            let x = additive(n, sr, hz(midi), &partials, |t| {
                attack_decay(t, 0.003, 0.35) * ((total - t) / 0.01).min(1.0)
            });
            (mono(x), note_truth(midi, Kind::OneShot, Category::Synth))
        }
        Content::Pad { midi, secs } => {
            let n = frames_for(secs, sr);
            let total = n as f64 / f64::from(sr);
            let x = additive(n, sr, hz(midi), &[1.0, 0.3, 0.1], |t| {
                fades(t, total, 2.0) * (0.75 + 0.25 * (TAU * 0.2 * t).sin())
            });
            (mono(x), note_truth(midi, Kind::Tone, Category::Synth))
        }
        Content::ClippedSine {
            midi,
            secs,
            drive_db,
        } => {
            let g = db_to_amp(drive_db);
            let x = additive(frames_for(secs, sr), sr, hz(midi), &[g], |_| 1.0)
                .into_iter()
                .map(|v| v.clamp(-1.0, 1.0))
                .collect();
            (mono(x), note_truth(midi, Kind::Tone, Category::Sine))
        }
        Content::Chords { key, bpm, bars } => {
            let (audio, chords) = chord_loop(sr, ch, key, bpm, bars);
            let mut t = loop_truth(Category::Synth, bpm, bars, FOUR_FOUR);
            t.key = Some(key.truth());
            t.chords = Some(chords);
            (audio, t)
        }
        Content::DrumLoop { bpm, bars, pattern } => {
            let audio = drum_loop(sr, ch, bpm, bars, pattern, rng);
            (audio, loop_truth(Category::Drums, bpm, bars, FOUR_FOUR))
        }
        Content::ClickLoop {
            bpm,
            bars,
            beats_per_bar,
        } => {
            let audio = click_loop(sr, ch, bpm, bars, beats_per_bar);
            (audio, loop_truth(Category::Click, bpm, bars, beats_per_bar))
        }
        Content::Hit(hit) => {
            let truth = Truth {
                kind: Some(Kind::OneShot),
                category: Some(hit.category()),
                ..Truth::default()
            };
            (mono(hit.render(sr, rng)), truth)
        }
        Content::Noise { color, secs, burst } => {
            let n = frames_for(secs, sr);
            let mut x = match color {
                NoiseColor::White => white(n, rng),
                NoiseColor::Pink => pink(n, rng),
            };
            if let Some((start, len)) = burst {
                gate(&mut x, sr, start, len);
            }
            let truth = Truth {
                kind: Some(Kind::Noise),
                category: Some(Category::Noise),
                noise_color: Some(color),
                ..Truth::default()
            };
            (mono(x), truth)
        }
        Content::Silence { frames } => {
            let truth = Truth {
                kind: Some(Kind::Silence),
                ..Truth::default()
            };
            (Audio::silent(ch, frames), truth)
        }
        Content::Impulse => {
            let truth = Truth {
                kind: Some(Kind::OneShot),
                ..Truth::default()
            };
            (mono(vec![0.5]), truth)
        }
    }
}

fn hz(midi: u8) -> f64 {
    midi_to_hz(f64::from(midi))
}

/// Keeps `x` only inside `[start, start+len)` s, with 2 ms linear edges.
fn gate(x: &mut [f32], sample_rate: u32, start: f64, len: f64) {
    let sr = f64::from(sample_rate);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f64 / sr - start;
        let g = if (0.0..len).contains(&t) {
            fades(t, len, 0.002)
        } else {
            0.0
        };
        *v *= g as f32;
    }
}

fn note_truth(midi: u8, kind: Kind, category: Category) -> Truth {
    Truth {
        kind: Some(kind),
        category: Some(category),
        note_midi: Some(midi),
        note_name: Some(note_name(midi)),
        pitch_hz: Some((hz(midi) * 1e4).round() / 1e4),
        ..Truth::default()
    }
}

fn loop_truth(category: Category, bpm: f64, bars: u32, beats_per_bar: u32) -> Truth {
    Truth {
        kind: Some(Kind::Loop),
        category: Some(category),
        bpm: Some(bpm),
        bars: Some(bars),
        beats_per_bar: Some(beats_per_bar),
        ..Truth::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::drums::Pattern;
    use crate::fixtures::wav::SampleFormat;

    const SPEC: WavSpec = WavSpec::new(44_100, 1, SampleFormat::Int16);

    fn run(c: Content) -> (Audio, Truth) {
        render(&c, &SPEC, &mut SplitMix64::new(5))
    }

    #[test]
    fn sine_truth_and_length() {
        let (a, t) = run(Content::Sine {
            midi: 70,
            secs: 0.005,
            kind: Kind::OneShot,
        });
        assert_eq!(a.frames(), frames_for(0.005, 44_100));
        assert_eq!(t.note_name.as_deref(), Some("Bb4"));
        assert_eq!(t.pitch_hz, Some(466.1638));
        assert_eq!(t.kind, Some(Kind::OneShot));
    }

    #[test]
    fn loop_truth_matches_length() {
        let (a, t) = run(Content::DrumLoop {
            bpm: 122.5,
            bars: 4,
            pattern: Pattern::FourOnFloor,
        });
        assert_eq!(a.frames(), 345_600); // 16 beats · 60/122.5 s · 44.1 kHz
        assert_eq!(
            (t.bpm, t.bars, t.beats_per_bar),
            (Some(122.5), Some(4), Some(4))
        );
    }

    #[test]
    fn noise_burst_is_gated() {
        let (a, t) = run(Content::Noise {
            color: NoiseColor::White,
            secs: 1.0,
            burst: Some((0.3, 0.2)),
        });
        let x = &a.channels[0];
        assert!(x[..13_000].iter().all(|v| *v == 0.0));
        assert!(x[22_100..].iter().all(|v| *v == 0.0));
        assert!(x[13_500..21_500].iter().any(|v| v.abs() > 0.5));
        assert_eq!(t.noise_color, Some(NoiseColor::White));
    }

    #[test]
    fn clipped_sine_has_flat_tops() {
        let (a, _) = run(Content::ClippedSine {
            midi: 57,
            secs: 0.1,
            drive_db: 6.0,
        });
        let at_rail = a.channels[0].iter().filter(|v| v.abs() == 1.0).count();
        assert!(at_rail > 1_000, "{at_rail}");
    }

    #[test]
    fn silence_and_impulse() {
        let (a, t) = run(Content::Silence { frames: 0 });
        assert_eq!(a.frames(), 0);
        assert_eq!(t.kind, Some(Kind::Silence));
        let (a, _) = run(Content::Impulse);
        assert_eq!(a.channels, vec![vec![0.5]]);
    }
}

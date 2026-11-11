//! Note (MIDI pitch) of tonal one-shots from YIN f0 tracking
//! ([`crate::dsp::yin`]; de Cheveigné & Kawahara 2002).
//!
//! 1. **Region**: from the first sample within [`START_DB`] of the peak, at
//!    most [`NOTE_MAX_S`] (the note of a one-shot is established early; this
//!    bounds the cost on 30 s windows). Signals shorter than one YIN frame
//!    get a tracker sized to them (window = half the signal), so 5–15 ms
//!    blips still have a pitch.
//! 2. **Frames**: YIN 30 Hz–4.2 kHz (B0..C8), window 1024 (46 ms), hop 512,
//!    threshold [`crate::dsp::yin::YIN_THRESHOLD`]. Frames more than
//!    [`ACTIVE_DB`] below the loudest are ignored (decay tails, reverb).
//! 3. **Pitch**: energy-weighted median of the voiced frames' MIDI value
//!    `69 + 12·log2(f0/440)`; then the energy-weighted mean of the frames
//!    within ±0.5 semitone of it (the stable part) is rounded to the note.
//!    YIN's first-dip rule avoids sub-octave errors; a minority of octave-up
//!    frames cannot move a median.
//! 4. **Gate**: the stable frames must carry at least
//!    [`MIN_STABLE`] of the active energy. A pitch sweep (kick drums: the
//!    loud attack sweeps down, only the quiet tail is steady), noise
//!    (snares, hats: unvoiced) and silence give no note; a sustained tuned
//!    808 does get one.
//!
//! **Confidence** = stable share × (1 − mean aperiodicity of the stable
//! frames) × (1 − |deviation from the note| in semitones).

use serde::Serialize;

use crate::dsp::yin::{Yin, YinConfig, YinFrame, YIN_THRESHOLD};
use crate::Result;

/// Lowest detectable f0 (Hz), just below B0.
pub const NOTE_FMIN: f32 = 30.0;
/// Highest detectable f0 (Hz), just above C8 (4186 Hz).
pub const NOTE_FMAX: f32 = 4300.0;
/// YIN integration window (samples at 22.05 kHz: 46 ms).
pub const NOTE_WINDOW: usize = 1024;
/// Hop between YIN frames (samples).
pub const NOTE_HOP: usize = 512;
/// Longest stretch analysed (s).
pub const NOTE_MAX_S: f32 = 3.0;
/// The region starts at the first sample within this many dB of the peak.
pub const START_DB: f32 = 40.0;
/// Frames more than this many dB below the loudest frame are ignored.
pub const ACTIVE_DB: f32 = 30.0;
/// Minimum share of the active energy in stable voiced frames.
pub const MIN_STABLE: f32 = 0.6;
/// Frames within this many semitones of the median count as stable.
const STABLE_SEMITONES: f32 = 0.5;

/// Detected note of one file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Note {
    /// MIDI note number (69 = A4).
    pub midi: u8,
    /// Deviation of the measured pitch from `midi` (cents, `−50..=50`).
    pub cents: f32,
    /// Measured fundamental (Hz).
    pub f0_hz: f32,
    /// `0..=1` (see the module docs).
    pub confidence: f32,
}

impl Note {
    /// Pitch class (0 = C).
    pub fn pitch_class(&self) -> u8 {
        self.midi % 12
    }

    /// Scientific pitch name with the manifest's spelling (`C#4`, `Bb2`).
    pub fn name(&self) -> String {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
        ];
        let octave = i32::from(self.midi / 12) - 1;
        format!("{}{octave}", NAMES[usize::from(self.midi % 12)])
    }
}

/// Reusable note detector for one sample rate (owns a [`Yin`] tracker:
/// keep one per worker thread).
#[derive(Debug)]
pub struct NoteTracker {
    yin: Yin,
    weights: Vec<(f32, f32)>,
}

impl NoteTracker {
    /// Tracker for signals at `sample_rate` (normally
    /// [`crate::decode::ANALYSIS_RATE`]). Fails if the rate cannot resolve
    /// [`NOTE_FMAX`] (`sample_rate / NOTE_FMAX < 2`).
    pub fn new(sample_rate: u32) -> Result<Self> {
        Ok(Self {
            yin: Yin::new(config(sample_rate, NOTE_FMIN, NOTE_WINDOW))?,
            weights: Vec::new(),
        })
    }

    /// Sample rate this tracker was built for.
    pub fn sample_rate(&self) -> u32 {
        self.yin.config().sample_rate
    }

    /// The note of `samples` (mono, at [`Self::sample_rate`]), `None` when
    /// there is no stable pitch. Errors only on internal FFT failures.
    pub fn detect(&mut self, samples: &[f32]) -> Result<Option<Note>> {
        let sr = self.sample_rate();
        let Some(region) = region(samples, sr) else {
            return Ok(None);
        };
        let frames = if region.len() >= self.yin.frame_len() {
            self.yin.track(region)?
        } else {
            // Shorter than one frame: a tracker sized to the signal.
            let w = region.len() / 2;
            let max_lag = region.len().saturating_sub(w + 2);
            if max_lag < 8 {
                return Ok(None);
            }
            let fmin = (sr as f32 / max_lag as f32).max(NOTE_FMIN);
            if fmin * 2.0 >= NOTE_FMAX {
                return Ok(None);
            }
            Yin::new(config(sr, fmin, w))?.track(region)?
        };
        Ok(summarize(&frames, &mut self.weights))
    }
}

fn config(sample_rate: u32, fmin: f32, window: usize) -> YinConfig {
    YinConfig {
        sample_rate,
        fmin,
        fmax: NOTE_FMAX,
        window,
        hop: NOTE_HOP,
        threshold: YIN_THRESHOLD,
    }
}

/// The analysed stretch (see the module docs); `None` for silence.
fn region(samples: &[f32], sample_rate: u32) -> Option<&[f32]> {
    let mag = |x: f32| if x.is_finite() { x.abs() } else { 0.0 };
    let peak = samples.iter().map(|&x| mag(x)).fold(0.0f32, f32::max);
    if peak.is_nan() || peak <= 0.0 {
        return None;
    }
    let thr = peak * 10f32.powf(-START_DB / 20.0);
    let start = samples.iter().position(|&x| mag(x) >= thr)?;
    let len = (NOTE_MAX_S * sample_rate as f32) as usize;
    let end = start.saturating_add(len).min(samples.len());
    samples.get(start..end)
}

/// MIDI value of a frequency.
fn midi_of(f0: f32) -> f32 {
    69.0 + 12.0 * (f0 / 440.0).log2()
}

/// Note from YIN frames (see the module docs); `scratch` is reused.
fn summarize(frames: &[YinFrame], scratch: &mut Vec<(f32, f32)>) -> Option<Note> {
    let loudest = frames.iter().map(|f| f.energy).fold(0.0f32, f32::max);
    if !(loudest > 0.0 && loudest.is_finite()) {
        return None;
    }
    let floor = loudest * 10f32.powf(-ACTIVE_DB / 10.0);
    let active = |f: &&YinFrame| f.energy >= floor;
    let total: f32 = frames.iter().filter(active).map(|f| f.energy).sum();
    scratch.clear();
    scratch.extend(
        frames
            .iter()
            .filter(active)
            .filter(|f| f.voiced && f.f0_hz > 0.0 && f.f0_hz.is_finite())
            .map(|f| (midi_of(f.f0_hz), f.energy)),
    );
    if scratch.is_empty() || total.is_nan() || total <= 0.0 {
        return None;
    }
    let median = weighted_median(scratch)?;
    let (mut w_sum, mut m_sum, mut ap_sum) = (0.0f32, 0.0f32, 0.0f32);
    for f in frames.iter().filter(active).filter(|f| f.voiced) {
        let m = midi_of(f.f0_hz);
        if (m - median).abs() <= STABLE_SEMITONES {
            w_sum += f.energy;
            m_sum += f.energy * m;
            ap_sum += f.energy * f.aperiodicity;
        }
    }
    let stable = w_sum / total;
    if !(stable >= MIN_STABLE && w_sum > 0.0) {
        return None;
    }
    let pitch = m_sum / w_sum;
    let midi = pitch.round();
    if !(0.0..=127.0).contains(&midi) {
        return None;
    }
    let dev = pitch - midi;
    let clarity = 1.0 - (ap_sum / w_sum).clamp(0.0, 1.0);
    let confidence = (stable.min(1.0) * clarity * (1.0 - dev.abs())).clamp(0.0, 1.0);
    Some(Note {
        midi: midi as u8,
        cents: (dev * 100.0).clamp(-50.0, 50.0),
        f0_hz: 440.0 * 2f32.powf((pitch - 69.0) / 12.0),
        confidence,
    })
}

/// Energy-weighted median of `(value, weight)` pairs (sorts in place).
fn weighted_median(v: &mut [(f32, f32)]) -> Option<f32> {
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f32 = v.iter().map(|x| x.1).sum();
    let mut acc = 0.0f32;
    for &(m, w) in v.iter() {
        acc += w;
        if acc >= 0.5 * total {
            return Some(m);
        }
    }
    v.last().map(|x| x.0)
}

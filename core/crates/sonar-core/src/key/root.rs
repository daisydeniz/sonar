//! Root note of pitched loops (T10.3, ADR-041): tom, bass and 808 loops
//! that mostly hit one pitch class get it as a mode-less key candidate.
//!
//! 1. **Onsets**: peaks of the spectral-flux onset envelope
//!    ([`crate::dsp::onset`]) with the usual peak-picking rules (Bello et
//!    al. 2005, "A tutorial on onset detection in music signals", IEEE
//!    TSAP 13(5); Böck, Krebs & Schedl 2012, "Evaluating the online
//!    capabilities of onset detection methods", ISMIR): local maximum over
//!    ±[`PEAK_RADIUS`] frames, above the local mean (±[`AVG_RADIUS`]) and
//!    above [`PEAK_FRACTION`] of the strongest peak, at least [`MIN_GAP`]
//!    frames after the previous onset. The window start is always a
//!    boundary (the flux of frame 0 is 0 by definition).
//! 2. **Segments**: onset to next onset. Each segment's energy (sum of
//!    squares) is its weight; its pitch is the one-shot note of
//!    [`NoteTracker::detect`] (YIN, de Cheveigné & Kawahara 2002, with the
//!    same stability gate) measured from [`ATTACK_SKIP_S`] after the
//!    segment's peak sample (onset frames precede the hit by up to two
//!    hops, and the attack's click and glide carry no stable pitch).
//!    Segments whose sound lasts less than [`MIN_SUSTAIN_S`] after the
//!    peak (metronome clicks, blips) count as unpitched. Unpitched
//!    segments (kicks sweeping, snares, hats, chords without a stable f0)
//!    keep their energy in the total, so they count against every pitch
//!    class.
//! 3. **Root**: the pitch class with the largest energy share. It is a
//!    root only when that share is at least [`MIN_ROOT_SHARE`]; its
//!    confidence is `share × ` the energy-weighted mean note confidence of
//!    its segments (clarity and tuning, see [`super::note`]).
//!
//! [`combine`] fuses the root with the chroma key candidate: same tonic →
//! noisy-or, keeping the chroma mode only when the chroma key has real
//! support ([`AUDIO_SUPPORT`]); different tonic → the stronger wins,
//! discounted by the other (the T10.2 disagreement rule).

use super::fuse::AUDIO_SUPPORT;
use super::note::{NoteTracker, NOTE_WINDOW};
use crate::dsp::Features;
use crate::text::Key;
use crate::Result;

/// Minimum share of the loop's onset-segment energy on the root's pitch
/// class.
pub const MIN_ROOT_SHARE: f32 = 0.6;
/// Half-width (frames) of the local-maximum test (±70 ms at hop 512).
pub const PEAK_RADIUS: usize = 3;
/// Half-width (frames) of the local mean an onset must exceed.
pub const AVG_RADIUS: usize = 10;
/// Onsets weaker than this fraction of the envelope maximum are ignored.
pub const PEAK_FRACTION: f32 = 0.1;
/// Minimum distance (frames) between two onsets.
pub const MIN_GAP: usize = 3;
/// A segment's sound must last this long (s) after its peak, within
/// [`SUSTAIN_DB`], for its pitch to count: metronome clicks and blips are
/// heard as clicks, not as notes that establish a root.
pub const MIN_SUSTAIN_S: f32 = 0.05;
/// Level below the segment peak (dB) that ends its sustain.
pub const SUSTAIN_DB: f32 = 20.0;
/// Time after a segment's peak skipped before its pitch is measured (s):
/// the click and pitch glide of a tom or pluck carry the most energy of a
/// short segment but no stable pitch.
pub const ATTACK_SKIP_S: f32 = 0.02;

/// Root note of a loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Root {
    /// Pitch class (0 = C).
    pub pitch_class: u8,
    /// Share of the segment energy on this pitch class (`0..=1`).
    pub share: f32,
    /// `0..=1` (see the module docs).
    pub confidence: f32,
}

impl Root {
    /// The root as a mode-less key candidate.
    pub fn candidate(&self) -> (Key, f32) {
        let key = Key {
            root: self.pitch_class,
            mode: None,
        };
        (key, self.confidence)
    }
}

/// Onset frames of an onset-strength envelope (see the module docs),
/// ascending.
pub fn onset_frames(env: &[f32]) -> Vec<usize> {
    let max = env
        .iter()
        .copied()
        .filter(|x| x.is_finite())
        .fold(0.0f32, f32::max);
    let mut out: Vec<usize> = Vec::new();
    if max <= 0.0 {
        return out;
    }
    let thr = PEAK_FRACTION * max;
    let val = |x: f32| if x.is_finite() { x } else { 0.0 };
    for (t, &x) in env.iter().enumerate() {
        let x = val(x);
        if x < thr {
            continue;
        }
        let lo = t.saturating_sub(PEAK_RADIUS);
        let hi = (t + PEAK_RADIUS + 1).min(env.len());
        if env[lo..hi].iter().any(|&y| val(y) > x) {
            continue;
        }
        let (alo, ahi) = (
            t.saturating_sub(AVG_RADIUS),
            (t + AVG_RADIUS + 1).min(env.len()),
        );
        let mean = env[alo..ahi].iter().map(|&y| val(y)).sum::<f32>() / (ahi - alo) as f32;
        if x <= mean {
            continue;
        }
        if out.last().is_some_and(|&p| t < p + MIN_GAP) {
            continue;
        }
        out.push(t);
    }
    out
}

/// Root note of a loop (`samples`: the analysis window `features` was
/// computed from, at the tracker's rate); `None` when no pitch class holds
/// [`MIN_ROOT_SHARE`] of the energy.
///
/// # Errors
/// Only internal FFT failures of the note tracker.
pub fn loop_root(
    features: &Features,
    samples: &[f32],
    tracker: &mut NoteTracker,
) -> Result<Option<Root>> {
    let hop = features.spectrogram.hop.max(1);
    let sr = tracker.sample_rate() as f32;
    let (skip, min_sustain) = ((ATTACK_SKIP_S * sr) as usize, (MIN_SUSTAIN_S * sr) as usize);
    let mut bounds: Vec<usize> = Vec::with_capacity(features.onset.len() / MIN_GAP + 2);
    bounds.push(0);
    bounds.extend(
        onset_frames(&features.onset)
            .into_iter()
            .map(|t| t.saturating_mul(hop).min(samples.len())),
    );
    bounds.push(samples.len());
    bounds.dedup();

    let (mut total, mut by_pc, mut conf_pc) = (0.0f64, [0.0f64; 12], [0.0f64; 12]);
    for w in bounds.windows(2) {
        let Some(seg) = samples.get(w[0]..w[1]) else {
            continue;
        };
        let energy: f64 = seg
            .iter()
            .filter(|x| x.is_finite())
            .map(|&x| f64::from(x) * f64::from(x))
            .sum();
        if energy <= 0.0 {
            continue;
        }
        total += energy;
        // Pitch after the attack; shorter than one YIN frame after it: a
        // click or flam, counted as unpitched.
        let peak = peak_index(seg);
        if sustain(seg, peak) < min_sustain {
            continue;
        }
        let Some(body) = seg
            .get(peak.saturating_add(skip)..)
            .filter(|b| b.len() >= NOTE_WINDOW)
        else {
            continue;
        };
        if let Some(note) = tracker.detect(body)? {
            let pc = usize::from(note.pitch_class());
            by_pc[pc] += energy;
            conf_pc[pc] += energy * f64::from(note.confidence);
        }
    }
    if !(total > 0.0 && total.is_finite()) {
        return Ok(None);
    }
    let best = (0..12).fold(0, |b, i| if by_pc[i] > by_pc[b] { i } else { b });
    let share = (by_pc[best] / total) as f32;
    if share.is_nan() || share < MIN_ROOT_SHARE {
        return Ok(None);
    }
    let clarity = (conf_pc[best] / by_pc[best]) as f32;
    Ok(Some(Root {
        pitch_class: best as u8,
        share: share.min(1.0),
        confidence: (share * clarity).clamp(0.0, 1.0),
    }))
}

/// Samples from `peak` to the last sample within [`SUSTAIN_DB`] of it.
fn sustain(seg: &[f32], peak: usize) -> usize {
    let mag = |x: f32| if x.is_finite() { x.abs() } else { 0.0 };
    let Some(&p) = seg.get(peak) else {
        return 0;
    };
    let thr = mag(p) * 10f32.powf(-SUSTAIN_DB / 20.0);
    seg[peak..]
        .iter()
        .rposition(|&x| mag(x) >= thr)
        .unwrap_or(0)
}

/// Index of the largest finite magnitude (0 for an all-zero segment).
fn peak_index(seg: &[f32]) -> usize {
    let mag = |x: f32| if x.is_finite() { x.abs() } else { 0.0 };
    (0..seg.len()).fold(0, |b, i| if mag(seg[i]) > mag(seg[b]) { i } else { b })
}

/// Audio key candidate of a loop from its root and its chroma key (module
/// docs).
pub fn combine(root: Option<Root>, chroma: Option<(Key, f32)>) -> Option<(Key, f32)> {
    let Some(r) = root else {
        return chroma;
    };
    let (rk, rc) = r.candidate();
    let Some((k, a)) = chroma else {
        return Some((rk, rc));
    };
    Some(if k.root % 12 == r.pitch_class {
        let mode = if a >= AUDIO_SUPPORT { k.mode } else { None };
        let key = Key {
            root: r.pitch_class,
            mode,
        };
        (key, 1.0 - (1.0 - a) * (1.0 - rc))
    } else if rc > a {
        (rk, rc * (1.0 - a))
    } else {
        (k, a * (1.0 - rc))
    })
}

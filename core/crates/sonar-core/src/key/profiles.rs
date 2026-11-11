//! Key profiles and the Krumhansl–Schmuckler key-finding score.
//!
//! A key profile gives the expected weight of each pitch class (relative to
//! the tonic) in music of that key. The score of a key is the Pearson
//! correlation between a pitch-class distribution (here a time-averaged
//! chroma) and the profile rotated to the key's tonic (C. Krumhansl,
//! *Cognitive Foundations of Musical Pitch*, OUP 1990, ch. 4 — the
//! Krumhansl–Schmuckler algorithm).
//!
//! Profiles: D. Temperley, "What's key for key? The Krumhansl–Schmuckler
//! key-finding algorithm reconsidered", *Music Perception* 17(1), 1999 —
//! his revision of the Krumhansl & Kessler (1982) probe-tone ratings. It
//! weights the leading tone (major 7th) equally in major and minor (as in
//! the harmonic minor), which separates a minor key from its relative major
//! better than the original ratings ([`KK_MAJOR`] / [`KK_MINOR`] are kept
//! for reference and tests).
//!
//! **Harmonic expansion** (E. Gómez, *Tonal Description of Music Audio
//! Signals*, PhD thesis, UPF 2006, §4.2): an audio chroma also contains the
//! overtones of every note (the 3rd harmonic adds the fifth, the 5th the
//! major third), so each profile is convolved with the pitch classes of the
//! first [`N_HARMONICS`] harmonics of a note, weighted `s^(h−1)` with `s =`
//! [`HARMONIC_DECAY`], before correlating.

use crate::dsp::chroma::N_CHROMA;
use crate::text::{Key, Mode};

/// Krumhansl & Kessler (1982) major-key probe-tone ratings (C = index 0).
pub const KK_MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// Krumhansl & Kessler (1982) minor-key probe-tone ratings.
pub const KK_MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
/// Temperley (1999) major profile.
pub const TEMPERLEY_MAJOR: [f32; 12] = [5.0, 2.0, 3.5, 2.0, 4.5, 4.0, 2.0, 4.5, 2.0, 3.5, 1.5, 4.0];
/// Temperley (1999) minor profile.
pub const TEMPERLEY_MINOR: [f32; 12] = [5.0, 2.0, 3.5, 4.5, 2.0, 4.0, 2.0, 4.5, 3.5, 2.0, 1.5, 4.0];
/// Harmonics per note in the expanded profile (Gómez 2006 uses 4).
pub const N_HARMONICS: u32 = 4;
/// Weight decay per harmonic (Gómez 2006: 0.6).
pub const HARMONIC_DECAY: f32 = 0.6;
/// Number of candidate keys (12 tonics × major/minor).
pub const N_KEYS: usize = 24;

/// Key of candidate index `i` (`tonic·2 + minor`, the search key code).
pub fn key_of_index(i: usize) -> Key {
    Key {
        root: (i / 2 % 12) as u8,
        mode: Some(if i % 2 == 1 { Mode::Minor } else { Mode::Major }),
    }
}

/// `profile` with every pitch class spread over its first [`N_HARMONICS`]
/// harmonics (see the module docs).
pub fn harmonic_expand(profile: &[f32; 12]) -> [f32; 12] {
    let mut out = [0.0f32; 12];
    for (pc, &w) in profile.iter().enumerate() {
        let mut gain = 1.0f32;
        for h in 1..=N_HARMONICS {
            let offset = (12.0 * f64::from(h).log2()).round() as usize;
            out[(pc + offset) % 12] += w * gain;
            gain *= HARMONIC_DECAY;
        }
    }
    out
}

/// Correlation scores of `chroma` with all 24 keys (index = `tonic·2 +
/// minor`), using the harmonic-expanded Temperley profiles. A flat or empty
/// chroma scores 0 everywhere.
pub fn key_scores(chroma: &[f32; N_CHROMA]) -> [f32; N_KEYS] {
    let major = harmonic_expand(&TEMPERLEY_MAJOR);
    let minor = harmonic_expand(&TEMPERLEY_MINOR);
    let mut out = [0.0f32; N_KEYS];
    for tonic in 0..12 {
        out[tonic * 2] = rotated_correlation(chroma, &major, tonic);
        out[tonic * 2 + 1] = rotated_correlation(chroma, &minor, tonic);
    }
    out
}

/// Pearson correlation of `x` with `profile` rotated so its index 0 sits on
/// `tonic`; 0 when either has no variance.
pub fn rotated_correlation(x: &[f32; 12], profile: &[f32; 12], tonic: usize) -> f32 {
    let mx = x.iter().sum::<f32>() / 12.0;
    let mp = profile.iter().sum::<f32>() / 12.0;
    let (mut sxy, mut sxx, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for (pc, &v) in x.iter().enumerate() {
        let dx = v - mx;
        let dy = profile[(pc + 12 - tonic % 12) % 12] - mp;
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    let denom = (sxx * syy).sqrt();
    if denom > 1e-12 && denom.is_finite() {
        (sxy / denom).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

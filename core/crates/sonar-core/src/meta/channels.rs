//! BS.1770-4 channel weights `G_i` from the container's channel layout.
//!
//! Table 3 of BS.1770-4: L, R, C → 1.0; Ls, Rs → 1.41 (+1.5 dB); LFE is
//! not measured (0). Table 4 (advanced systems) gives 1.41 only to
//! channels at 60°–120° azimuth near ear level, 1.0 elsewhere. Mapped onto
//! WAVE-style positions:
//! - `SIDE_LEFT`/`SIDE_RIGHT` → 1.41;
//! - `REAR_LEFT`/`REAR_RIGHT` → 1.41 when they are the only surround pair
//!   (5.1 "back" layouts), 1.0 when side channels also exist (7.1: the
//!   rears sit at ±135°–150°);
//! - `LFE1`/`LFE2` → 0;
//! - everything else (front, centre, wide, height, rear centre) → 1.0.
//!
//! Without positional information (discrete, ambisonic, unknown layouts)
//! every channel weighs 1.0. A mono file is one channel with `G = 1`
//! (literal BS.1770; no dual-mono +3 dB).
//!
//! Guessed layouts count as unknown: a plain (non-extensible) multichannel
//! WAV gets the first `n` WAVE positions from symphonia
//! (`Position::from_wave_channel_count`), e.g. FL FR FC LFE for 4 channels,
//! which would silence channel 4 of a multitrack or ambisonic recording.
//! So a mask equal to that count-derived default is treated as unknown
//! (all 1.0) — except for 6 channels, where the default *is* the canonical
//! 5.1 (FL FR FC LFE RL RR). Cost: an explicit WAVE_FORMAT_EXTENSIBLE mask
//! that happens to equal the default (e.g. 3.1, 4.1) also gets 1.0 for its
//! LFE; symphonia does not tell the two apart.

use symphonia::core::audio::{ChannelLabel, Channels, Position};

/// Weight of a surround channel (BS.1770-4 Table 3).
pub const SURROUND_WEIGHT: f64 = 1.41;

/// Channel count whose count-derived default mask is canonical 5.1.
const CANONICAL_5_1_CHANNELS: usize = 6;

/// `mask` is what a count-only header yields, not a declared layout.
fn is_count_default(mask: Position, count: usize) -> bool {
    count != CANONICAL_5_1_CHANNELS
        && u32::try_from(count)
            .ok()
            .and_then(Position::from_wave_channel_count)
            == Some(mask)
}

/// One weight per decoded plane (`count` planes, in buffer order).
pub(crate) fn channel_weights(layout: Option<&Channels>, count: usize) -> Vec<f64> {
    let positions: Vec<Option<Position>> = match layout {
        Some(Channels::Positioned(mask)) if is_count_default(*mask, count) => Vec::new(),
        // Positioned planes are in ascending bit order (symphonia's
        // canonical buffer order).
        Some(Channels::Positioned(mask)) => mask.iter().map(Some).collect(),
        Some(Channels::Custom(labels)) => labels
            .iter()
            .map(|l| match l {
                ChannelLabel::Positioned(p) => Some(*p),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let all = positions
        .iter()
        .flatten()
        .fold(Position::empty(), |acc, p| acc | *p);
    let has_side = all.intersects(Position::SIDE_LEFT | Position::SIDE_RIGHT);
    (0..count)
        .map(|i| match positions.get(i).copied().flatten() {
            Some(p) => weight(p, has_side),
            None => 1.0,
        })
        .collect()
}

fn weight(p: Position, has_side: bool) -> f64 {
    if p.intersects(Position::LFE1 | Position::LFE2) {
        0.0
    } else if p.intersects(Position::SIDE_LEFT | Position::SIDE_RIGHT)
        || (p.intersects(Position::REAR_LEFT | Position::REAR_RIGHT) && !has_side)
    {
        SURROUND_WEIGHT
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_and_stereo_are_unweighted() {
        let mono = Channels::Positioned(Position::FRONT_CENTER);
        assert_eq!(channel_weights(Some(&mono), 1), [1.0]);
        let st = Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT);
        assert_eq!(channel_weights(Some(&st), 2), [1.0, 1.0]);
        assert_eq!(channel_weights(None, 3), [1.0; 3]);
        assert_eq!(channel_weights(Some(&Channels::Discrete(2)), 2), [1.0; 2]);
    }

    #[test]
    fn surround_layouts_follow_bs1770() {
        let five_one = Position::from_wave_channel_count(6).unwrap(); // FL FR FC LFE RL RR
        let w = channel_weights(Some(&Channels::Positioned(five_one)), 6);
        assert_eq!(w, [1.0, 1.0, 1.0, 0.0, 1.41, 1.41]);
        let seven_one = five_one | Position::SIDE_LEFT | Position::SIDE_RIGHT;
        let w = channel_weights(Some(&Channels::Positioned(seven_one)), 8);
        assert_eq!(w, [1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.41, 1.41]);
        let custom = Channels::Custom(
            vec![
                ChannelLabel::Positioned(Position::SIDE_RIGHT),
                ChannelLabel::Discrete(0),
            ]
            .into(),
        );
        assert_eq!(channel_weights(Some(&custom), 3), [1.41, 1.0, 1.0]);
    }

    #[test]
    fn count_derived_default_masks_are_unknown() {
        for n in [3u32, 4, 5, 7, 8] {
            let mask = Position::from_wave_channel_count(n).unwrap();
            let w = channel_weights(Some(&Channels::Positioned(mask)), n as usize);
            assert!(w.iter().all(|&g| g == 1.0), "{n} ch: {w:?}");
        }
        // An explicit layout that differs from the default keeps its weights.
        let quad = Position::FRONT_LEFT
            | Position::FRONT_RIGHT
            | Position::REAR_LEFT
            | Position::REAR_RIGHT;
        let w = channel_weights(Some(&Channels::Positioned(quad)), 4);
        assert_eq!(w, [1.0, 1.0, 1.41, 1.41]);
    }
}

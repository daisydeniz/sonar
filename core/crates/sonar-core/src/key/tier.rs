//! Confidence tiers of a key (T10.4, ADR-041): the UI shows a key as
//! high / medium / low certainty instead of a continuous fade, and the
//! key filter / facet use medium + high by default.
//!
//! The bounds are fitted on eval data (names hidden) so that the high tier
//! is at least 90 % correct and the medium tier at least 60 % (see
//! `docs/EVAL.md`, "Key calibration"). Both are inclusive lower bounds.

/// Smallest stored key confidence of the high tier (inclusive).
pub const KEY_TIER_HIGH: f32 = 0.75;
/// Smallest stored key confidence of the medium tier (inclusive); below it
/// is the low tier (drums and noise stay there: their confidence is
/// at most 0.24 on the fixtures, ADR-041).
pub const KEY_TIER_MEDIUM: f32 = 0.30;

/// Certainty class of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyTier {
    /// Below [`KEY_TIER_MEDIUM`] (also confidence 0 and NaN).
    Low,
    /// `KEY_TIER_MEDIUM <= confidence < KEY_TIER_HIGH`.
    Medium,
    /// `confidence >= KEY_TIER_HIGH`.
    High,
}

impl KeyTier {
    /// Lowercase name used in reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Tier of a stored key confidence. Non-finite values are [`KeyTier::Low`].
pub fn key_tier(confidence: f32) -> KeyTier {
    if confidence >= KEY_TIER_HIGH {
        KeyTier::High
    } else if confidence >= KEY_TIER_MEDIUM {
        KeyTier::Medium
    } else {
        KeyTier::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_monotone_and_inclusive() {
        const { assert!(0.0 < KEY_TIER_MEDIUM && KEY_TIER_MEDIUM < KEY_TIER_HIGH) };
        const { assert!(KEY_TIER_HIGH <= 1.0) };
        assert_eq!(key_tier(KEY_TIER_HIGH), KeyTier::High);
        assert_eq!(key_tier(KEY_TIER_MEDIUM), KeyTier::Medium);
        assert_eq!(key_tier(KEY_TIER_HIGH - 1e-4), KeyTier::Medium);
        assert_eq!(key_tier(KEY_TIER_MEDIUM - 1e-4), KeyTier::Low);
        assert_eq!(key_tier(1.0), KeyTier::High);
        assert_eq!(key_tier(0.0), KeyTier::Low);
        assert_eq!(key_tier(f32::NAN), KeyTier::Low);
        assert!(KeyTier::Low < KeyTier::Medium && KeyTier::Medium < KeyTier::High);
    }
}

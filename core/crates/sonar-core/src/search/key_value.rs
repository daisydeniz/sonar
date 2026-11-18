//! How an effective key is stored in the `ff_key` index field (T10.5,
//! ADR-041): one u64 per file carrying the key, whether it has a mode, and
//! whether it is uncertain (low tier).
//!
//! **Value**: `root * 3 + slot (+ LOW_OFFSET)`, slot 0 = major, 1 =
//! mode-less (a note or loop root, a bare `C` in a name), 2 = minor.
//! Values below [`LOW_OFFSET`] are medium/high tier (or a user override);
//! from [`LOW_OFFSET`] on the same layout repeats for the low tier.
//!
//! - **Filter**: a requested key ([`super::keys::key_code`]) accepts its
//!   own value and the mode-less value of its root, so an `A` 808 answers
//!   both `A` and `Am`. Low-tier values are accepted only with
//!   [`super::Filters::include_uncertain_keys`]; without it they count as
//!   "no key" for the filter and the facet.
//! - **Facet**: a mode-less value counts for both modes of its root.
//! - **Sort**: values order by root, then major / mode-less / minor; the
//!   low tier sorts after every certain key in both directions (the
//!   collector treats values from [`LOW_OFFSET`] on as a trailing group).
//!
//! One single-valued column keeps filtering, drill-sideways counting and
//! sorting as cheap as before (no multi-valued reads).

use super::keys::key_code;
use crate::key::KeyTier;
use crate::text::{Key, Mode};

/// Values per tier group: 12 roots × (major, mode-less, minor).
const PER_GROUP: u64 = 36;

/// First index value of the low tier.
pub(crate) const LOW_OFFSET: u64 = PER_GROUP;

/// Slot of the mode-less value of a root.
const MODELESS: u64 = 1;

/// Index value of an effective key of `tier` (module docs).
pub(crate) fn index_value(key: Key, tier: KeyTier) -> u64 {
    let slot = match key.mode {
        Some(Mode::Major) => 0,
        None => MODELESS,
        Some(Mode::Minor) => 2,
    };
    let v = u64::from(key.root % 12) * 3 + slot;
    if tier == KeyTier::Low {
        v + LOW_OFFSET
    } else {
        v
    }
}

/// Index values a filter on the key codes `codes` (0..24, see
/// [`key_code`]) accepts: each code's own value and its root's mode-less
/// value, plus their low-tier twins with `include_uncertain`. Sorted and
/// deduplicated.
pub(crate) fn accepted_values(codes: &[u64], include_uncertain: bool) -> Vec<u64> {
    let mut out = Vec::with_capacity(codes.len() * 4);
    for &code in codes {
        let root = code / 2 % 12;
        let own = root * 3 + if code % 2 == 1 { 2 } else { 0 };
        for v in [own, root * 3 + MODELESS] {
            out.push(v);
            if include_uncertain {
                out.push(v + LOW_OFFSET);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Key codes ([`key_code`]) an index value is counted under in the key
/// facet: one, or both modes of the root for a mode-less value. Values out
/// of range give none.
pub(crate) fn facet_codes(value: u64) -> impl Iterator<Item = u64> {
    let in_range = value < 2 * PER_GROUP;
    let v = value % PER_GROUP;
    let root = u8::try_from(v / 3).unwrap_or(0);
    let mode = |m| u64::from(key_code(Key { root, mode: m }));
    let (first, second) = match v % 3 {
        0 => (mode(Some(Mode::Major)), None),
        2 => (mode(Some(Mode::Minor)), None),
        _ => (mode(Some(Mode::Major)), Some(mode(Some(Mode::Minor)))),
    };
    std::iter::once(first)
        .chain(second)
        .filter(move |_| in_range)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::keys::{key_label, parse_key};

    fn key(s: &str) -> Key {
        parse_key(s).unwrap_or_else(|| panic!("not a key: {s}"))
    }

    fn labels(values: impl Iterator<Item = u64>) -> Vec<String> {
        values.map(|c| key_label(c as u8)).collect()
    }

    #[test]
    fn values_order_by_root_then_mode_and_tier() {
        let v = |s, t| index_value(key(s), t);
        let high = KeyTier::High;
        assert!(v("C", high) < v("Cm", high), "mode-less between");
        assert!(v("Cmaj", high) < v("C", high));
        assert!(v("Cm", high) < v("C#maj", high));
        assert_eq!(v("Bm", high), LOW_OFFSET - 1);
        assert_eq!(v("Am", KeyTier::Medium), v("Am", high));
        assert_eq!(v("Am", KeyTier::Low), v("Am", high) + LOW_OFFSET);
        assert!(v("Cmaj", KeyTier::Low) > v("Bm", high), "low after all");
    }

    #[test]
    fn filters_accept_the_mode_less_root_and_low_only_on_request() {
        let am = u64::from(key_code(key("Am")));
        let certain = accepted_values(&[am], false);
        assert_eq!(
            certain,
            [
                index_value(key("A"), KeyTier::High),
                index_value(key("Am"), KeyTier::High)
            ]
        );
        assert!(!certain.contains(&index_value(key("Amaj"), KeyTier::High)));
        let all = accepted_values(&[am], true);
        assert_eq!(all.len(), 4);
        assert!(all.contains(&index_value(key("A"), KeyTier::Low)));
        assert!(all.contains(&index_value(key("Am"), KeyTier::Low)));
        assert!(accepted_values(&[], true).is_empty());
    }

    #[test]
    fn mode_less_values_count_for_both_modes() {
        for tier in [KeyTier::High, KeyTier::Low] {
            let f = |s| labels(facet_codes(index_value(key(s), tier)));
            assert_eq!(f("A"), ["A", "Am"]);
            assert_eq!(f("Am"), ["Am"]);
            assert_eq!(f("F#maj"), ["F#"]);
        }
        assert_eq!(facet_codes(2 * PER_GROUP).count(), 0);
    }
}

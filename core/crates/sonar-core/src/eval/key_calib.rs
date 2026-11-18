//! Key confidence calibration (T10.4, ADR-041, `docs/EVAL.md` "Key
//! calibration"): accuracy per confidence tier, the tier targets and the
//! expected calibration error of the stored key confidence.
//!
//! A key the analyzer emitted is **correct** when the file has tonal truth
//! and the tonic pitch class matches and, if both name a mode, the modes
//! agree (a mode-less key, e.g. a one-shot note or a loop root, matches on
//! the pitch class alone; a mode-less truth compares the tonic only). A key
//! emitted for a file with **no tonal truth** (drums, noise) is always
//! incorrect, which is what keeps the low tier honest. Files without any
//! truth statement and files that got no key are not scored.
//!
//! ECE: standard 10-bin expected calibration error (Naeini, Cooper &
//! Hauskrecht 2015; Guo et al. 2017, "On calibration of modern neural
//! networks") over `(confidence, correct)`, see [`crate::classify::category::calibrate::ece`].

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::classify::category::calibrate::ece;
use crate::key::{key_tier, KeyTier, KEY_TIER_HIGH, KEY_TIER_MEDIUM};
use crate::text::Key;

/// Required accuracy (%) of the high tier.
pub const HIGH_TARGET_PCT: f64 = 90.0;
/// Required accuracy (%) of the medium tier.
pub const MEDIUM_TARGET_PCT: f64 = 60.0;
/// Reliability-table bins (the ECE bins).
const BINS: usize = 10;

/// True if `got` is the key `truth` names (see the module doc).
pub fn key_correct(truth: Key, got: Key) -> bool {
    truth.root % 12 == got.root % 12
        && (truth.mode.is_none() || got.mode.is_none() || truth.mode == got.mode)
}

/// One confidence tier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TierRow {
    /// `high`, `medium` or `low`.
    pub tier: String,
    /// Keys in the tier.
    pub n: usize,
    /// Correct ones.
    pub correct: usize,
    /// `100 * correct / n` (0 when empty).
    pub accuracy_pct: f64,
}

/// One equal-width confidence bin `[lo, lo + 0.1)`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BinRow {
    /// Lower edge.
    pub lo: f64,
    /// Keys in the bin.
    pub n: usize,
    /// Correct ones.
    pub correct: usize,
    /// Mean confidence in the bin.
    pub mean_conf: f64,
}

/// Calibration of the key confidence over one dataset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyCalibration {
    /// Scored keys (with truth or known atonal, and a key emitted).
    pub total: usize,
    /// Of those, files with no tonal truth.
    pub atonal: usize,
    /// Scored files that got no key (not in any tier).
    pub no_key: usize,
    /// Tiers, high first.
    pub tiers: Vec<TierRow>,
    /// `KEY_TIER_HIGH` / `KEY_TIER_MEDIUM` the tiers were cut with.
    pub bound_high: f64,
    /// See `bound_high`.
    pub bound_medium: f64,
    /// 10-bin expected calibration error (%).
    pub ece_pct: f64,
    /// Reliability bins.
    pub bins: Vec<BinRow>,
    /// Accuracy high >= medium >= low (empty tiers skipped).
    pub monotone: bool,
    /// High tier >= [`HIGH_TARGET_PCT`] (`None` when the tier is empty:
    /// nothing to validate).
    pub high_ok: Option<bool>,
    /// Medium tier >= [`MEDIUM_TARGET_PCT`] (`None` when empty).
    pub medium_ok: Option<bool>,
}

impl KeyCalibration {
    /// Accuracy (%) of a tier, `None` when it has no keys.
    pub fn tier_acc(&self, tier: KeyTier) -> Option<f64> {
        self.tiers
            .iter()
            .find(|r| r.tier == tier.as_str() && r.n > 0)
            .map(|r| r.accuracy_pct)
    }
}

/// Collects `(confidence, correct)` pairs.
#[derive(Debug, Default)]
pub struct KeyCalibrationAcc {
    samples: Vec<(f32, bool)>,
    atonal: usize,
    no_key: usize,
}

impl KeyCalibrationAcc {
    /// Scores one file: `truth` its tonal truth (`None` = none known),
    /// `atonal` true when it is known to have no key at all, `got` the
    /// emitted key with its confidence.
    pub fn add(&mut self, truth: Option<Key>, atonal: bool, got: Option<(Key, f32)>) {
        if truth.is_none() && !atonal {
            return;
        }
        let Some((key, conf)) = got else {
            self.no_key += 1;
            return;
        };
        self.atonal += usize::from(truth.is_none());
        self.samples
            .push((conf, truth.is_some_and(|t| key_correct(t, key))));
    }

    /// The finished calibration; `None` when nothing was scored.
    pub fn finish(self) -> Option<KeyCalibration> {
        if self.samples.is_empty() && self.no_key == 0 {
            return None;
        }
        Some(calibrate(&self.samples, self.atonal, self.no_key))
    }
}

/// Builds the tier table, bins and checks from `(confidence, correct)`.
pub fn calibrate(samples: &[(f32, bool)], atonal: usize, no_key: usize) -> KeyCalibration {
    let mut tiers = [KeyTier::High, KeyTier::Medium, KeyTier::Low].map(|t| TierRow {
        tier: t.as_str().into(),
        ..TierRow::default()
    });
    let mut bins: Vec<BinRow> = (0..BINS)
        .map(|b| BinRow {
            lo: b as f64 / BINS as f64,
            ..BinRow::default()
        })
        .collect();
    for &(c, ok) in samples {
        let c = if c.is_finite() {
            c.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let t = &mut tiers[2 - key_tier(c) as usize];
        t.n += 1;
        t.correct += usize::from(ok);
        let b = &mut bins[((c * BINS as f32) as usize).min(BINS - 1)];
        b.n += 1;
        b.correct += usize::from(ok);
        b.mean_conf += f64::from(c);
    }
    for t in &mut tiers {
        if t.n > 0 {
            t.accuracy_pct = 100.0 * t.correct as f64 / t.n as f64;
        }
    }
    for b in &mut bins {
        if b.n > 0 {
            b.mean_conf /= b.n as f64;
        }
    }
    let accs: Vec<f64> = tiers
        .iter()
        .filter(|t| t.n > 0)
        .map(|t| t.accuracy_pct)
        .collect();
    let monotone = accs.windows(2).all(|w| w[0] >= w[1]);
    let ok = |t: &TierRow, target: f64| (t.n > 0).then_some(t.accuracy_pct >= target);
    KeyCalibration {
        total: samples.len(),
        atonal,
        no_key,
        high_ok: ok(&tiers[0], HIGH_TARGET_PCT),
        medium_ok: ok(&tiers[1], MEDIUM_TARGET_PCT),
        tiers: tiers.into(),
        bound_high: f64::from(KEY_TIER_HIGH),
        bound_medium: f64::from(KEY_TIER_MEDIUM),
        ece_pct: ece(samples),
        bins,
        monotone,
    }
}

/// Markdown of the calibration (tier table, checks, reliability bins).
pub fn markdown(s: &mut String, k: &KeyCalibration) {
    let _ = writeln!(
        s,
        "\nKey calibration ({} keys scored, {} of them on files without a key, {} files got no key; bounds high >= {:.2}, medium >= {:.2}):\n",
        k.total, k.atonal, k.no_key, k.bound_high, k.bound_medium
    );
    s.push_str("| Tier | n | Correct | Accuracy (%) |\n|---|---:|---:|---:|\n");
    for t in &k.tiers {
        let acc = if t.n == 0 {
            "n/a".to_string()
        } else {
            format!("{:.2}", t.accuracy_pct)
        };
        let _ = writeln!(s, "| {} | {} | {} | {} |", t.tier, t.n, t.correct, acc);
    }
    let yn = |b: Option<bool>| match b {
        Some(true) => "yes",
        Some(false) => "NO",
        None => "n/a (empty tier)",
    };
    let _ = writeln!(
        s,
        "\nECE {:.2} %; monotone {}; high >= {HIGH_TARGET_PCT:.0} %: {}; medium >= {MEDIUM_TARGET_PCT:.0} %: {}.",
        k.ece_pct,
        yn(Some(k.monotone)),
        yn(k.high_ok),
        yn(k.medium_ok)
    );
    s.push_str("\n| Confidence bin | n | Correct | Mean conf | Accuracy (%) |\n|---|---:|---:|---:|---:|\n");
    for b in k.bins.iter().filter(|b| b.n > 0) {
        let _ = writeln!(
            s,
            "| {:.1}-{:.1} | {} | {} | {:.3} | {:.1} |",
            b.lo,
            b.lo + 0.1,
            b.n,
            b.correct,
            b.mean_conf,
            100.0 * b.correct as f64 / b.n as f64
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Mode;

    fn key(root: u8, mode: Option<Mode>) -> Key {
        Key { root, mode }
    }

    #[test]
    fn correctness_rules() {
        let am = key(9, Some(Mode::Minor));
        assert!(key_correct(am, am));
        assert!(key_correct(am, key(9, None)), "mode-less got: pitch class");
        assert!(key_correct(key(9, None), am), "mode-less truth: tonic");
        assert!(!key_correct(am, key(9, Some(Mode::Major))));
        assert!(!key_correct(am, key(0, Some(Mode::Major))));
        assert!(key_correct(am, key(21, Some(Mode::Minor))), "mod 12");
    }

    /// Hand-computed: high {0.9 ok, 0.8 ok, 0.8 wrong}, medium {0.5 ok,
    /// 0.4 wrong}, low {0.1 wrong x2}. The atonal files count wrong, files
    /// without truth or without a key are skipped.
    #[test]
    fn tiers_and_ece_match_hand_computation() {
        let c = key(0, Some(Mode::Major));
        let d = key(2, Some(Mode::Major));
        let mut a = KeyCalibrationAcc::default();
        for (conf, got) in [(0.9, c), (0.8, c), (0.8, d), (0.5, c), (0.4, d)] {
            a.add(Some(c), false, Some((got, conf)));
        }
        a.add(None, true, Some((c, 0.1)));
        a.add(None, true, Some((d, 0.1)));
        a.add(None, false, Some((c, 0.9))); // no truth statement: skipped
        a.add(None, true, None); // no key: counted separately
        let k = a.finish().unwrap();
        assert_eq!((k.total, k.atonal, k.no_key), (7, 2, 1));
        let row = |i: usize| (k.tiers[i].n, k.tiers[i].correct);
        assert_eq!((row(0), row(1), row(2)), ((3, 2), (2, 1), (2, 0)));
        assert!((k.tiers[0].accuracy_pct - 200.0 / 3.0).abs() < 1e-9);
        assert!(k.monotone && k.high_ok == Some(false) && k.medium_ok == Some(false));
        // Bins: 0.1 -> n2 acc 0 |0.1-0|; 0.4 -> n1 acc 0 gap .4; 0.5 -> n1
        // acc 1 gap .5; 0.8 -> n2 acc .5 conf .8 gap .3; 0.9 -> n1 acc 1 gap .1.
        let want = (2.0 * 0.1 + 0.4 + 0.5 + 2.0 * 0.3 + 0.1) / 7.0 * 100.0;
        assert!((k.ece_pct - want).abs() < 1e-4, "{} vs {want}", k.ece_pct);
    }

    #[test]
    fn non_monotone_and_empty_cases() {
        let c = key(0, None);
        let wrong = key(1, None);
        let mut a = KeyCalibrationAcc::default();
        a.add(Some(c), false, Some((wrong, 0.9)));
        a.add(Some(c), false, Some((c, 0.5)));
        let k = a.finish().unwrap();
        assert!(!k.monotone && k.high_ok == Some(false) && k.medium_ok == Some(true));
        assert_eq!(k.tier_acc(KeyTier::Low), None);
        assert_eq!(k.tiers[2].n, 0);
        assert!(KeyCalibrationAcc::default().finish().is_none());
    }
}

//! `eval/reports/BASELINE.md`: the accepted numbers, and the regression
//! rule of `docs/EVAL.md`.
//!
//! The file is a human-readable table per tier plus one fenced `json` block,
//! which is the only part [`Baseline::parse`] reads. Metric keys are
//! `area.name`; keys ending in `_pct` are percentages (higher is better, a
//! drop of more than [`MAX_DROP_POINTS`] is a regression), all others are
//! counts that must not rise (one-shots given a BPM, unexpected decode
//! errors). New metrics only need a row in [`METRICS`].

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::report::EvalReport;
use super::run::TierReport;

/// Largest tolerated drop of a percentage metric (points); a bigger drop is
/// a regression (`docs/EVAL.md`).
pub const MAX_DROP_POINTS: f64 = 1.0;
/// Schema of the JSON block.
pub const SCHEMA_VERSION: u32 = 1;

/// One metric as documented in the report and baseline.
pub struct MetricInfo {
    /// Stable key (`area.name`).
    pub key: &'static str,
    /// Display name.
    pub label: &'static str,
    /// Target from `docs/EVAL.md` (text).
    pub target: &'static str,
    /// Part of the regression check.
    pub tracked: bool,
}

/// Every metric of a tier, in display order.
pub const METRICS: &[MetricInfo] = &[
    MetricInfo {
        key: "kind.accuracy_pct",
        label: "Loop/one-shot accuracy (%)",
        target: ">= 95",
        tracked: true,
    },
    MetricInfo {
        key: "bpm.accuracy_pct",
        label: "BPM accuracy, +-1 BPM, loops (%)",
        target: ">= 90",
        tracked: true,
    },
    MetricInfo {
        key: "bpm.octave_rate_pct",
        label: "BPM octave-error rate (%)",
        target: "report only",
        tracked: false,
    },
    MetricInfo {
        key: "bpm.one_shots_with_bpm",
        label: "One-shots given a BPM (count)",
        target: "0",
        tracked: true,
    },
    MetricInfo {
        key: "key.exact_pct",
        label: "Key exact (%)",
        target: "report only",
        tracked: true,
    },
    MetricInfo {
        key: "key.mirex_pct",
        label: "Key MIREX weighted (%)",
        target: ">= 80",
        tracked: true,
    },
    MetricInfo {
        key: "note.pitch_class_pct",
        label: "Note exact pitch class (%)",
        target: ">= 90",
        tracked: true,
    },
    MetricInfo {
        key: "note.midi_pct",
        label: "Note exact MIDI (%)",
        target: ">= 90",
        tracked: true,
    },
    MetricInfo {
        key: "similar.p10_pct",
        label: "Similarity P@10, k = min(10, relevant) (%)",
        target: ">= 70",
        tracked: true,
    },
    MetricInfo {
        key: "similar.mrr_pct",
        label: "Similarity MRR (%)",
        target: "report only",
        tracked: true,
    },
    MetricInfo {
        key: "similar.pitch_p10_pct",
        label: "Similarity, pitch-only, same pitch class P@10 (%)",
        target: "report only",
        tracked: false,
    },
    MetricInfo {
        key: "decode.unexpected_errors",
        label: "Unexpected decode errors (count)",
        target: "0",
        tracked: true,
    },
];

/// Display info of `key`.
pub fn info(key: &str) -> Option<&'static MetricInfo> {
    METRICS.iter().find(|m| m.key == key)
}

/// All metric values of a tier by key (metrics without data are absent).
pub fn values(t: &TierReport) -> BTreeMap<String, f64> {
    let m = &t.metrics;
    let mut v = BTreeMap::new();
    let mut put = |k: &str, x: f64| {
        v.insert(k.to_string(), x);
    };
    if let Some(k) = &m.kind {
        put("kind.accuracy_pct", k.accuracy_pct);
    }
    if let Some(b) = &m.bpm {
        if b.total > 0 {
            put("bpm.accuracy_pct", b.accuracy_pct);
            put("bpm.octave_rate_pct", b.octave_rate_pct);
        }
        if b.one_shots_checked > 0 {
            put("bpm.one_shots_with_bpm", b.one_shots_with_bpm as f64);
        }
    }
    if let Some(k) = &m.key {
        put("key.exact_pct", k.exact_pct);
        put("key.mirex_pct", k.mirex_pct);
    }
    if let Some(n) = &m.note {
        put("note.pitch_class_pct", n.pitch_class_pct);
        put("note.midi_pct", n.midi_pct);
    }
    if let Some(s) = &m.similar {
        put("similar.p10_pct", s.p10_pct);
        put("similar.mrr_pct", s.mrr_pct);
        if let Some(p) = &s.pitch {
            put("similar.pitch_p10_pct", p.p10_pct);
        }
    }
    put("decode.unexpected_errors", t.unexpected_errors as f64);
    v
}

/// Tracked values of a tier (what a baseline stores).
pub fn tracked_values(t: &TierReport) -> BTreeMap<String, f64> {
    let mut v = values(t);
    v.retain(|k, _| info(k).is_some_and(|i| i.tracked));
    v
}

/// Accepted numbers of one tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierBaseline {
    /// Date (UTC) of the run that set it.
    pub date: String,
    /// Git revision of that run.
    pub git_sha: String,
    /// Files analysed.
    pub files: usize,
    /// Dataset source (silver/golden directory).
    pub source: String,
    /// Tracked metric values.
    pub metrics: BTreeMap<String, f64>,
}

/// The parsed baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Tier name → numbers.
    pub tiers: BTreeMap<String, TierBaseline>,
}

impl Default for Baseline {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            tiers: BTreeMap::new(),
        }
    }
}

/// Why a baseline could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineError(pub String);

impl fmt::Display for BaselineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BaselineError {}

const FENCE_OPEN: &str = "```json\n";
const FENCE_CLOSE: &str = "\n```";

impl Baseline {
    /// Reads the fenced `json` block of a BASELINE.md.
    ///
    /// # Errors
    /// No block, or it is not a valid baseline.
    pub fn parse(markdown: &str) -> Result<Self, BaselineError> {
        let start = markdown
            .find(FENCE_OPEN)
            .ok_or_else(|| BaselineError("no ```json block (no baseline written yet?)".into()))?
            + FENCE_OPEN.len();
        let len = markdown[start..]
            .find(FENCE_CLOSE)
            .ok_or_else(|| BaselineError("unterminated ```json block".into()))?;
        let b: Self = serde_json::from_str(&markdown[start..start + len])
            .map_err(|e| BaselineError(format!("invalid baseline JSON: {e}")))?;
        if b.schema_version != SCHEMA_VERSION {
            return Err(BaselineError(format!(
                "baseline schema {} not supported (expected {SCHEMA_VERSION})",
                b.schema_version
            )));
        }
        Ok(b)
    }

    /// Replaces the tiers present in `report` (others are kept).
    pub fn update(&mut self, report: &EvalReport) {
        for t in &report.tiers {
            self.tiers.insert(
                t.tier.as_str().to_string(),
                TierBaseline {
                    date: report.date.clone(),
                    git_sha: report.git_sha.clone(),
                    files: t.files,
                    source: t.source.clone(),
                    metrics: tracked_values(t),
                },
            );
        }
    }

    /// BASELINE.md text: a table per tier and the machine-readable block.
    pub fn render(&self) -> String {
        let mut s = String::from(
            "# Accepted baseline metrics\n\n\
             Written by `sonar eval --write-baseline`; `sonar eval --check` fails when a \
             tracked metric drops by more than 1 point (or a count that must stay at \
             0 rises). The JSON block at the end is what `--check` reads; edit the numbers \
             only by re-running the eval.\n",
        );
        for (name, t) in &self.tiers {
            s.push_str(&format!(
                "\n## {name}\n\n{} files, {}, revision {} ({})\n\n\
                 | Metric | Value | Target |\n|---|---:|---|\n",
                t.files, t.source, t.git_sha, t.date
            ));
            for m in METRICS {
                if let Some(v) = t.metrics.get(m.key) {
                    s.push_str(&format!(
                        "| {} | {} | {} |\n",
                        m.label,
                        fmt_value(m.key, *v),
                        m.target
                    ));
                }
            }
        }
        s.push_str("\n```json\n");
        // Serialising plain maps and numbers cannot fail.
        s.push_str(&serde_json::to_string_pretty(self).unwrap_or_default());
        s.push_str("\n```\n");
        s
    }
}

/// `12.34` for percentages, `3` for counts.
pub fn fmt_value(key: &str, v: f64) -> String {
    if key.ends_with("_pct") {
        format!("{v:.2}")
    } else {
        format!("{v:.0}")
    }
}

/// One tracked metric that got worse than allowed.
#[derive(Debug, Clone, PartialEq)]
pub struct Regression {
    /// Tier name.
    pub tier: String,
    /// Metric key.
    pub metric: String,
    /// Accepted value.
    pub baseline: f64,
    /// Current value; `None` = no longer measured.
    pub current: Option<f64>,
}

impl fmt::Display for Regression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = info(&self.metric).map_or(self.metric.as_str(), |i| i.label);
        match self.current {
            Some(c) => write!(
                f,
                "{}: {label}: {} -> {} (baseline -> now)",
                self.tier,
                fmt_value(&self.metric, self.baseline),
                fmt_value(&self.metric, c)
            ),
            None => write!(
                f,
                "{}: {label}: not measured any more (baseline {})",
                self.tier,
                fmt_value(&self.metric, self.baseline)
            ),
        }
    }
}

/// Outcome of [`compare`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comparison {
    /// Regressions found.
    pub regressions: Vec<Regression>,
    /// Tiers compared.
    pub compared: Vec<String>,
    /// Tiers run but absent from the baseline.
    pub no_baseline: Vec<String>,
}

/// True if going from `baseline` to `current` breaks the rule for `key`.
pub fn is_regression(key: &str, baseline: f64, current: f64) -> bool {
    if key.ends_with("_pct") {
        baseline - current > MAX_DROP_POINTS + 1e-9
    } else {
        current > baseline + 1e-9
    }
}

/// Checks `report` against `baseline`, tier by tier (only tiers present in
/// both; only metrics stored in the baseline).
pub fn compare(baseline: &Baseline, report: &EvalReport) -> Comparison {
    let mut c = Comparison::default();
    for t in &report.tiers {
        let name = t.tier.as_str();
        let Some(base) = baseline.tiers.get(name) else {
            c.no_baseline.push(name.to_string());
            continue;
        };
        c.compared.push(name.to_string());
        let now = values(t);
        for (key, &b) in &base.metrics {
            let current = now.get(key).copied();
            if current.is_none_or(|v| is_regression(key, b, v)) {
                c.regressions.push(Regression {
                    tier: name.to_string(),
                    metric: key.clone(),
                    baseline: b,
                    current,
                });
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::dataset::Tier;
    use crate::eval::metrics::{KindMetrics, Metrics};

    fn tier_report(kind_pct: f64, unexpected: usize) -> TierReport {
        TierReport {
            tier: Tier::Fixtures,
            source: "gen".into(),
            files: 10,
            unlabeled: 0,
            unexpected_errors: unexpected,
            expected_errors: 0,
            missing_files: 0,
            wall_s: 1.0,
            metrics: Metrics {
                kind: Some(KindMetrics {
                    total: 10,
                    correct: 10,
                    accuracy_pct: kind_pct,
                    ..KindMetrics::default()
                }),
                ..Metrics::default()
            },
            notes: vec![],
        }
    }

    fn report(t: TierReport) -> EvalReport {
        EvalReport {
            schema_version: 1,
            date: "2026-01-01".into(),
            git_sha: "abc1234".into(),
            sonar_version: "0".into(),
            tiers: vec![t],
            not_available: vec![],
        }
    }

    fn baseline(kind_pct: f64) -> Baseline {
        let mut b = Baseline::default();
        b.update(&report(tier_report(kind_pct, 0)));
        b
    }

    #[test]
    fn rule_units() {
        assert!(
            !is_regression("kind.accuracy_pct", 100.0, 99.0),
            "exactly 1 point passes"
        );
        assert!(is_regression("kind.accuracy_pct", 100.0, 98.9));
        assert!(
            !is_regression("kind.accuracy_pct", 90.0, 99.0),
            "improvement"
        );
        assert!(!is_regression("bpm.one_shots_with_bpm", 0.0, 0.0));
        assert!(is_regression("bpm.one_shots_with_bpm", 0.0, 1.0));
        assert!(!is_regression("decode.unexpected_errors", 2.0, 1.0));
    }

    #[test]
    fn markdown_round_trips_and_keeps_other_tiers() {
        let mut b = baseline(97.5);
        let mut silver = tier_report(80.0, 0);
        silver.tier = Tier::Silver;
        b.update(&report(silver));
        let md = b.render();
        assert!(
            md.contains("| Loop/one-shot accuracy (%) | 97.50 | >= 95 |"),
            "{md}"
        );
        let parsed = Baseline::parse(&md).unwrap();
        assert_eq!(parsed, b);
        // Re-running only fixtures keeps the silver block.
        let mut again = parsed;
        again.update(&report(tier_report(99.0, 0)));
        assert_eq!(again.tiers.len(), 2);
        assert_eq!(again.tiers["fixtures"].metrics["kind.accuracy_pct"], 99.0);
        assert_eq!(again.tiers["silver"].metrics["kind.accuracy_pct"], 80.0);
    }

    #[test]
    fn parse_errors_are_clear() {
        let e = Baseline::parse("# nothing yet\n").unwrap_err();
        assert!(e.0.contains("no ```json block"), "{e}");
        assert!(Baseline::parse("```json\n{oops}\n```\n").is_err());
        let wrong = "```json\n{\"schema_version\":99,\"tiers\":{}}\n```\n";
        assert!(Baseline::parse(wrong).unwrap_err().0.contains("schema 99"));
    }

    #[test]
    fn compare_flags_regressions_only() {
        let b = baseline(100.0);
        let same = compare(&b, &report(tier_report(100.0, 0)));
        assert!(same.regressions.is_empty() && same.compared == ["fixtures"]);
        let better = compare(&baseline(90.0), &report(tier_report(100.0, 0)));
        assert!(better.regressions.is_empty());
        let within = compare(&b, &report(tier_report(99.2, 0)));
        assert!(within.regressions.is_empty(), "within 1 point");
        let worse = compare(&b, &report(tier_report(97.0, 0)));
        assert_eq!(worse.regressions.len(), 1);
        assert_eq!(worse.regressions[0].metric, "kind.accuracy_pct");
        assert!(worse.regressions[0].to_string().contains("100.00 -> 97.00"));
        let errors = compare(&b, &report(tier_report(100.0, 1)));
        assert_eq!(errors.regressions[0].metric, "decode.unexpected_errors");
        // A metric that disappeared is a regression too.
        let mut gone = tier_report(100.0, 0);
        gone.metrics.kind = None;
        let c = compare(&b, &report(gone));
        assert_eq!(c.regressions[0].current, None);
        // Tiers not in the baseline are reported, not failed.
        let mut other = tier_report(10.0, 0);
        other.tier = Tier::Golden;
        let c = compare(&b, &report(other));
        assert!(c.regressions.is_empty() && c.no_baseline == ["golden"]);
    }
}

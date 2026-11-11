//! The eval report: one JSON document and a markdown rendering with deltas
//! against the baseline.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

use super::baseline::{fmt_value, info, is_regression, values, Baseline, METRICS};
use super::run::TierReport;

/// Schema of the JSON report.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Metric families the docs define that have no implementation yet.
pub const NOT_AVAILABLE: &[&str] = &[
    "category (top-1, macro-F1, confusion): needs the category classifier (T5.3)",
    "text / NL search (NDCG@10, recall@50): needs NL search (T5.4)",
];

/// A whole eval run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalReport {
    /// [`REPORT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// UTC date `YYYY-MM-DD`.
    pub date: String,
    /// Short git revision (or `nogit`).
    pub git_sha: String,
    /// Core version.
    pub sonar_version: String,
    /// Tiers that were run.
    pub tiers: Vec<TierReport>,
    /// Metric families not measurable yet.
    pub not_available: Vec<String>,
}

impl EvalReport {
    /// Report of `tiers` for the given revision, dated today (UTC).
    pub fn new(git_sha: &str, tiers: Vec<TierReport>) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            date: utc_date(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs()),
            ),
            git_sha: git_sha.to_string(),
            sonar_version: crate::version().to_string(),
            tiers,
            not_available: NOT_AVAILABLE.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// Pretty JSON.
    pub fn to_json(&self) -> String {
        // Plain data: serialising cannot fail.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Markdown; with a `baseline`, adds baseline and delta columns and
    /// flags regressions.
    pub fn to_markdown(&self, baseline: Option<&Baseline>) -> String {
        let mut s = format!(
            "# Sonar eval {} ({})\n\nsonar-core {}\n",
            self.date, self.git_sha, self.sonar_version
        );
        for t in &self.tiers {
            tier_markdown(&mut s, t, baseline);
        }
        s.push_str("\n## Not available yet\n\n");
        for n in &self.not_available {
            let _ = writeln!(s, "- {n}");
        }
        s
    }

    /// Writes `<date>-<sha>.json` and `.md` into `dir` (created if needed);
    /// returns both paths.
    ///
    /// # Errors
    /// The directory or a file cannot be written.
    pub fn write(&self, dir: &Path, baseline: Option<&Baseline>) -> Result<(PathBuf, PathBuf)> {
        let io = |path: &Path, source| Error::Io {
            path: path.to_path_buf(),
            source,
        };
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        let stem = format!("{}-{}", self.date, self.git_sha);
        let (json, md) = (
            dir.join(format!("{stem}.json")),
            dir.join(format!("{stem}.md")),
        );
        std::fs::write(&json, self.to_json()).map_err(|e| io(&json, e))?;
        std::fs::write(&md, self.to_markdown(baseline)).map_err(|e| io(&md, e))?;
        Ok((json, md))
    }
}

fn tier_markdown(s: &mut String, t: &TierReport, baseline: Option<&Baseline>) {
    let base = baseline.and_then(|b| b.tiers.get(t.tier.as_str()));
    let _ = write!(
        s,
        "\n## {}\n\nsource: {}\n\n{} files analysed, {} without labels (skipped), \
         {} unexpected errors, {} expected errors (corrupt fixtures), {} missing, {:.1} s\n\n",
        t.tier.as_str(),
        t.source,
        t.files,
        t.unlabeled,
        t.unexpected_errors,
        t.expected_errors,
        t.missing_files,
        t.wall_s
    );
    s.push_str("| Metric | Value | Baseline | Delta | Target |\n|---|---:|---:|---:|---|\n");
    let now = values(t);
    for m in METRICS {
        let Some(&v) = now.get(m.key) else { continue };
        let b = base.and_then(|b| b.metrics.get(m.key)).copied();
        let (bs, ds) = match b {
            Some(b) => {
                let flag = if m.tracked && is_regression(m.key, b, v) {
                    " **REGRESSION**"
                } else {
                    ""
                };
                (fmt_value(m.key, b), format!("{:+.2}{flag}", v - b))
            }
            None => ("-".into(), "-".into()),
        };
        let _ = writeln!(
            s,
            "| {} | {} | {bs} | {ds} | {} |",
            m.label,
            fmt_value(m.key, v),
            m.target
        );
    }
    // Baseline metrics that vanished.
    if let Some(base) = base {
        for k in base.metrics.keys().filter(|k| !now.contains_key(*k)) {
            let label = info(k).map_or(k.as_str(), |i| i.label);
            let _ = writeln!(
                s,
                "| {label} | not measured | {} | - | |",
                fmt_value(k, base.metrics[k])
            );
        }
    }
    let m = &t.metrics;
    s.push_str("\nCoverage (files with that label):");
    let mut parts = Vec::new();
    if let Some(k) = &m.kind {
        parts.push(format!(
            "kind {} (loop->one-shot {}, one-shot->loop {})",
            k.total, k.loop_as_one_shot, k.one_shot_as_loop
        ));
    }
    if let Some(b) = &m.bpm {
        parts.push(format!(
            "bpm {} ({} octave errors, {} without estimate; {} one-shots checked)",
            b.total, b.octave_errors, b.missing, b.one_shots_checked
        ));
    }
    if let Some(k) = &m.key {
        parts.push(format!("key {} ({} detected)", k.total, k.detected));
    }
    if let Some(n) = &m.note {
        parts.push(format!("note {}", n.total));
    }
    if let Some(x) = &m.similar {
        let pitch = x.pitch.as_ref().map_or(String::new(), |p| {
            format!(", pitch-only {} seeds", p.queries)
        });
        let skipped = if x.skipped > 0 {
            format!(", {} golden entries not in the pool", x.skipped)
        } else {
            String::new()
        };
        parts.push(format!(
            "similarity {} seeds over {} files{skipped}{pitch}",
            x.queries, x.pool
        ));
    }
    if parts.is_empty() {
        parts.push("none".into());
    }
    let _ = writeln!(s, " {}.", parts.join("; "));
    if let Some(x) = m.similar.as_ref().filter(|x| !x.sweep.is_empty()) {
        s.push_str(
            "\nSimilarity sweep (report only):\n\n| Configuration | P@10 (%) | MRR (%) |\n|---|---:|---:|\n",
        );
        for r in &x.sweep {
            let _ = writeln!(s, "| {} | {:.2} | {:.2} |", r.name, r.p10_pct, r.mrr_pct);
        }
    }
    if !t.notes.is_empty() {
        s.push_str("\nNotes:\n\n");
        for n in &t.notes {
            let _ = writeln!(s, "- {n}");
        }
    }
}

/// `YYYY-MM-DD` (UTC) of a Unix timestamp (Hinnant's civil-from-days).
pub fn utc_date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::dataset::Tier;
    use crate::eval::metrics::{KindMetrics, Metrics};

    fn report(pct: f64) -> EvalReport {
        let t = TierReport {
            tier: Tier::Fixtures,
            source: "gen".into(),
            files: 4,
            unlabeled: 1,
            unexpected_errors: 0,
            expected_errors: 2,
            missing_files: 0,
            wall_s: 2.0,
            metrics: Metrics {
                kind: Some(KindMetrics {
                    total: 4,
                    correct: 4,
                    accuracy_pct: pct,
                    ..KindMetrics::default()
                }),
                ..Metrics::default()
            },
            notes: vec!["a note".into()],
        };
        EvalReport::new("abc1234", vec![t])
    }

    #[test]
    fn dates() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
        assert_eq!(utc_date(1_798_761_599), "2026-12-31");
        assert_eq!(utc_date(1_798_761_600), "2027-01-01");
    }

    #[test]
    fn markdown_shows_deltas_and_regressions() {
        let mut b = Baseline::default();
        b.update(&report(100.0));
        let md = report(97.0).to_markdown(Some(&b));
        assert!(
            md.contains(
                "| Loop/one-shot accuracy (%) | 97.00 | 100.00 | -3.00 **REGRESSION** | >= 95 |"
            ),
            "{md}"
        );
        let ok = report(100.0).to_markdown(Some(&b));
        assert!(ok.contains("| 100.00 | 100.00 | +0.00 |"), "{ok}");
        assert!(!ok.contains("REGRESSION"));
        let none = report(100.0).to_markdown(None);
        assert!(none.contains("| 100.00 | - | - |"), "{none}");
        assert!(none.contains("Not available yet") && none.contains("T5.3"));
        assert!(none.contains("- a note"));
    }

    #[test]
    fn json_round_trips_and_files_are_named_by_date_and_sha() {
        let r = report(100.0);
        let back: EvalReport = serde_json::from_str(&r.to_json()).unwrap();
        assert_eq!(back, r);
        let tmp = tempfile::tempdir().unwrap();
        let (json, md) = r.write(&tmp.path().join("out"), None).unwrap();
        assert_eq!(
            json.file_name().unwrap().to_string_lossy(),
            format!("{}-abc1234.json", r.date)
        );
        assert!(md.is_file() && json.is_file());
    }
}

//! Category metrics (`docs/EVAL.md` "Category"): top-1 accuracy, macro-F1
//! and the confusion matrix of the shipped classifier (CLAP + DSP when the
//! model is available, else DSP only), the DSP-only and CLAP-only
//! classifiers, and the calibration of the audio confidence (ECE, Guo et
//! al. 2017) with a held-out temperature fit.
//!
//! - **Macro-F1**: unweighted mean of per-class F1 over the categories
//!   present in the labels (a prediction of an unlabeled category only
//!   lowers the recall of the true one).
//! - **Calibration**: files are split by a hash of their id (even = fit,
//!   odd = held out); the temperature is fitted on the fit half
//!   ([`fit_temperature`]) and ECE is reported on the held-out half at
//!   `T = 1` and at the fitted `T`, plus ECE of the shipped confidence over
//!   all files.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::classify::category::calibrate::{ece, fit_temperature, softmax};
use crate::classify::category::N_CATEGORIES as N;
use crate::classify::Category;

/// What the classifiers said about one file (names hidden).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CategoryObs {
    /// Shipped classifier (CLAP + DSP, or DSP only).
    pub fused: Option<Category>,
    /// Confidence of `fused` (audio only).
    pub confidence: f32,
    /// Uncalibrated audio logits of the shipped classifier.
    pub raw_logits: [f32; N],
    /// DSP only.
    pub dsp: Option<Category>,
    /// Uncalibrated DSP-only logits.
    pub dsp_logits: [f32; N],
    /// CLAP zero-shot only (`None` without the model).
    pub clap: Option<Category>,
}

/// Per-class scores.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClassScore {
    /// Category id.
    pub category: String,
    /// Labeled files of this class.
    pub support: usize,
    /// Precision (%).
    pub precision_pct: f64,
    /// Recall (%).
    pub recall_pct: f64,
    /// F1 (%).
    pub f1_pct: f64,
}

/// Top-1 / macro-F1 of one classifier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TopOne {
    /// Correct decisions.
    pub correct: usize,
    /// `100 * correct / total`.
    pub accuracy_pct: f64,
    /// Macro-F1 (%).
    pub macro_f1_pct: f64,
}

/// Held-out temperature fit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// Files used for the fit.
    pub fit_files: usize,
    /// Files held out.
    pub held_out_files: usize,
    /// Temperature fitted on the fit half.
    pub fitted_t: f64,
    /// Temperature shipped in the classifier.
    pub shipped_t: f64,
    /// ECE on the held-out half at `T = 1` (%).
    pub ece_t1_pct: f64,
    /// ECE on the held-out half at the fitted `T` (%).
    pub ece_fitted_pct: f64,
}

/// Category metrics of a tier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CategoryMetrics {
    /// Files with a category label.
    pub total: usize,
    /// Shipped classifier.
    pub fused: TopOne,
    /// DSP only (tracked on fixtures: no model needed).
    pub dsp: TopOne,
    /// CLAP zero-shot only (`None` without the model).
    pub clap: Option<TopOne>,
    /// The shipped classifier used CLAP.
    pub used_clap: bool,
    /// ECE of the shipped audio confidence over all files (%).
    pub ece_pct: f64,
    /// Held-out temperature fit of the shipped logits.
    pub calibration: Option<Calibration>,
    /// Held-out temperature fit of the DSP-only logits.
    pub dsp_calibration: Option<Calibration>,
    /// Per-class scores of the shipped classifier.
    pub classes: Vec<ClassScore>,
    /// Confusion matrix of the shipped classifier: truth id → predicted id
    /// → count (`none` = no category).
    pub confusion: BTreeMap<String, BTreeMap<String, usize>>,
}

/// Collects labeled observations.
#[derive(Debug, Default)]
pub struct CategoryAccumulator {
    rows: Vec<(u64, Category, CategoryObs)>,
}

/// FNV-1a of an id (split key of the calibration halves).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl CategoryAccumulator {
    /// Adds one file.
    pub fn add(&mut self, id: &str, truth: Category, obs: CategoryObs) {
        self.rows.push((fnv(id), truth, obs));
    }

    /// The metrics; `None` without labeled files. `shipped_t` /
    /// `dsp_shipped_t`: temperatures of the classifier.
    pub fn finish(&self, shipped_t: f32, dsp_shipped_t: f32) -> Option<CategoryMetrics> {
        if self.rows.is_empty() {
            return None;
        }
        let pairs = |f: fn(&CategoryObs) -> Option<Category>| -> Vec<(Category, Option<Category>)> {
            self.rows.iter().map(|(_, t, o)| (*t, f(o))).collect()
        };
        let fused = pairs(|o| o.fused);
        let used_clap = self.rows.iter().any(|(_, _, o)| o.clap.is_some());
        let (fused_top, classes) = top_one(&fused);
        let conf: Vec<(f32, bool)> = self
            .rows
            .iter()
            .map(|(_, t, o)| (o.confidence, o.fused == Some(*t)))
            .collect();
        let shipped = if used_clap { shipped_t } else { dsp_shipped_t };
        Some(CategoryMetrics {
            total: self.rows.len(),
            fused: fused_top,
            dsp: top_one(&pairs(|o| o.dsp)).0,
            clap: used_clap.then(|| top_one(&pairs(|o| o.clap)).0),
            used_clap,
            ece_pct: ece(&conf),
            calibration: self.calibration(|o| &o.raw_logits, shipped),
            dsp_calibration: self.calibration(|o| &o.dsp_logits, dsp_shipped_t),
            classes,
            confusion: confusion(&fused),
        })
    }

    fn calibration(
        &self,
        logits: fn(&CategoryObs) -> &[f32; N],
        shipped_t: f32,
    ) -> Option<Calibration> {
        let (fit, held): (Vec<_>, Vec<_>) = self.rows.iter().partition(|(h, _, _)| h % 2 == 0);
        let samples = |rows: &[&(u64, Category, CategoryObs)]| -> Vec<(Vec<f32>, usize)> {
            rows.iter()
                .map(|(_, t, o)| (logits(o).to_vec(), t.index()))
                .collect()
        };
        let (fit_s, held_s) = (samples(&fit), samples(&held));
        if fit_s.is_empty() || held_s.is_empty() {
            return None;
        }
        let refs: Vec<(&[f32], usize)> = fit_s.iter().map(|(z, y)| (z.as_slice(), *y)).collect();
        let t = fit_temperature(&refs);
        let ece_at = |t: f32| {
            let mut p = [0.0f32; N];
            let v: Vec<(f32, bool)> = held_s
                .iter()
                .map(|(z, y)| {
                    softmax(z, t, &mut p);
                    let best = (0..N).fold(0, |b, i| if p[i] > p[b] { i } else { b });
                    (p[best], best == *y)
                })
                .collect();
            ece(&v)
        };
        Some(Calibration {
            fit_files: fit_s.len(),
            held_out_files: held_s.len(),
            fitted_t: f64::from(t),
            shipped_t: f64::from(shipped_t),
            ece_t1_pct: ece_at(1.0),
            ece_fitted_pct: ece_at(t),
        })
    }
}

/// Top-1 accuracy, macro-F1 and per-class scores of `(truth, predicted)`.
pub fn top_one(pairs: &[(Category, Option<Category>)]) -> (TopOne, Vec<ClassScore>) {
    let mut tp = [0usize; N];
    let mut support = [0usize; N];
    let mut predicted = [0usize; N];
    for &(t, p) in pairs {
        support[t.index()] += 1;
        if let Some(p) = p {
            predicted[p.index()] += 1;
            if p == t {
                tp[t.index()] += 1;
            }
        }
    }
    let correct: usize = tp.iter().sum();
    let pct = |a: usize, b: usize| {
        if b > 0 {
            100.0 * a as f64 / b as f64
        } else {
            0.0
        }
    };
    let classes: Vec<ClassScore> = Category::ALL
        .iter()
        .filter(|c| support[c.index()] > 0)
        .map(|c| {
            let i = c.index();
            let (p, r) = (pct(tp[i], predicted[i]), pct(tp[i], support[i]));
            ClassScore {
                category: c.as_str().into(),
                support: support[i],
                precision_pct: p,
                recall_pct: r,
                f1_pct: if p + r > 0.0 {
                    2.0 * p * r / (p + r)
                } else {
                    0.0
                },
            }
        })
        .collect();
    let macro_f1 = if classes.is_empty() {
        0.0
    } else {
        classes.iter().map(|c| c.f1_pct).sum::<f64>() / classes.len() as f64
    };
    (
        TopOne {
            correct,
            accuracy_pct: pct(correct, pairs.len()),
            macro_f1_pct: macro_f1,
        },
        classes,
    )
}

/// Markdown section of a tier report: classifiers, calibration, per-class
/// scores and the confusion matrix of the shipped classifier.
pub(crate) fn markdown(s: &mut String, c: &CategoryMetrics) {
    use std::fmt::Write as _;
    let _ = writeln!(
        s,
        "\nCategory ({} files):\n\n| Classifier | Top-1 (%) | Macro-F1 (%) |\n|---|---:|---:|",
        c.total
    );
    let mut row = |name: &str, t: &TopOne| {
        let _ = writeln!(
            s,
            "| {name} | {:.2} | {:.2} |",
            t.accuracy_pct, t.macro_f1_pct
        );
    };
    row(
        if c.used_clap {
            "shipped (CLAP + DSP)"
        } else {
            "shipped (DSP only)"
        },
        &c.fused,
    );
    row("DSP only", &c.dsp);
    if let Some(t) = &c.clap {
        row("CLAP zero-shot only", t);
    }
    let _ = writeln!(s, "\nShipped confidence ECE: {:.2} %.", c.ece_pct);
    for (name, cal) in [
        ("shipped", &c.calibration),
        ("DSP-only", &c.dsp_calibration),
    ] {
        if let Some(k) = cal {
            let _ = writeln!(
                s,
                "Temperature fit ({name} logits, {} fit / {} held out): T = {:.3} (shipped {:.3}); held-out ECE {:.2} % at T = 1, {:.2} % at the fitted T.",
                k.fit_files, k.held_out_files, k.fitted_t, k.shipped_t, k.ece_t1_pct, k.ece_fitted_pct
            );
        }
    }
    s.push_str("\n| Category | Support | Precision (%) | Recall (%) | F1 (%) |\n|---|---:|---:|---:|---:|\n");
    for k in &c.classes {
        let _ = writeln!(
            s,
            "| {} | {} | {:.1} | {:.1} | {:.1} |",
            k.category, k.support, k.precision_pct, k.recall_pct, k.f1_pct
        );
    }
    let mut cols: Vec<&String> = c.confusion.values().flat_map(|r| r.keys()).collect();
    cols.sort();
    cols.dedup();
    let _ = write!(
        s,
        "\nConfusion (rows truth, columns shipped decision):\n\n| |"
    );
    for col in &cols {
        let _ = write!(s, " {col} |");
    }
    let _ = write!(s, "\n|---|{}\n", "---:|".repeat(cols.len()));
    for (truth, r) in &c.confusion {
        let _ = write!(s, "| {truth} |");
        for col in &cols {
            let _ = write!(s, " {} |", r.get(*col).copied().unwrap_or(0));
        }
        s.push('\n');
    }
}

/// Truth id → predicted id → count.
pub fn confusion(
    pairs: &[(Category, Option<Category>)],
) -> BTreeMap<String, BTreeMap<String, usize>> {
    let mut m: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for &(t, p) in pairs {
        let col = p.map_or("none", Category::as_str).to_string();
        *m.entry(t.as_str().into())
            .or_default()
            .entry(col)
            .or_default() += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use Category::{Hat, Kick, Snare};

    /// Hand-computed: kick 2/3 right, snare 1/1, hat 0/1 (called kick).
    #[test]
    fn top_one_and_macro_f1_match_hand_computation() {
        let pairs = [
            (Kick, Some(Kick)),
            (Kick, Some(Kick)),
            (Kick, Some(Snare)),
            (Snare, Some(Snare)),
            (Hat, Some(Kick)),
            (Hat, None),
        ];
        let (t, classes) = top_one(&pairs);
        assert_eq!(t.correct, 3);
        assert!((t.accuracy_pct - 50.0).abs() < 1e-9);
        // kick: P 2/3, R 2/3, F1 2/3; snare: P 1/2, R 1, F1 2/3; hat: 0.
        let f1: Vec<f64> = classes.iter().map(|c| c.f1_pct).collect();
        assert!((f1[0] - 200.0 / 3.0).abs() < 1e-9, "{f1:?}");
        assert!((f1[1] - 200.0 / 3.0).abs() < 1e-9, "{f1:?}");
        assert_eq!(f1[2], 0.0);
        assert!((t.macro_f1_pct - 400.0 / 9.0).abs() < 1e-9);
        let m = confusion(&pairs);
        assert_eq!(m["kick"]["snare"], 1);
        assert_eq!(m["hat"]["none"], 1);
        assert_eq!(m["hat"]["kick"], 1);
    }

    #[test]
    fn empty_gives_no_metrics() {
        assert_eq!(CategoryAccumulator::default().finish(1.0, 1.0), None);
    }
}

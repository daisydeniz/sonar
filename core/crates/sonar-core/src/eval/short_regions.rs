//! `sonar eval --short-regions` (T13.1): which widening strategy
//! ([`Widen`]) makes "find similar to this selection" work best for very
//! short selections (10, 20, 30 ms). Report only, never in the baseline.
//!
//! **Library.** Every file of the tier is analysed like a stored row
//! ([`crate::similar::seed::analyze_file`]); the raw aspect vectors are
//! z-scored with the stats of that pool and ranked with the default
//! weights and distance of [`super::similarity`] (ADR-024).
//!
//! **Crops.** From each file's analysis-rate signal (from the file start,
//! as region times are), two deterministic positions: the *onset* (first
//! sample with `|x| ≥ 0.1 · peak`) and a *later* point (half-way between
//! the onset and the end of the analysed signal), each cropped to 10, 20
//! and 30 ms (shifted left if it would pass the end; files shorter than
//! the crop or silent are skipped). Each crop is analysed with every
//! strategy through [`region_samples`], the function region seeds use.
//!
//! **Metrics.** A region seed excludes nothing (ADR-040), so the source
//! file is a candidate:
//! - *source@10*: the source file is in the top 10;
//! - *same-category@10*: share of the top 10 with the source's category
//!   label (sources without a label are skipped; unlabeled hits count as
//!   different).

use rayon::prelude::*;
use serde::Serialize;

use crate::classify::Category;
use crate::decode::DecodeOptions;
use crate::dsp::aspects::{AspectVectors, LibraryNorm};
use crate::jobs::work::{reset_workers, window_aspects, with_workers};
use crate::quiet_panic;
use crate::similar::seed::analyze_file;
use crate::similar::widen::{region_samples, widen_samples, Widen};

use super::dataset::Dataset;
use super::similarity::{distance, Config};

/// Crop lengths (ms).
pub const CROP_MS: [u32; 3] = [10, 20, 30];
/// Ranks that count as a hit.
pub const TOP_K: usize = 10;
/// Onset threshold, relative to the peak magnitude.
const ONSET_REL: f32 = 0.1;

/// One crop length × strategy.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    /// Crop length (ms).
    pub crop_ms: u32,
    /// Strategy name ([`Widen::as_str`]).
    pub strategy: &'static str,
    /// Region seeds scored.
    pub queries: usize,
    /// Seeds whose source file is in the top 10 (%).
    pub source_at10_pct: f64,
    /// Seeds with a category label.
    pub cat_queries: usize,
    /// Mean share of the top 10 with the source's category (%).
    pub same_cat_at10_pct: f64,
}

/// Result of one tier.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShortRegionReport {
    /// Tier name.
    pub tier: &'static str,
    /// Files in the library pool.
    pub pool: usize,
    /// Files that could not be analysed (not in the pool).
    pub failed: usize,
    /// Rows, crop length major, strategies in [`Widen::ALL`] order.
    pub rows: Vec<Row>,
}

/// Vectors of one file: the whole file, and per (crop, position,
/// strategy) the region's vectors (`None` = crop skipped).
struct FileResult {
    whole: AspectVectors,
    crops: Vec<Option<AspectVectors>>,
}

/// Index of `(crop, position, strategy)` in [`FileResult::crops`].
fn slot(crop: usize, pos: usize, strategy: usize) -> usize {
    (crop * 2 + pos) * Widen::ALL.len() + strategy
}

/// Start samples of the onset and the later crop of `len` samples, or
/// `None` if the signal is shorter than `len` or silent.
pub fn crop_starts(samples: &[f32], len: usize) -> Option<[usize; 2]> {
    let n = samples.len();
    if n < len || len == 0 {
        return None;
    }
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if !peak.is_finite() || peak <= 0.0 {
        return None;
    }
    let onset = samples.iter().position(|x| x.abs() >= ONSET_REL * peak)?;
    let later = onset + (n - onset) / 2;
    Some([onset.min(n - len), later.min(n - len)])
}

/// Region vectors of every crop of `item_path` (decode once).
fn crop_vectors(path: &std::path::Path) -> Vec<Option<AspectVectors>> {
    let empty = vec![None; CROP_MS.len() * 2 * Widen::ALL.len()];
    let run = quiet_panic::guarded(|| {
        with_workers(|w| {
            let Ok(mut d) = w.decoder.decode(path, &DecodeOptions::default()) else {
                return empty.clone();
            };
            let samples = std::mem::take(&mut d.samples);
            let sr = d.sample_rate;
            let w_len = widen_samples(sr);
            let mut out = empty.clone();
            for (c, &ms) in CROP_MS.iter().enumerate() {
                let len = (f64::from(ms) * 1e-3 * f64::from(sr)).round() as usize;
                let Some(starts) = crop_starts(&samples, len) else {
                    continue;
                };
                for (p, &first) in starts.iter().enumerate() {
                    for (s, &st) in Widen::ALL.iter().enumerate() {
                        d.samples = region_samples(&samples, first, first + len, w_len, st);
                        out[slot(c, p, s)] = window_aspects(w, &d).ok();
                    }
                }
            }
            out
        })
    });
    run.unwrap_or_else(|_| {
        reset_workers();
        empty
    })
}

/// Runs the short-region evaluation on `ds`.
pub fn evaluate(ds: &Dataset) -> ShortRegionReport {
    let results: Vec<Option<FileResult>> = ds
        .items
        .par_iter()
        .map(|item| {
            let whole = analyze_file(&item.path, None).ok()?;
            Some(FileResult {
                whole,
                crops: crop_vectors(&item.path),
            })
        })
        .collect();
    let failed = results.iter().filter(|r| r.is_none()).count();
    let pool: Vec<(&FileResult, Option<Category>)> = results
        .iter()
        .zip(&ds.items)
        .filter_map(|(r, item)| Some((r.as_ref()?, item.labels.category)))
        .collect();
    let mut norm = LibraryNorm::new();
    for (r, _) in &pool {
        let _ = norm.push(&r.whole);
    }
    let z = |v: &AspectVectors| {
        let n = norm.normalize(v);
        [n.spectrum, n.timbre, n.pitch, n.amplitude]
    };
    let lib: Vec<[Vec<f32>; 4]> = pool.iter().map(|(r, _)| z(&r.whole)).collect();
    let cfg = Config::default();
    let mut rows = Vec::new();
    for (c, &crop_ms) in CROP_MS.iter().enumerate() {
        for (s, &st) in Widen::ALL.iter().enumerate() {
            let mut acc = Acc::default();
            for (src, (r, cat)) in pool.iter().enumerate() {
                for p in 0..2 {
                    let Some(v) = &r.crops[slot(c, p, s)] else {
                        continue;
                    };
                    let q = z(v);
                    let top = top_k(&lib, &q, &cfg);
                    let cats = top.iter().map(|&i| pool[i].1);
                    acc.add(top.contains(&src), *cat, cats);
                }
            }
            rows.push(acc.row(crop_ms, st));
        }
    }
    ShortRegionReport {
        tier: ds.tier.as_str(),
        pool: pool.len(),
        failed,
        rows,
    }
}

/// Pool positions of the [`TOP_K`] nearest to `q` (ties by position).
fn top_k(lib: &[[Vec<f32>; 4]], q: &[Vec<f32>; 4], cfg: &Config) -> Vec<usize> {
    let mut d: Vec<(f32, usize)> = lib
        .iter()
        .enumerate()
        .map(|(i, x)| (distance(q, x, cfg), i))
        .collect();
    d.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    d.into_iter().take(TOP_K).map(|(_, i)| i).collect()
}

/// Running sums of one row.
#[derive(Default)]
struct Acc {
    queries: usize,
    source_hits: usize,
    cat_queries: usize,
    cat_share: f64,
}

impl Acc {
    fn add(
        &mut self,
        source_in_top: bool,
        cat: Option<Category>,
        top: impl Iterator<Item = Option<Category>>,
    ) {
        self.queries += 1;
        self.source_hits += usize::from(source_in_top);
        if let Some(c) = cat {
            let same = top.filter(|t| *t == Some(c)).count();
            self.cat_queries += 1;
            self.cat_share += same as f64 / TOP_K as f64;
        }
    }

    fn row(&self, crop_ms: u32, st: Widen) -> Row {
        let pct = |x: f64, n: usize| if n == 0 { 0.0 } else { 100.0 * x / n as f64 };
        Row {
            crop_ms,
            strategy: st.as_str(),
            queries: self.queries,
            source_at10_pct: pct(self.source_hits as f64, self.queries),
            cat_queries: self.cat_queries,
            same_cat_at10_pct: pct(self.cat_share, self.cat_queries),
        }
    }
}

impl ShortRegionReport {
    /// Markdown table of the tier.
    pub fn to_markdown(&self) -> String {
        let mut s = format!(
            "### Short regions: {} (pool {}, failed {})\n\n\
             | crop | strategy | seeds | source@10 | seeds w/ category | same-category@10 |\n\
             |---|---|---|---|---|---|\n",
            self.tier, self.pool, self.failed
        );
        for r in &self.rows {
            s.push_str(&format!(
                "| {} ms | {} | {} | {:.1}% | {} | {:.1}% |\n",
                r.crop_ms,
                r.strategy,
                r.queries,
                r.source_at10_pct,
                r.cat_queries,
                r.same_cat_at10_pct
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_starts_are_onset_and_midpoint() {
        let mut s = vec![0.0f32; 1000];
        s[100] = 0.05; // below 10 % of the peak
        s[200] = 0.5;
        s[300] = 1.0;
        assert_eq!(crop_starts(&s, 10), Some([200, 600]));
        // Shifted left at the end.
        assert_eq!(crop_starts(&s, 500), Some([200, 500]));
        assert_eq!(crop_starts(&s, 1001), None);
        assert_eq!(crop_starts(&[0.0; 100], 10), None);
    }

    #[test]
    fn fixtures_report_has_every_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ds = super::super::fixtures_dataset(dir.path(), false).expect("fixtures");
        let r = evaluate(&ds);
        assert_eq!(r.rows.len(), CROP_MS.len() * Widen::ALL.len());
        assert!(r.pool > 50, "{r:?}");
        for row in &r.rows {
            assert!(row.queries > 50, "{row:?}");
            assert!((0.0..=100.0).contains(&row.source_at10_pct));
            assert!((0.0..=100.0).contains(&row.same_cat_at10_pct));
        }
        assert!(r.to_markdown().contains("| 10 ms | context |"));
    }
}

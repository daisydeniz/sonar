//! Similarity metrics of `docs/EVAL.md` ("Similarity"): P@10 and MRR of
//! the aspect-weighted nearest-neighbour ranking of [`crate::similar`].
//!
//! The ranking is computed in memory from the raw aspect vectors of the
//! evaluated files (library normalization over exactly that pool, then
//! `D = Σₐ wₐ·dₐ / Σₐ wₐ` with `dₐ` the RMS z-difference, ADR-024; ties
//! order by pool position), so no store or index is needed. The seed is
//! never a result.
//!
//! **Relevance.** Fixtures: two files are relevant to each other when they
//! share a *family* ([`fixture_family`]: truth category + kind, hats
//! merged). Golden: the lists of `similar.jsonl` ([`SimilarQuery`]).
//! Silver names carry no category, so silver has no similarity metric.
//!
//! **P@10.** For a seed with `|R| ≥ 1` relevant files the precision is
//! taken at `k = min(10, |R|)` (R-precision capped at 10), so a group of 4
//! needs all 4 in the top 4. Plain P@10 would cap at `|R|/10` for groups
//! smaller than 10. Seeds without a relevant file do not count. **MRR** is
//! the mean of `1 / rank` of the first relevant file in the whole ranking.
//! Both are reported in percent (0–100), like every other metric.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dsp::aspects::{Aspect, AspectVectors, LibraryNorm};
use crate::fixtures::manifest::{Category, Fixture, Group, Kind};
use crate::similar::AspectWeights;

/// Largest `k` of precision-at-k.
pub const MAX_K: usize = 10;

/// One golden query: a seed and the files a human judged similar. Paths
/// are already joined with the golden directory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SimilarQuery {
    /// Seed file.
    pub query: PathBuf,
    /// Files that should rank high.
    pub relevant: Vec<PathBuf>,
}

/// One analysed file of the pool.
#[derive(Debug, Clone)]
pub struct Entry {
    /// File (identity for golden queries).
    pub path: PathBuf,
    /// Raw aspect vectors.
    pub vectors: AspectVectors,
    /// Fixture sound family, if any.
    pub family: Option<String>,
    /// Labeled MIDI note, if any.
    pub note_midi: Option<u8>,
}

/// A seed and its relevant pool positions (seed excluded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Position of the seed in the pool.
    pub seed: usize,
    /// Positions of the relevant files.
    pub relevant: HashSet<usize>,
}

/// Distance configuration of one run.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Aspect weights.
    pub weights: AspectWeights,
    /// Per-aspect mean absolute difference instead of RMS.
    pub l1: bool,
}

impl Default for Config {
    /// What `sonar similar` does by default.
    fn default() -> Self {
        Self {
            weights: AspectWeights::default(),
            l1: false,
        }
    }
}

/// One configuration of the (report-only) sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepRow {
    /// Description of the configuration.
    pub name: String,
    /// P@10 (%).
    pub p10_pct: f64,
    /// MRR (%).
    pub mrr_pct: f64,
}

/// Same-pitch-class variant (pitch aspect only, among labeled notes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchSimilar {
    /// Seeds with at least one other note of the same pitch class.
    pub queries: usize,
    /// P@10 (%) with relevance = same pitch class.
    pub p10_pct: f64,
}

/// Similarity metrics of a tier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SimilarMetrics {
    /// Seeds with at least one relevant file.
    pub queries: usize,
    /// Files with vectors the ranking ran over.
    pub pool: usize,
    /// P@10 (%), default weights.
    pub p10_pct: f64,
    /// MRR (%), default weights.
    pub mrr_pct: f64,
    /// Pitch-only same-pitch-class variant (report only).
    pub pitch: Option<PitchSimilar>,
    /// Weight / metric sweep (only with `--similar-sweep`).
    #[serde(default)]
    pub sweep: Vec<SweepRow>,
    /// Golden queries or relevant files that could not be used.
    #[serde(default)]
    pub skipped: usize,
}

/// Sound family of a fixture (what "similar" means on synthetic files);
/// `None` for files outside the similarity pool: `formats/` (20 identical
/// sines), `containers/`, corrupt, silent and categoryless files.
pub fn fixture_family(f: &Fixture) -> Option<&'static str> {
    if matches!(f.group, Group::Formats | Group::Containers) || f.truth.corrupt.is_some() {
        return None;
    }
    Some(match (f.truth.category?, f.truth.kind?) {
        (Category::Sine, Kind::Tone) => "sine",
        (Category::Sine, _) => "sine burst",
        (Category::Synth, Kind::OneShot) => "pluck",
        (Category::Synth, Kind::Loop) => "chords",
        (Category::Synth, _) => "pad",
        (Category::Kick, _) => "kick",
        (Category::Snare, _) => "snare",
        (Category::HatClosed | Category::HatOpen, _) => "hat",
        (Category::Drums, _) => "drum loop",
        (Category::Click, _) => "click",
        (Category::Noise, _) => "noise",
    })
}

/// Relevant hits among the first `min(k, ranked.len())` of `ranked`,
/// divided by `k` (0 for `k == 0`).
pub fn precision_at_k(ranked: &[usize], relevant: &HashSet<usize>, k: usize) -> f64 {
    let top = &ranked[..k.min(ranked.len())];
    if k == 0 {
        return 0.0;
    }
    top.iter().filter(|r| relevant.contains(r)).count() as f64 / k as f64
}

/// `1 / rank` of the first relevant file (rank from 1); 0 if none ranks.
pub fn reciprocal_rank(ranked: &[usize], relevant: &HashSet<usize>) -> f64 {
    ranked
        .iter()
        .position(|r| relevant.contains(r))
        .map_or(0.0, |p| 1.0 / (p + 1) as f64)
}

/// z-scored aspects of every pool entry (library stats over the pool).
fn normalized(entries: &[Entry]) -> Vec<[Vec<f32>; 4]> {
    let mut norm = LibraryNorm::new();
    for e in entries {
        // A dimension mismatch cannot happen (one extractor); such an
        // entry is scored against whatever the other files give.
        let _ = norm.push(&e.vectors);
    }
    entries
        .iter()
        .map(|e| {
            let n = norm.normalize(&e.vectors);
            [n.spectrum, n.timbre, n.pitch, n.amplitude]
        })
        .collect()
}

fn aspect_distance(a: &[f32], b: &[f32], l1: bool) -> f32 {
    let n = a.len().max(1) as f32;
    if l1 {
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / n
    } else {
        (a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / n).sqrt()
    }
}

/// Combined distance of two normalized entries (module docs).
fn distance(a: &[Vec<f32>; 4], b: &[Vec<f32>; 4], cfg: &Config) -> f32 {
    let w = cfg.weights.as_array();
    let total: f32 = w.iter().sum();
    (0..4)
        .filter(|&k| w[k] > 0.0)
        .map(|k| w[k] / total * aspect_distance(&a[k], &b[k], cfg.l1))
        .sum()
}

/// Pool positions of `candidates` ordered by distance to `seed` (seed
/// excluded; ties by position).
fn ranking(z: &[[Vec<f32>; 4]], seed: usize, candidates: &[usize], cfg: &Config) -> Vec<usize> {
    let mut d: Vec<(f32, usize)> = candidates
        .iter()
        .filter(|&&c| c != seed)
        .map(|&c| (distance(&z[seed], &z[c], cfg), c))
        .collect();
    d.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    d.into_iter().map(|(_, c)| c).collect()
}

/// Mean P@k (k = min(10, |R|)) and MRR, in percent, over `queries`
/// restricted to `candidates`; `None` if no query has a relevant candidate.
fn score(
    z: &[[Vec<f32>; 4]],
    queries: &[Query],
    candidates: &[usize],
    cfg: &Config,
) -> Option<(usize, f64, f64)> {
    let allowed: HashSet<usize> = candidates.iter().copied().collect();
    let (mut n, mut p, mut rr) = (0usize, 0.0, 0.0);
    for q in queries {
        let relevant: HashSet<usize> = q
            .relevant
            .iter()
            .copied()
            .filter(|r| allowed.contains(r) && *r != q.seed)
            .collect();
        if relevant.is_empty() {
            continue;
        }
        let ranked = ranking(z, q.seed, candidates, cfg);
        p += precision_at_k(&ranked, &relevant, MAX_K.min(relevant.len()));
        rr += reciprocal_rank(&ranked, &relevant);
        n += 1;
    }
    (n > 0).then(|| (n, 100.0 * p / n as f64, 100.0 * rr / n as f64))
}

/// Queries from the golden lists; the count is of entries dropped (seed
/// not in the pool, or relevant file not in the pool).
fn given_queries(entries: &[Entry], given: &[SimilarQuery]) -> (Vec<Query>, usize) {
    let at = |p: &PathBuf| entries.iter().position(|e| &e.path == p);
    let mut skipped = 0;
    let mut out = Vec::new();
    for g in given {
        let Some(seed) = at(&g.query) else {
            skipped += 1;
            continue;
        };
        let relevant: HashSet<usize> = g
            .relevant
            .iter()
            .filter_map(|r| {
                let i = at(r);
                skipped += usize::from(i.is_none());
                i
            })
            .collect();
        out.push(Query { seed, relevant });
    }
    (out, skipped)
}

/// Queries where relevant = same value of `key` (`None` = not a member).
fn grouped_queries<T: PartialEq>(
    entries: &[Entry],
    key: impl Fn(&Entry) -> Option<T>,
) -> Vec<Query> {
    let keys: Vec<Option<T>> = entries.iter().map(key).collect();
    keys.iter()
        .enumerate()
        .filter_map(|(seed, k)| {
            let k = k.as_ref()?;
            let relevant = keys
                .iter()
                .enumerate()
                .filter(|(i, o)| *i != seed && o.as_ref() == Some(k))
                .map(|(i, _)| i)
                .collect();
            Some(Query { seed, relevant })
        })
        .collect()
}

/// The configurations of `--similar-sweep`: (description, config).
pub fn sweep_grid() -> Vec<(String, Config)> {
    let w = |s, t, p, a| AspectWeights {
        spectrum: s,
        timbre: t,
        pitch: p,
        amplitude: a,
    };
    let l2 = |weights| Config { weights, l1: false };
    vec![
        ("default: equal weights, L2".into(), Config::default()),
        (
            "equal weights, L1".into(),
            Config {
                weights: AspectWeights::default(),
                l1: true,
            },
        ),
        ("spectrum only".into(), l2(w(1.0, 0.0, 0.0, 0.0))),
        ("timbre only".into(), l2(w(0.0, 1.0, 0.0, 0.0))),
        ("pitch only".into(), l2(w(0.0, 0.0, 1.0, 0.0))),
        ("amplitude only".into(), l2(w(0.0, 0.0, 0.0, 1.0))),
        ("no pitch".into(), l2(w(1.0, 1.0, 0.0, 1.0))),
        ("pitch x0.5".into(), l2(w(1.0, 1.0, 0.5, 1.0))),
        ("pitch x2".into(), l2(w(1.0, 1.0, 2.0, 1.0))),
        ("spectrum+timbre x2".into(), l2(w(2.0, 2.0, 1.0, 1.0))),
        ("amplitude x2".into(), l2(w(1.0, 1.0, 1.0, 2.0))),
    ]
}

/// Computes the similarity metrics of a pool; `given` are golden queries
/// (empty: relevance by fixture family). `None` when no seed has a
/// relevant file (nothing to measure).
pub fn compute(entries: &[Entry], given: &[SimilarQuery], sweep: bool) -> Option<SimilarMetrics> {
    let z = normalized(entries);
    let all: Vec<usize> = (0..entries.len()).collect();
    let (queries, skipped) = if given.is_empty() {
        (
            grouped_queries(entries, |e| e.family.clone()),
            0, // fixtures: every entry is in the pool
        )
    } else {
        given_queries(entries, given)
    };
    let (n, p10, mrr) = score(&z, &queries, &all, &Config::default())?;
    let notes: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].note_midi.is_some())
        .collect();
    let pitch_queries = grouped_queries(entries, |e| e.note_midi.map(|m| m % 12));
    let pitch_cfg = Config {
        weights: AspectWeights::only(Aspect::Pitch),
        l1: false,
    };
    let pitch = score(&z, &pitch_queries, &notes, &pitch_cfg)
        .map(|(queries, p10_pct, _)| PitchSimilar { queries, p10_pct });
    let sweep = if sweep {
        sweep_grid()
            .into_iter()
            .filter_map(|(name, cfg)| {
                let (_, p10_pct, mrr_pct) = score(&z, &queries, &all, &cfg)?;
                Some(SweepRow {
                    name,
                    p10_pct,
                    mrr_pct,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(SimilarMetrics {
        queries: n,
        pool: entries.len(),
        p10_pct: p10,
        mrr_pct: mrr,
        pitch,
        sweep,
        skipped,
    })
}

/// Parses `similar.jsonl` (one JSON object per line, blank lines
/// ignored); paths are joined with `dir` (absolute paths stay as they
/// are). Returns the queries and one warning per unusable line.
pub fn parse_similar_jsonl(text: &str, dir: &std::path::Path) -> (Vec<SimilarQuery>, Vec<String>) {
    let mut queries = Vec::new();
    let mut warnings = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<SimilarQuery>(line) {
            Ok(q) if !q.relevant.is_empty() => queries.push(SimilarQuery {
                query: dir.join(&q.query),
                relevant: q.relevant.iter().map(|r| dir.join(r)).collect(),
            }),
            Ok(_) => warnings.push(format!("similar.jsonl line {}: empty relevant list", i + 1)),
            Err(e) => warnings.push(format!("similar.jsonl line {}: {e}", i + 1)),
        }
    }
    (queries, warnings)
}

#[cfg(test)]
#[path = "similarity_tests.rs"]
mod tests;

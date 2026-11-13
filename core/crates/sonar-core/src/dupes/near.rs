//! Near duplicates (ADR-036): the same sound in other files — re-encodes,
//! other sample rate / bit depth / channel count, gain changes, a few ms
//! of padding or trim.
//!
//! 1. **Blocking.** Files with a vector row and a duration, sorted by
//!    `audio_meta.duration_ms`; a pair is compared only if the durations
//!    differ by at most [`WINDOW_MS`] or [`WINDOW_PCT`] % of the longer one
//!    (no all-pairs scan; re-encodes and padding change the length by
//!    tens of ms). Digital silence (no level at all) is left out: silent
//!    files are not "the same sound".
//! 2. **Prefilter + metric.** [`super::distance`]: a 16-dim lower bound,
//!    then the max-aspect distance with early exit; a pair at most the
//!    threshold is an edge. Pairs are scanned in parallel (rayon) in
//!    chunks of the sorted list; the cancel flag is checked per chunk.
//! 3. **Groups.** Connected components of the edges (union-find), so a
//!    group is the same whatever the scan order. `max_distance` /
//!    `mean_distance` are over every pair of the group (only over its edges
//!    above [`ALL_PAIRS_MAX`] members).

use std::sync::atomic::AtomicBool;

use rayon::prelude::*;

use super::distance::{pre_pass, Metric, PreRow};
use super::{is_cancelled, Entry};
use crate::dsp::aspects::amplitude::LOUDNESS;
use crate::dsp::aspects::Aspect;
use crate::index::vectors::VectorStore;
use crate::{Error, Result};

/// Absolute duration window (ms).
pub const WINDOW_MS: i64 = 80;
/// Relative duration window (% of the longer file).
pub const WINDOW_PCT: i64 = 1;
/// Raw Amplitude `LOUDNESS` at or below which a file is digital silence
/// (the aspect's silent value is −100 dBFS).
const SILENT_DBFS: f32 = -99.5;
/// Sorted positions per parallel work item.
const CHUNK: usize = 2048;
/// Largest group whose distances are taken over every pair.
pub const ALL_PAIRS_MAX: usize = 64;

/// One near group: entry indices (ascending file id) and its distances.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct NearGroup {
    pub members: Vec<usize>,
    pub max_distance: f32,
    pub mean_distance: f32,
}

/// Outcome of the near pass.
#[derive(Debug, Default)]
pub(super) struct Near {
    pub groups: Vec<NearGroup>,
    /// Files compared (vector row, duration, not silent, not a hidden copy).
    pub files: usize,
    /// Pairs within the duration window.
    pub pairs: u64,
}

/// A linked pair: positions in the sorted items and the pair distance.
type Edge = (u32, u32, f32);

/// One sortable file of the pass.
struct Item {
    duration: i64,
    entry: usize,
    row: usize,
}

/// Whether durations `a ≤ b` (ms) are close enough to compare.
#[inline]
fn in_window(a: i64, b: i64) -> bool {
    b - a <= WINDOW_MS.max(b.saturating_mul(WINDOW_PCT) / 100)
}

/// Finds the near groups among `entries` whose `skip` flag is false (see
/// the module docs).
///
/// # Errors
/// [`Error::Cancelled`]; [`Error::Internal`] for a damaged vector store.
pub(super) fn find(
    vs: &VectorStore,
    entries: &[Entry],
    skip: &[bool],
    threshold: f32,
    cancel: Option<&AtomicBool>,
) -> Result<Near> {
    let metric = Metric::new(vs)?;
    let mut items: Vec<Item> = entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !skip[*i])
        .filter_map(|(entry, e)| {
            let row = usize::try_from(vs.row_of(e.id)?).ok()?;
            let level = vs.row(Aspect::Amplitude, row as u64)?.get(LOUDNESS)?;
            (*level > SILENT_DBFS).then_some(Item {
                duration: e.duration_ms?,
                entry,
                row,
            })
        })
        .collect();
    items.sort_unstable_by_key(|it| (it.duration, entries[it.entry].id));
    let pre: Vec<PreRow> = items.par_iter().map(|it| metric.pre_row(it.row)).collect();
    let budgets = metric.pre_budgets(threshold);
    let starts: Vec<usize> = (0..items.len()).step_by(CHUNK).collect();
    let found: Vec<(Vec<Edge>, u64)> = starts
        .par_iter()
        .map(|&start| {
            if is_cancelled(cancel) {
                return Err(Error::Cancelled);
            }
            let mut edges = Vec::new();
            let mut pairs = 0u64;
            for i in start..(start + CHUNK).min(items.len()) {
                let a = &items[i];
                for (j, b) in items.iter().enumerate().skip(i + 1) {
                    if !in_window(a.duration, b.duration) {
                        break;
                    }
                    pairs += 1;
                    if !pre_pass(&pre[i], &pre[j], &budgets) {
                        continue;
                    }
                    if let Some(d) = metric.within(a.row, b.row, threshold) {
                        edges.push((i as u32, j as u32, d));
                    }
                }
            }
            Ok((edges, pairs))
        })
        .collect::<Result<_>>()?;
    let pairs = found.iter().map(|(_, p)| p).sum();
    let edges: Vec<Edge> = found.into_iter().flat_map(|(e, _)| e).collect();
    let mut groups = components(items.len(), &edges)
        .into_iter()
        .map(|members| stats(&metric, &items, &members, &edges))
        .collect::<Vec<_>>();
    for g in &mut groups {
        g.members = g.members.iter().map(|&k| items[k].entry).collect();
        g.members.sort_unstable_by_key(|&i| entries[i].id);
    }
    Ok(Near {
        groups,
        files: items.len(),
        pairs,
    })
}

/// Connected components with ≥ 2 members (positions into the items).
fn components(n: usize, edges: &[Edge]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for &(a, b, _) in edges {
        let (ra, rb) = (root(&mut parent, a as usize), root(&mut parent, b as usize));
        if ra != rb {
            // Smaller root wins: the forest does not depend on edge order.
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut by_root: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for x in 0..n {
        let r = root(&mut parent, x);
        by_root.entry(r).or_default().push(x);
    }
    by_root.into_values().filter(|v| v.len() > 1).collect()
}

/// Max / mean distance of a component (positions into the items).
fn stats(metric: &Metric, items: &[Item], members: &[usize], edges: &[Edge]) -> NearGroup {
    let mut ds = Vec::new();
    if members.len() <= ALL_PAIRS_MAX {
        for (n, &a) in members.iter().enumerate() {
            for &b in &members[n + 1..] {
                if let Some(d) = metric.per_aspect(items[a].row, items[b].row) {
                    ds.push(d.iter().fold(0.0f32, |m, &x| m.max(x)));
                }
            }
        }
    } else {
        // `members` is ascending; an edge with one end inside lies inside.
        ds.extend(
            edges
                .iter()
                .filter(|(a, _, _)| members.binary_search(&(*a as usize)).is_ok())
                .map(|e| e.2),
        );
    }
    let max_distance = ds.iter().fold(0.0f32, |m, &x| m.max(x));
    let mean_distance = ds.iter().sum::<f32>() / ds.len().max(1) as f32;
    NearGroup {
        members: members.to_vec(),
        max_distance,
        mean_distance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_absolute_then_relative() {
        assert!(in_window(1000, 1080));
        assert!(!in_window(1000, 1081));
        assert!(in_window(20_000, 20_200));
        assert!(!in_window(20_000, 20_203));
        assert!(in_window(0, 0));
    }

    #[test]
    fn components_ignore_edge_order() {
        let edges = [(0, 1, 0.1), (3, 4, 0.0), (1, 2, 0.1), (5, 5, 0.0)];
        let mut rev = edges;
        rev.reverse();
        let want = vec![vec![0, 1, 2], vec![3, 4]];
        assert_eq!(components(6, &edges), want);
        assert_eq!(components(6, &rev), want);
        assert!(components(3, &[]).is_empty());
    }
}

//! Semantic leg of hybrid search (ADR-029): exact cosine k-nearest
//! neighbours of a CLAP text embedding among the CLAP audio embeddings of
//! the allowed files (Wu et al. 2023, arXiv:2211.06687: text-to-audio
//! retrieval by cosine similarity in the joint space).
//!
//! Both towers' outputs are L2-normalised, so cosine = dot product. The
//! scan is exact brute force over the allowed ids in 4096-id chunks on the
//! rayon pool, one bounded top-K per chunk, merged under the total
//! (distance, file id) order, as similarity search does (ADR-025): the
//! result does not depend on the thread count, and the cancel flag is
//! checked per chunk.
//!
//! **Current rows only.** The embedding store keeps the last row of a file
//! even when the file changed and its re-embedding failed or is still
//! pending (T5.2 review). [`SemanticIndex::open`] takes the files whose
//! `embed` stage result is current ([`embedded_file_ids`]); other rows are
//! never returned.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;
use rusqlite::params;

use crate::embed::{EmbedStore, Embedding, EMBED_DIM};
use crate::jobs::{Outcome, Stage};
use crate::similar::{Cand, TopK};
use crate::{Error, Result, Store};

/// Allowed ids per parallel work item (cancel check granularity).
const CHUNK: usize = 4096;

/// One semantic hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SemanticHit {
    /// `files.id`.
    pub file_id: i64,
    /// Cosine similarity to the query (−1..1, higher is closer).
    pub cosine: f32,
}

/// Where the rows live.
enum Rows {
    /// Memory-mapped read view of the on-disk store.
    Store(EmbedStore),
    /// Rows held in memory (eval, tests).
    Owned(Vec<f32>),
}

/// CLAP audio embeddings that the semantic leg searches: a read view of the
/// embedding store restricted to current rows, loaded once and shared by
/// queries (`&self`).
pub struct SemanticIndex {
    rows: Rows,
    row_of: HashMap<i64, u32>,
}

impl std::fmt::Debug for SemanticIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticIndex")
            .field("files", &self.row_of.len())
            .finish_non_exhaustive()
    }
}

impl SemanticIndex {
    /// Opens the embedding store in `dir` read-only. With `current`, only
    /// those files' rows are searchable (see the module docs); `None`
    /// keeps every live row.
    ///
    /// # Errors
    /// [`Error::VectorStore`] if there is no store or it is of another
    /// format; [`Error::Io`].
    pub fn open(dir: impl AsRef<Path>, current: Option<&[i64]>) -> Result<Self> {
        let store = EmbedStore::open_read(dir)?;
        let keep: Option<HashSet<i64>> = current.map(|c| c.iter().copied().collect());
        let mut row_of = HashMap::with_capacity(store.live_rows() as usize);
        for id in store.file_ids() {
            if keep.as_ref().is_some_and(|k| !k.contains(&id)) {
                continue;
            }
            if let Some(row) = store.row_of(id).and_then(|r| u32::try_from(r).ok()) {
                row_of.insert(id, row);
            }
        }
        Ok(Self {
            rows: Rows::Store(store),
            row_of,
        })
    }

    /// An index over embeddings held in memory (later duplicates win).
    pub fn from_embeddings(items: impl IntoIterator<Item = (i64, Embedding)>) -> Self {
        let mut matrix = Vec::new();
        let mut row_of = HashMap::new();
        for (row, (id, e)) in items.into_iter().enumerate() {
            let Ok(row) = u32::try_from(row) else { break };
            matrix.extend_from_slice(&e);
            row_of.insert(id, row);
        }
        Self {
            rows: Rows::Owned(matrix),
            row_of,
        }
    }

    /// Searchable files.
    pub fn len(&self) -> usize {
        self.row_of.len()
    }

    /// No searchable file.
    pub fn is_empty(&self) -> bool {
        self.row_of.is_empty()
    }

    /// Whether `file_id` is searchable.
    pub fn contains(&self, file_id: i64) -> bool {
        self.row_of.contains_key(&file_id)
    }

    fn matrix(&self) -> &[f32] {
        match &self.rows {
            Rows::Store(s) => s.matrix().unwrap_or(&[]),
            Rows::Owned(m) => m,
        }
    }

    /// The `k` files of `allowed` closest to `query` (cosine, best first;
    /// ties by file id) and the number of allowed files with a row.
    ///
    /// # Errors
    /// [`Error::Cancelled`] when `cancel` is set during the scan.
    pub fn knn(
        &self,
        query: &Embedding,
        allowed: &[i64],
        k: usize,
        cancel: Option<&AtomicBool>,
    ) -> Result<(Vec<SemanticHit>, usize)> {
        let matrix = self.matrix();
        let empty = || (TopK::new(k), 0usize);
        let (top, candidates) = allowed
            .par_chunks(CHUNK)
            .try_fold(empty, |(mut top, mut n), chunk| {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return Err(Error::Cancelled);
                }
                for &file_id in chunk {
                    let Some(&row) = self.row_of.get(&file_id) else {
                        continue;
                    };
                    let row = row as usize;
                    let Some(x) = matrix.get(row * EMBED_DIM..(row + 1) * EMBED_DIM) else {
                        continue;
                    };
                    n += 1;
                    top.push(Cand {
                        distance: 1.0 - dot(query, x),
                        file_id,
                        row,
                    });
                }
                Ok((top, n))
            })
            .try_reduce(empty, |(a, n), (b, m)| Ok((a.merge(b), n + m)))?;
        let hits = top
            .into_sorted()
            .into_iter()
            .map(|c| SemanticHit {
                file_id: c.file_id,
                cosine: 1.0 - c.distance,
            })
            .collect();
        Ok((hits, candidates))
    }
}

/// Dot product of two 512-d rows; 16 independent accumulators so the loop
/// vectorises (NEON / AVX) without fast-math.
#[inline]
fn dot(q: &Embedding, x: &[f32]) -> f32 {
    let mut acc = [0.0f32; 16];
    for (a, b) in q.as_chunks::<16>().0.iter().zip(x.as_chunks::<16>().0) {
        for i in 0..16 {
            acc[i] += a[i] * b[i];
        }
    }
    acc.iter().sum()
}

/// Files whose CLAP embedding is current: an `embed` stage result `ok` or
/// `partial` (a changed file loses its stage rows on rescan, so its old
/// embedding is not current until it is embedded again).
///
/// # Errors
/// [`Error::Sqlite`].
pub fn embedded_file_ids(store: &Store) -> Result<Vec<i64>> {
    let mut stmt = store.conn().prepare(
        "SELECT file_id FROM file_stages WHERE stage = ?1 AND outcome IN (?2, ?3) ORDER BY file_id",
    )?;
    let ids = stmt
        .query_map(
            params![
                Stage::Embed.as_str(),
                Outcome::Ok.as_str(),
                Outcome::Partial.as_str()
            ],
            |r| r.get::<_, i64>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Unit vector along `axis`, slightly tilted towards `axis + 1`.
    pub(crate) fn unit(axis: usize, tilt: f32) -> Embedding {
        let mut e = [0.0f32; EMBED_DIM];
        e[axis % EMBED_DIM] = 1.0;
        e[(axis + 1) % EMBED_DIM] = tilt;
        let n = (1.0 + tilt * tilt).sqrt();
        e.iter_mut().for_each(|v| *v /= n);
        e
    }

    #[test]
    fn dot_matches_the_naive_sum() {
        let a: Embedding = std::array::from_fn(|i| ((i * 37) % 11) as f32 - 5.0);
        let b: Vec<f32> = (0..EMBED_DIM)
            .map(|i| ((i * 13) % 7) as f32 * 0.25)
            .collect();
        let naive: f64 = a.iter().zip(&b).map(|(x, y)| f64::from(x * y)).sum();
        assert!((f64::from(dot(&a, &b)) - naive).abs() < 1e-3);
    }

    #[test]
    fn knn_ranks_by_cosine_within_the_allowed_set() {
        let idx = SemanticIndex::from_embeddings([
            (1, unit(0, 0.0)),
            (2, unit(0, 0.5)),
            (3, unit(1, 0.0)),
            (4, unit(0, 0.1)),
            (5, unit(0, 0.0)),
        ]);
        let q = unit(0, 0.0);
        let (hits, n) = idx.knn(&q, &[1, 2, 3, 4, 9], 3, None).expect("knn");
        let ids: Vec<i64> = hits.iter().map(|h| h.file_id).collect();
        // 9 has no row; 5 is not allowed.
        assert_eq!((ids, n), (vec![1, 4, 2], 4));
        assert!((hits[0].cosine - 1.0).abs() < 1e-6);
        // Ties order by file id; the result is the same for any split.
        let (all, _) = idx.knn(&q, &[5, 1], 10, None).expect("knn");
        assert_eq!(all.iter().map(|h| h.file_id).collect::<Vec<_>>(), [1, 5]);
        let many: Vec<i64> = (1..=20_000).collect();
        assert_eq!(idx.knn(&q, &many, 5, None).expect("knn").1, 5);
    }

    #[test]
    fn knn_is_cancellable_and_empty_is_fine() {
        let idx = SemanticIndex::from_embeddings([(1, unit(0, 0.0))]);
        let stop = AtomicBool::new(true);
        assert!(matches!(
            idx.knn(&unit(0, 0.0), &[1], 5, Some(&stop)),
            Err(Error::Cancelled)
        ));
        let empty = SemanticIndex::from_embeddings([]);
        assert!(empty.is_empty());
        let (hits, n) = empty.knn(&unit(0, 0.0), &[1, 2], 5, None).expect("knn");
        assert!(hits.is_empty() && n == 0);
    }

    #[test]
    fn open_keeps_only_current_rows() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path().join("clap");
        {
            let (mut w, _) = EmbedStore::open(&dir).expect("open");
            let (a, b, c) = (unit(0, 0.0), unit(1, 0.0), unit(2, 0.0));
            w.append(&[(1, &a), (2, &b), (3, &c)]).expect("append");
        }
        let all = SemanticIndex::open(&dir, None).expect("open");
        assert_eq!(all.len(), 3);
        let some = SemanticIndex::open(&dir, Some(&[1, 3, 7])).expect("open");
        assert_eq!(some.len(), 2);
        assert!(some.contains(3) && !some.contains(2));
        let (hits, _) = some.knn(&unit(1, 0.0), &[1, 2, 3], 3, None).expect("knn");
        assert!(hits.iter().all(|h| h.file_id != 2));
        assert!(SemanticIndex::open(tmp.path().join("none"), None).is_err());
    }

    #[test]
    fn embedded_ids_are_the_ok_embed_results() {
        let store = Store::open_in_memory().expect("store");
        store
            .conn()
            .execute_batch(
                "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/l', 0);
                 INSERT INTO files (id, library_id, rel_path, size, mtime, status)
                   VALUES (1, 1, 'a.wav', 1, 1, 'analyzed'), (2, 1, 'b.wav', 1, 1, 'analyzed'),
                          (3, 1, 'c.wav', 1, 1, 'analyzed');
                 INSERT INTO file_stages (file_id, stage, version, outcome, done_at)
                   VALUES (1, 'embed', 1, 'ok', 0), (2, 'embed', 2, 'error', 0),
                          (3, 'dsp', 1, 'ok', 0);",
            )
            .expect("rows");
        assert_eq!(embedded_file_ids(&store).expect("ids"), [1]);
    }
}

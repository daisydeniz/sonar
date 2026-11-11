//! Synthetic aspect vectors for the similarity benchmark: one row per
//! (non-missing) file of the search corpus ([`super::search_corpus`]),
//! appended to a real [`VectorStore`] by the production path.
//!
//! Rows are clustered like a sample library: each folder category
//! ("family") has a centroid, each pack shifts it a little, and every file
//! adds its own noise. Every dimension has its own offset and scale (10⁻²
//! to 10²), so the library z-score matters, and about 1 % of the values
//! are outliers that hit the ±8 z clamp. Deterministic for a seed.

use rayon::prelude::*;

use super::search_corpus::{file_row, pack_and_family, Rng, FAMILIES};
use crate::dsp::aspects::{Aspect, AspectVectors, OVERALL_DIM};
use crate::index::vectors::{VectorStore, RAW_ASPECTS};
use crate::Result;

/// Rows per [`VectorStore::append`] call.
const BATCH: usize = 32_768;
/// Spread of a pack around its family centroid (centroids have spread 1).
const PACK_SPREAD: f64 = 0.5;
/// Spread of a file around its pack.
const FILE_SPREAD: f64 = 0.8;

/// Per-dimension constants of the generator.
struct Model {
    offset: Vec<f64>,
    scale: Vec<f64>,
    /// `FAMILIES × OVERALL_DIM` centroids.
    family: Vec<f64>,
    /// `packs × OVERALL_DIM` shifts.
    pack: Vec<f64>,
}

impl Model {
    fn new(seed: u64, packs: usize) -> Self {
        let mut r = Rng::new(seed ^ 0xA5B3_C7D1, 0);
        let offset = (0..OVERALL_DIM)
            .map(|_| 100.0 * (unit(&mut r) - 0.5))
            .collect();
        let scale = (0..OVERALL_DIM)
            .map(|_| 10f64.powf(4.0 * unit(&mut r) - 2.0))
            .collect();
        let family = (0..FAMILIES * OVERALL_DIM).map(|_| gauss(&mut r)).collect();
        let pack = (0..packs * OVERALL_DIM)
            .map(|_| PACK_SPREAD * gauss(&mut r))
            .collect();
        Self {
            offset,
            scale,
            family,
            pack,
        }
    }

    /// Raw vectors of file number `i`.
    fn row(&self, seed: u64, i: usize) -> AspectVectors {
        let (pack, family) = pack_and_family(i);
        let mut r = Rng::new(seed ^ 0x0FF1_CE00, i);
        let mut flat = (0..OVERALL_DIM).map(|d| {
            let mut x = self.family[family * OVERALL_DIM + d]
                + self.pack[pack * OVERALL_DIM + d]
                + FILE_SPREAD * gauss(&mut r);
            if r.next().is_multiple_of(100) {
                x *= 12.0;
            }
            (self.offset[d] + self.scale[d] * x) as f32
        });
        let mut take = |a: Aspect| flat.by_ref().take(a.dim()).collect::<Vec<f32>>();
        AspectVectors {
            spectrum: take(RAW_ASPECTS[0]),
            timbre: take(RAW_ASPECTS[1]),
            pitch: take(RAW_ASPECTS[2]),
            amplitude: take(RAW_ASPECTS[3]),
        }
    }
}

/// Uniform in (0, 1].
fn unit(r: &mut Rng) -> f64 {
    ((r.next() >> 11) + 1) as f64 / (1u64 << 53) as f64
}

/// Standard normal (Box–Muller).
fn gauss(r: &mut Rng) -> f64 {
    let (u, v) = (unit(r), unit(r));
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// Appends a row for every file `0..files` of the search corpus with
/// `seed` that is not `missing` (file id = number + 1); returns the rows
/// written.
///
/// # Errors
/// Those of [`VectorStore::append`].
pub(crate) fn write_vectors(vs: &mut VectorStore, files: usize, seed: u64) -> Result<usize> {
    let model = Model::new(seed, files.div_ceil(super::search_corpus::PER_PACK) + 1);
    let mut written = 0;
    for start in (0..files).step_by(BATCH) {
        let end = (start + BATCH).min(files);
        let rows: Vec<(i64, AspectVectors)> = (start..end)
            .into_par_iter()
            .filter(|&i| !file_row(seed, i).missing)
            .map(|i| (i as i64 + 1, model.row(seed, i)))
            .collect();
        let batch: Vec<(i64, &AspectVectors)> = rows.iter().map(|(id, v)| (*id, v)).collect();
        vs.append(&batch)?;
        written += batch.len();
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_deterministic_clustered_and_sized() {
        let m = Model::new(3, 4);
        let a = m.row(3, 0);
        assert_eq!(a, m.row(3, 0));
        for (aspect, x) in a.parts() {
            assert_eq!(x.len(), aspect.dim());
            assert!(x.iter().all(|v| v.is_finite()));
        }
        // Same leaf folder (same family and pack) vs another family.
        let flat = |v: &AspectVectors| -> Vec<f64> {
            v.parts()
                .iter()
                .flat_map(|(_, x)| x.iter().copied())
                .zip(&m.scale)
                .map(|(x, s)| f64::from(x) / s)
                .collect()
        };
        let d =
            |x: &[f64], y: &[f64]| -> f64 { x.iter().zip(y).map(|(a, b)| (a - b).powi(2)).sum() };
        let (x, same, other) = (flat(&a), flat(&m.row(3, 1)), flat(&m.row(3, 60)));
        assert!(d(&x, &same) < d(&x, &other), "clusters");
    }

    #[test]
    fn writes_rows_for_present_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (mut vs, _) = VectorStore::open(tmp.path()).expect("store");
        let n = write_vectors(&mut vs, 500, 9).expect("write");
        assert!(n > 480 && n <= 500, "{n}");
        assert_eq!(vs.live_rows(), n as u64);
        assert_eq!(vs.norm().count(), n as u64);
    }
}

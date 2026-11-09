//! Synthetic, seeded audio fixtures with exact truth (`sonar fixtures`).
//!
//! [`generate`] writes audio files into group subdirectories plus a
//! `fixtures.json` [`Manifest`] that documents every truth value (tempo,
//! key, note, category, levels, container/codec, corrupt-file
//! expectations). Most groups are WAV; the `containers` group covers every
//! other scanner extension with hand-written, dependency-free writers
//! ([`aiff`], [`flac`], [`m4a`] (ALAC), [`aac`] (AAC-LC with priming),
//! [`mp3`], [`ogg`] (Vorbis) — each
//! module cites its spec). File names are neutral; truth lives only in the
//! manifest. The fixtures tier of `docs/EVAL.md` evaluates DSP against it.
//!
//! Determinism: the same seed and crate version produce byte-identical
//! files and manifest on the same platform/toolchain — a hand-rolled PRNG
//! ([`rng::SplitMix64`]), one stream per file path, no timestamps, sorted
//! output. Across OSes, libm `sin`/`exp`/`powf` may differ in the last bit,
//! which can flip a quantised sample; truth values are unaffected. Only noise-based
//! content (noise, snare/hat noise, random corrupt bytes) depends on the
//! seed; tonal and tempo content is seed-independent.

pub mod aac;
pub mod aiff;
pub mod bits;
pub mod catalog;
pub mod containers;
pub mod corrupt;
pub mod drums;
pub mod encode;
pub mod flac;
pub mod legend;
pub mod m4a;
pub mod manifest;
pub mod mp3;
pub mod mp4box;
pub mod music;
pub mod ogg;
pub mod oneshot;
pub mod pcm;
pub mod render;
pub mod rng;
pub mod spec;
pub mod synth;
pub mod truth;
pub mod wav;

use std::path::{Path, PathBuf};

pub use manifest::{Fixture, Manifest};
pub use pcm::Pcm;

use catalog::catalog;
use legend::fields_legend;
use manifest::{Generator, SCHEMA_VERSION};

use crate::{Error, Result};

/// Default seed (ASCII "SONAR").
pub const DEFAULT_SEED: u64 = 0x53_4F_4E_41_52;

/// Largest seed the CLI accepts (2^53 − 1), so the manifest's `seed` stays
/// exact in JSON readers that parse numbers as IEEE doubles (JavaScript).
pub const MAX_SEED: u64 = (1 << 53) - 1;

/// Manifest file name inside the output directory.
pub const MANIFEST_FILE: &str = "fixtures.json";

/// Options for [`generate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureOptions {
    /// PRNG seed for noise content. Keep it ≤ [`MAX_SEED`] if the manifest
    /// will be read by double-based JSON parsers.
    pub seed: u64,
    /// Include the > 5 min file (~27 MB; slowest item to synthesise).
    pub include_long: bool,
}

impl Default for FixtureOptions {
    fn default() -> Self {
        Self {
            seed: DEFAULT_SEED,
            include_long: true,
        }
    }
}

/// Writes every fixture plus `fixtures.json` into `dir` (created if
/// missing; files of the same name are overwritten, nothing is deleted).
/// Returns the manifest that was written.
pub fn generate(dir: &Path, opts: &FixtureOptions) -> Result<Manifest> {
    create_dir(dir)?;
    let mut fixtures = Vec::new();
    for spec in catalog(opts.include_long) {
        let built = encode::build(&spec, opts.seed)?;
        let path = file_path(dir, &spec.path);
        if let Some(parent) = path.parent() {
            create_dir(parent)?;
        }
        std::fs::write(&path, &built.bytes).map_err(|e| Error::io(&path, e))?;
        fixtures.push(Fixture {
            path: spec.path,
            group: spec.group,
            description: spec.description,
            bytes: built.bytes.len() as u64,
            format: built.format,
            truth: built.truth,
        });
    }
    fixtures.sort_by(|a, b| a.path.cmp(&b.path));

    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        generator: Generator {
            name: "sonar-core fixtures".to_owned(),
            version: crate::version().to_owned(),
        },
        seed: opts.seed,
        fields: fields_legend(),
        fixtures,
    };
    let path = dir.join(MANIFEST_FILE);
    let mut json = serde_json::to_vec_pretty(&manifest).map_err(|e| Error::json(&path, e))?;
    json.push(b'\n');
    std::fs::write(&path, json).map_err(|e| Error::io(&path, e))?;
    Ok(manifest)
}

/// The integer samples stored in the lossless fixture at `path` (relative,
/// `/`-separated) when generated with `seed`, for bit-exact decoder tests:
/// `Some` for AIFF, FLAC, ALAC and integer WAV entries; `None` for float
/// WAV, lossy, corrupt or unknown paths. A decoder must return each sample
/// `q` as `q / 2^(bits−1)`.
pub fn stored_pcm(path: &str, seed: u64) -> Option<Pcm> {
    catalog(true)
        .iter()
        .find(|f| f.path == path)
        .and_then(|f| encode::stored_pcm(f, seed))
}

fn create_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))
}

/// Joins a `/`-separated relative path onto `dir` component by component.
fn file_path(dir: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(dir.to_path_buf(), |p, c| p.join(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options() {
        let o = FixtureOptions::default();
        assert_eq!(o.seed, DEFAULT_SEED);
        assert!(o.include_long);
    }

    // Compile-time check: the default seed is accepted by the CLI cap.
    const _: () = assert!(DEFAULT_SEED <= MAX_SEED);

    #[test]
    fn file_path_splits_components() {
        let p = file_path(Path::new("/x"), "loops/loop_001.wav");
        assert_eq!(p, Path::new("/x").join("loops").join("loop_001.wav"));
    }

    #[test]
    fn stored_pcm_only_for_integer_lossless_entries() {
        let cat = catalog(false);
        let find = |ext: &str| cat.iter().find(|f| f.path.ends_with(ext)).unwrap();
        let pcm = stored_pcm(&find(".flac").path, DEFAULT_SEED).unwrap();
        assert!(pcm.frames() > 0);
        assert!(stored_pcm(&find(".mp3").path, DEFAULT_SEED).is_none());
        assert!(stored_pcm("containers/nope.flac", DEFAULT_SEED).is_none());
    }

    #[test]
    fn generate_fails_cleanly_when_dir_is_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("occupied");
        std::fs::write(&file, b"x").unwrap();
        let opts = FixtureOptions {
            include_long: false,
            ..FixtureOptions::default()
        };
        assert!(matches!(generate(&file, &opts), Err(Error::Io { .. })));
    }
}

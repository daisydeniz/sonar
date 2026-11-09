//! Synthetic, seeded audio fixtures with exact truth (`sonar fixtures`).
//!
//! [`generate`] writes WAV files into group subdirectories plus a
//! `fixtures.json` [`Manifest`] that documents every truth value (tempo,
//! key, note, category, levels, corrupt-file expectations). File names are
//! neutral; truth lives only in the manifest. The fixtures tier of
//! `docs/EVAL.md` evaluates DSP against it.
//!
//! Determinism: the same seed and crate version produce byte-identical
//! files and manifest on the same platform/toolchain — a hand-rolled PRNG
//! ([`rng::SplitMix64`]), one stream per file path, no timestamps, sorted
//! output. Across OSes, libm `sin`/`exp`/`powf` may differ in the last bit,
//! which can flip a quantised sample; truth values are unaffected. Only noise-based
//! content (noise, snare/hat noise, random corrupt bytes) depends on the
//! seed; tonal and tempo content is seed-independent.

pub mod catalog;
pub mod corrupt;
pub mod drums;
pub mod legend;
pub mod manifest;
pub mod music;
pub mod oneshot;
pub mod render;
pub mod rng;
pub mod spec;
pub mod synth;
pub mod wav;

use std::path::{Path, PathBuf};

pub use manifest::{Fixture, Manifest};

use catalog::catalog;
use legend::fields_legend;
use manifest::{Format, FormatTag, Generator, SampleKind, Truth, SCHEMA_VERSION};
use rng::SplitMix64;
use spec::Body;
use wav::{encode, WavSpec};

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
        let mut rng = SplitMix64::for_stream(opts.seed, &spec.path);
        let (bytes, format, truth) = match &spec.body {
            Body::Audio {
                spec: wav,
                content,
                peak_dbfs,
            } => {
                let (mut audio, mut truth) = render::render(content, wav, &mut rng);
                if let Some(p) = peak_dbfs {
                    audio.normalize_peak(*p);
                }
                let enc = encode(wav, &audio.interleave());
                truth.is_silent = Some(enc.levels.is_silent());
                truth.peak_dbfs = enc.levels.peak_dbfs().map(round3);
                truth.rms_dbfs = enc.levels.rms_dbfs().map(round3);
                (enc.bytes, Some(format_of(wav, enc.num_frames)), truth)
            }
            Body::Corrupt(kind) => {
                let truth = Truth {
                    corrupt: Some(kind.truth()),
                    ..Truth::default()
                };
                (kind.build(&mut rng), None, truth)
            }
        };
        let path = file_path(dir, &spec.path);
        if let Some(parent) = path.parent() {
            create_dir(parent)?;
        }
        std::fs::write(&path, &bytes).map_err(|e| Error::io(&path, e))?;
        fixtures.push(Fixture {
            path: spec.path,
            group: spec.group,
            description: spec.description,
            bytes: bytes.len() as u64,
            format,
            truth,
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

fn create_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))
}

/// Joins a `/`-separated relative path onto `dir` component by component.
fn file_path(dir: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(dir.to_path_buf(), |p, c| p.join(c))
}

fn format_of(spec: &WavSpec, num_frames: u64) -> Format {
    let format_tag = if spec.extensible {
        FormatTag::Extensible
    } else if spec.sample_format.is_float() {
        FormatTag::IeeeFloat
    } else {
        FormatTag::Pcm
    };
    Format {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        bit_depth: spec.sample_format.bits(),
        sample_format: if spec.sample_format.is_float() {
            SampleKind::Float
        } else {
            SampleKind::Int
        },
        format_tag,
        num_frames,
        duration_s: music::round6(num_frames as f64 / f64::from(spec.sample_rate)),
    }
}

/// Rounds to 3 decimals (0.001 dB) for stable, readable manifest values.
/// `-0.0` is normalised to `0.0` so the manifest never prints `-0.0`.
fn round3(x: f64) -> f64 {
    let r = (x * 1e3).round() / 1e3;
    if r == 0.0 {
        0.0
    } else {
        r
    }
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

    #[test]
    fn round3_never_yields_negative_zero() {
        let r = round3(-0.0003);
        assert_eq!(r, 0.0);
        assert!(r.is_sign_positive());
        assert_eq!(round3(-1.23456), -1.235);
    }

    // Compile-time check: the default seed is accepted by the CLI cap.
    const _: () = assert!(DEFAULT_SEED <= MAX_SEED);

    #[test]
    fn file_path_splits_components() {
        let p = file_path(Path::new("/x"), "loops/loop_001.wav");
        assert_eq!(p, Path::new("/x").join("loops").join("loop_001.wav"));
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

//! `sonar bench decode`: decode throughput over a directory of audio files
//! (normally `sonar fixtures` output) with the metadata pass off and on.

use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;

use super::corpus::list_audio;
use super::{machine, rate, Machine};
use crate::decode::{DecodeOptions, Decoder};
use crate::fixtures::{self, FixtureOptions};
use crate::meta::MetaOptions;
use crate::{Error, Result};

/// Options for [`bench_decode`].
#[derive(Debug, Clone, Copy)]
pub struct DecodeBenchOptions {
    /// Worker threads; 0 = all cores.
    pub threads: usize,
    /// Times each timed run walks the whole file set.
    pub repeat: usize,
}

/// One timed run over the file set.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecodeRun {
    /// Peak/loudness measured in the same pass (`decode_with_meta`).
    pub meta: bool,
    /// Files decoded (set size x repeat).
    pub files: u64,
    /// Wall-clock seconds.
    pub secs: f64,
    /// Files per second.
    pub files_per_sec: f64,
    /// Seconds of source audio covered by the files.
    pub source_audio_secs: f64,
    /// `source_audio_secs / secs`. The analysis decode only reads a 30 s
    /// window (meta: up to 600 s), so for long files this counts audio that
    /// was skipped.
    pub realtime_x: f64,
}

/// Result of [`bench_decode`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecodeBench {
    /// Where it ran.
    pub machine: Machine,
    /// Worker threads used.
    pub threads: usize,
    /// Audio files found.
    pub files_found: usize,
    /// Of those, decodable (deliberately corrupt fixtures are not).
    pub files_ok: usize,
    /// Repeats per timed run.
    pub repeat: usize,
    /// Meta off, then meta on.
    pub runs: Vec<DecodeRun>,
}

/// Generates the fixture set (without the > 5 min file when
/// `include_long` is false) into `dir` and returns its manifest.
///
/// # Errors
/// As [`fixtures::generate`].
pub fn generate_fixtures(dir: &Path, include_long: bool) -> Result<fixtures::Manifest> {
    let opts = FixtureOptions {
        include_long,
        ..FixtureOptions::default()
    };
    fixtures::generate(dir, &opts)
}

/// Decodes every audio file under `dir` with and without the meta pass.
/// One untimed pass first warms caches and drops undecodable files.
///
/// # Errors
/// [`Error::Internal`] if the thread pool cannot start.
pub fn bench_decode(dir: &Path, opts: DecodeBenchOptions) -> Result<DecodeBench> {
    let all = list_audio(dir);
    let dopts = DecodeOptions::default();
    let mopts = MetaOptions::default();
    let mut dec = Decoder::new();
    let mut good = Vec::new();
    for p in &all {
        if let Ok((d, _)) = dec.decode_with_meta(p, &dopts, &mopts) {
            good.push((p.clone(), d.source.duration_s));
        }
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads)
        .build()
        .map_err(|e| Error::Internal {
            reason: format!("cannot start thread pool: {e}"),
        })?;
    let repeat = opts.repeat.max(1);
    let source_secs: f64 = good.iter().map(|(_, s)| s).sum::<f64>() * repeat as f64;
    let n = (good.len() * repeat) as u64;
    let mut runs = Vec::new();
    for meta in [false, true] {
        let t = Instant::now();
        pool.install(|| {
            (0..good.len() * repeat)
                .into_par_iter()
                .for_each_init(Decoder::new, |dec, i| {
                    let p = &good[i % good.len()].0;
                    // Results are discarded: only the time matters.
                    let _ = if meta {
                        dec.decode_with_meta(p, &dopts, &mopts).map(|_| ())
                    } else {
                        dec.decode(p, &dopts).map(|_| ())
                    };
                });
        });
        let secs = t.elapsed().as_secs_f64();
        runs.push(DecodeRun {
            meta,
            files: n,
            secs,
            files_per_sec: rate(n, secs),
            source_audio_secs: source_secs,
            realtime_x: if secs > 0.0 { source_secs / secs } else { 0.0 },
        });
    }
    Ok(DecodeBench {
        machine: machine(),
        threads: pool.current_num_threads(),
        files_found: all.len(),
        files_ok: good.len(),
        repeat,
        runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fixtures_and_skips_corrupt_ones() {
        let tmp = tempfile::tempdir().expect("tmp");
        generate_fixtures(tmp.path(), false).expect("fixtures");
        let b = bench_decode(
            tmp.path(),
            DecodeBenchOptions {
                threads: 2,
                repeat: 1,
            },
        )
        .expect("bench");
        assert!(b.files_ok > 20 && b.files_ok < b.files_found, "{b:?}");
        assert_eq!(b.runs.len(), 2);
        assert!(!b.runs[0].meta && b.runs[1].meta);
        assert_eq!(b.runs[0].files, b.files_ok as u64);
        assert!(b.runs.iter().all(|r| r.files_per_sec > 0.0));
        assert_eq!(b.threads, 2);
    }
}

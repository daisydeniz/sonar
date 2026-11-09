//! End-to-end tests of fixture generation: manifest completeness, files vs
//! manifest (read back with `hound`, an independent WAV parser) and
//! determinism.

use std::collections::BTreeSet;
use std::io::BufReader;
use std::path::Path;

use sonar_core::fixtures::manifest::{Expect, Group, Kind, Mode, SampleKind};
use sonar_core::fixtures::{generate, FixtureOptions, Manifest, MANIFEST_FILE};

fn opts(seed: u64, include_long: bool) -> FixtureOptions {
    FixtureOptions { seed, include_long }
}

/// Every dotted path in a JSON value; array elements are written `[]`.
fn json_paths(prefix: &str, v: &serde_json::Value, out: &mut BTreeSet<String>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, child) in map {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                out.insert(p.clone());
                json_paths(&p, child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                json_paths(&format!("{prefix}[]"), item, out);
            }
        }
        _ => {}
    }
}

/// Reads all samples as f64 full-scale (ints divided by 2^(bits−1)).
fn read_samples(path: &Path) -> (hound::WavSpec, u32, Vec<f64>) {
    let file = std::fs::File::open(path).unwrap();
    let mut r = hound::WavReader::new(BufReader::new(file))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = r.spec();
    let frames = r.duration();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| f64::from(s.unwrap())).collect(),
        hound::SampleFormat::Int => {
            let scale = f64::from(1u32 << (spec.bits_per_sample - 1));
            r.samples::<i32>()
                .map(|s| f64::from(s.unwrap()) / scale)
                .collect()
        }
    };
    (spec, frames, samples)
}

fn check_files_match_manifest(dir: &Path, m: &Manifest) {
    for f in &m.fixtures {
        let path = dir.join(&f.path);
        let meta = std::fs::metadata(&path).unwrap_or_else(|e| panic!("{}: {e}", f.path));
        assert_eq!(meta.len(), f.bytes, "{}", f.path);
        let Some(fmt) = &f.format else {
            assert_eq!(f.group, Group::Corrupt, "{} has no format", f.path);
            continue;
        };
        let (spec, frames, samples) = read_samples(&path);
        assert_eq!(spec.sample_rate, fmt.sample_rate, "{}", f.path);
        assert_eq!(spec.channels, fmt.channels, "{}", f.path);
        assert_eq!(spec.bits_per_sample, fmt.bit_depth, "{}", f.path);
        let is_float = spec.sample_format == hound::SampleFormat::Float;
        assert_eq!(
            is_float,
            fmt.sample_format == SampleKind::Float,
            "{}",
            f.path
        );
        assert_eq!(u64::from(frames), fmt.num_frames, "{}", f.path);
        let dur = fmt.num_frames as f64 / f64::from(fmt.sample_rate);
        assert!((dur - fmt.duration_s).abs() < 1e-6, "{}", f.path);

        let peak = samples.iter().fold(0.0f64, |a, x| a.max(x.abs()));
        assert_eq!(f.truth.is_silent, Some(peak == 0.0), "{}", f.path);
        match f.truth.peak_dbfs {
            Some(db) => assert!((20.0 * peak.log10() - db).abs() < 0.001, "{}", f.path),
            None => assert_eq!(peak, 0.0, "{}", f.path),
        }
        if let Some(rms_db) = f.truth.rms_dbfs {
            let ms = samples.iter().map(|x| x * x).sum::<f64>() / samples.len() as f64;
            assert!((10.0 * ms.log10() - rms_db).abs() < 0.001, "{}", f.path);
        }

        if f.truth.kind == Some(Kind::Loop) {
            let (bpm, bars, bpb) = (
                f.truth.bpm.unwrap(),
                f.truth.bars.unwrap(),
                f.truth.beats_per_bar.unwrap(),
            );
            let exact = f64::from(bars * bpb) * 60.0 / bpm * f64::from(fmt.sample_rate);
            assert!((fmt.num_frames as f64 - exact).abs() <= 0.5, "{}", f.path);
        }
    }
}

fn check_manifest_contents(m: &Manifest) {
    // Sorted, unique paths.
    let paths: Vec<&str> = m.fixtures.iter().map(|f| f.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(paths, sorted);

    // Legend documents every field used, and nothing unused.
    let mut used = BTreeSet::new();
    for f in &m.fixtures {
        json_paths("", &serde_json::to_value(f).unwrap(), &mut used);
    }
    let legend: BTreeSet<String> = m.fields.keys().cloned().collect();
    let undocumented: Vec<_> = used.difference(&legend).collect();
    let unused: Vec<_> = legend.difference(&used).collect();
    assert!(
        undocumented.is_empty(),
        "undocumented fields: {undocumented:?}"
    );
    assert!(unused.is_empty(), "legend entries never used: {unused:?}");

    // Tempo span.
    let bpms: Vec<f64> = m.fixtures.iter().filter_map(|f| f.truth.bpm).collect();
    let min = bpms.iter().copied().fold(f64::INFINITY, f64::min);
    let max = bpms.iter().copied().fold(0.0, f64::max);
    assert_eq!((min, max), (70.0, 175.0));
    assert!(bpms.iter().any(|b| b.fract() != 0.0), "no non-integer BPM");

    // Keys: >= 8 distinct, both modes, sharps and flats.
    let keys: BTreeSet<(u8, bool)> = m
        .fixtures
        .iter()
        .filter_map(|f| f.truth.key.as_ref())
        .map(|k| (k.tonic_pc, k.mode == Mode::Major))
        .collect();
    assert!(keys.len() >= 8, "{} keys", keys.len());
    assert!(keys.iter().any(|k| k.1) && keys.iter().any(|k| !k.1));
    let tonics: BTreeSet<&str> = m
        .fixtures
        .iter()
        .filter_map(|f| f.truth.key.as_ref().map(|k| k.tonic.as_str()))
        .collect();
    assert!(tonics.iter().any(|t| t.contains('#')) && tonics.iter().any(|t| t.contains('b')));

    // Very short files.
    let short = m
        .fixtures
        .iter()
        .filter(|f| {
            f.format
                .as_ref()
                .is_some_and(|x| x.num_frames > 0 && x.duration_s < 0.02)
        })
        .count();
    assert!(short >= 2, "{short} very short files");

    // Format matrix: 3 depths × 3 rates × 2 channel counts.
    let combos: BTreeSet<(u16, u32, u16)> = m
        .fixtures
        .iter()
        .filter_map(|f| f.format.as_ref())
        .map(|x| (x.bit_depth, x.sample_rate, x.channels))
        .collect();
    for bits in [16, 24, 32] {
        for rate in [44_100, 48_000, 96_000] {
            for ch in [1, 2] {
                assert!(combos.contains(&(bits, rate, ch)), "{bits}/{rate}/{ch}");
            }
        }
    }

    // Corrupt files: no format, both expectations represented.
    let expects: BTreeSet<String> = m
        .fixtures
        .iter()
        .filter_map(|f| f.truth.corrupt.as_ref())
        .map(|c| format!("{:?}", c.expect))
        .collect();
    assert!(expects.contains(&format!("{:?}", Expect::Error)));
    assert!(expects.contains(&format!("{:?}", Expect::ErrorOrPartial)));

    // Every group populated; one-shots never carry tempo.
    let groups: BTreeSet<String> = m
        .fixtures
        .iter()
        .map(|f| format!("{:?}", f.group))
        .collect();
    assert_eq!(groups.len(), 8);
    for f in &m.fixtures {
        if f.truth.kind == Some(Kind::OneShot) {
            assert!(f.truth.bpm.is_none(), "{}", f.path);
        }
    }
}

#[test]
fn full_catalog_including_long_file_matches_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let m = generate(tmp.path(), &opts(42, true)).unwrap();

    let on_disk = Manifest::read(&tmp.path().join(MANIFEST_FILE)).unwrap();
    assert_eq!(on_disk, m);
    assert_eq!(m.seed, 42);
    check_manifest_contents(&m);
    check_files_match_manifest(tmp.path(), &m);

    let longest = m
        .fixtures
        .iter()
        .filter_map(|f| f.format.as_ref())
        .map(|x| x.duration_s)
        .fold(0.0, f64::max);
    assert!(longest > 300.0, "longest {longest} s");
    assert!(m.total_bytes() < 150_000_000, "{} bytes", m.total_bytes());
}

#[test]
fn same_seed_is_byte_identical_other_seed_changes_only_noise_and_rerun_overwrites() {
    let (a, b, c) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let ma = generate(a.path(), &opts(7, false)).unwrap();
    generate(b.path(), &opts(7, false)).unwrap();
    generate(c.path(), &opts(8, false)).unwrap();

    let read = |d: &Path, rel: &str| std::fs::read(d.join(rel)).unwrap();
    assert_eq!(read(a.path(), MANIFEST_FILE), read(b.path(), MANIFEST_FILE));
    for f in &ma.fixtures {
        assert_eq!(
            read(a.path(), &f.path),
            read(b.path(), &f.path),
            "{}",
            f.path
        );
        let other_seed_differs = read(a.path(), &f.path) != read(c.path(), &f.path);
        match f.truth.kind {
            Some(Kind::Noise) => assert!(other_seed_differs, "{} ignores seed", f.path),
            Some(Kind::Tone) | Some(Kind::Silence) => {
                assert!(!other_seed_differs, "{} depends on seed", f.path)
            }
            _ => {}
        }
    }
    assert!(ma
        .fixtures
        .iter()
        .all(|f| f.format.as_ref().is_none_or(|x| x.duration_s < 300.0)));

    // Regenerating seed 7 over the seed-8 directory overwrites every file.
    generate(c.path(), &opts(7, false)).unwrap();
    assert_eq!(read(a.path(), MANIFEST_FILE), read(c.path(), MANIFEST_FILE));
    for f in &ma.fixtures {
        assert!(
            read(a.path(), &f.path) == read(c.path(), &f.path),
            "{}",
            f.path
        );
    }
}

//! End-to-end tests of fixture generation: manifest completeness, files vs
//! manifest (WAV read back with `hound`, an independent parser; other
//! lossless containers checked against the stored integer samples; every
//! file's magic bytes against its declared container) and determinism.

use std::collections::BTreeSet;
use std::io::BufReader;
use std::path::Path;

use sonar_core::fixtures::manifest::{Container, Expect, Format, Group, Kind, Mode, SampleKind};
use sonar_core::fixtures::{generate, stored_pcm, FixtureOptions, Manifest, MANIFEST_FILE};

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

/// Extension and magic bytes agree with the declared container.
fn check_container(path: &str, fmt: &Format, b: &[u8]) {
    let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
    let (want, magic_ok) = match ext {
        "wav" => (Container::Wav, &b[..4] == b"RIFF" && &b[8..12] == b"WAVE"),
        "aif" | "aiff" => (Container::Aiff, &b[..4] == b"FORM" && &b[8..12] == b"AIFF"),
        "flac" => (Container::Flac, &b[..4] == b"fLaC"),
        "mp3" => (Container::Mp3, b[0] == 0xFF && b[1] & 0xE0 == 0xE0),
        "ogg" => (Container::Ogg, &b[..4] == b"OggS"),
        "m4a" => (Container::M4a, &b[4..8] == b"ftyp"),
        other => panic!("{path}: unexpected extension {other}"),
    };
    assert_eq!(fmt.container, want, "{path}");
    assert!(magic_ok, "{path}: magic bytes do not match {want:?}");
    assert_eq!(fmt.lossless, fmt.codec.is_lossless(), "{path}");
    assert_eq!(fmt.lossless, fmt.bit_depth.is_some(), "{path}");
    assert_eq!(fmt.format_tag.is_some(), want == Container::Wav, "{path}");
}

/// Stored samples as f64 full scale (ints divided by 2^(bits−1)), checked
/// against `fmt`; `None` for lossy files.
fn stored_samples(dir: &Path, m: &Manifest, path: &str, fmt: &Format) -> Option<Vec<f64>> {
    if fmt.container == Container::Wav {
        let (spec, frames, samples) = read_samples(&dir.join(path));
        assert_eq!(spec.sample_rate, fmt.sample_rate, "{path}");
        assert_eq!(spec.channels, fmt.channels, "{path}");
        assert_eq!(Some(spec.bits_per_sample), fmt.bit_depth, "{path}");
        let is_float = spec.sample_format == hound::SampleFormat::Float;
        let want_float = fmt.sample_format == Some(SampleKind::Float);
        assert_eq!(is_float, want_float, "{path}");
        assert_eq!(u64::from(frames), fmt.num_frames, "{path}");
        return Some(samples);
    }
    if !fmt.lossless {
        return None;
    }
    let pcm = stored_pcm(path, m.seed).unwrap_or_else(|| panic!("{path}: no stored pcm"));
    assert_eq!(pcm.sample_rate, fmt.sample_rate, "{path}");
    assert_eq!(pcm.channels, fmt.channels, "{path}");
    assert_eq!(Some(pcm.bits), fmt.bit_depth, "{path}");
    assert_eq!(fmt.sample_format, Some(SampleKind::Int), "{path}");
    assert_eq!(pcm.frames() as u64, fmt.num_frames, "{path}");
    let scale = pcm.scale();
    Some(pcm.samples.iter().map(|&q| f64::from(q) / scale).collect())
}

fn check_files_match_manifest(dir: &Path, m: &Manifest) {
    for f in &m.fixtures {
        let path = dir.join(&f.path);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", f.path));
        assert_eq!(bytes.len() as u64, f.bytes, "{}", f.path);
        let Some(fmt) = &f.format else {
            assert_eq!(f.group, Group::Corrupt, "{} has no format", f.path);
            continue;
        };
        check_container(&f.path, fmt, &bytes);
        let dur = fmt.num_frames as f64 / f64::from(fmt.sample_rate);
        assert!((dur - fmt.duration_s).abs() < 1e-6, "{}", f.path);

        let Some(samples) = stored_samples(dir, m, &f.path, fmt) else {
            // Lossy fixtures are digital silence: no level values.
            assert_eq!(f.truth.is_silent, Some(true), "{}", f.path);
            assert!(f.truth.peak_dbfs.is_none() && f.truth.rms_dbfs.is_none());
            continue;
        };
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
        .filter_map(|x| Some((x.bit_depth?, x.sample_rate, x.channels)))
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
    assert_eq!(groups.len(), Group::ALL.len());

    // Every scanner extension is represented.
    let exts: BTreeSet<&str> = m
        .fixtures
        .iter()
        .filter_map(|f| f.path.rsplit_once('.').map(|(_, e)| e))
        .collect();
    for ext in sonar_core::scan::AUDIO_EXTENSIONS {
        assert!(exts.contains(ext), "no .{ext} fixture");
    }
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

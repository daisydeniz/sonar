//! Test library for the duplicate finder: the `sonar fixtures` set plus,
//! generated at test time under `dupes/<sound>/`, variants of a few sounds
//! (byte copy, resampled, other bit depth / channel count, gain, FLAC,
//! low-pass + encoder-like padding). Every file gets a *label*: files with
//! the same label are the same sound, so they may (variants: must) group;
//! files with different labels must never group. Scanned and analyzed by
//! the real job pipeline once per test binary.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::fixtures::drums::Pattern;
use crate::fixtures::manifest::{Category, Fixture, Kind};
use crate::fixtures::music::Key;
use crate::fixtures::oneshot::{DrumHit, KickParams};
use crate::fixtures::render::render;
use crate::fixtures::rng::SplitMix64;
use crate::fixtures::spec::Content;
use crate::fixtures::wav::{encode, SampleFormat, WavSpec};
use crate::fixtures::{flac, generate, FixtureOptions, Pcm};
use crate::jobs::{self, JobOptions};
use crate::scan::scan_library;
use crate::Store;

/// Reference format every variant is derived from.
const REF_RATE: u32 = 44_100;

/// Variant names written for every sound (`copy` is a byte copy of `orig`).
pub(super) const VARIANTS: [&str; 8] = [
    "orig", "copy", "r48_m24", "r96_f32", "gain_m8", "flac16", "lowpass", "padded",
];

/// The analyzed library.
pub(super) struct TestLib {
    _tmp: tempfile::TempDir,
    /// Store database.
    pub db: PathBuf,
    /// Label per file id.
    pub labels: HashMap<i64, String>,
    /// Library-relative path per file id.
    pub paths: HashMap<i64, String>,
}

impl TestLib {
    /// File id of the library-relative `rel` path.
    pub fn id_of(&self, rel: &str) -> i64 {
        self.paths
            .iter()
            .find(|(_, p)| p.as_str() == rel)
            .map(|(&id, _)| id)
            .unwrap_or_else(|| panic!("{rel} not in the library"))
    }
}

/// The shared library (built on first use).
pub(super) fn lib() -> &'static TestLib {
    static LIB: OnceLock<TestLib> = OnceLock::new();
    LIB.get_or_init(build)
}

/// Sounds with variants: `(name, content)`. Each has hard negatives in the
/// fixture set: other kicks, plucks of other notes with the same length, a
/// chord loop in another key at the same tempo, another drum pattern at
/// the same tempo and length.
pub(super) fn sounds() -> Vec<(&'static str, Content)> {
    vec![
        (
            "kick",
            Content::Hit(DrumHit::Kick(KickParams {
                f_start: 150.0,
                f_end: 50.0,
                sweep_tau: 0.03,
                decay_tau: 0.2,
                secs: 0.7,
            })),
        ),
        (
            "pluck",
            Content::Pluck {
                midi: 62,
                secs: 1.5,
            },
        ),
        (
            "chords",
            Content::Chords {
                key: Key::minor(2),
                bpm: 100.0,
                bars: 2,
            },
        ),
        (
            "drums",
            Content::DrumLoop {
                bpm: 120.0,
                bars: 4,
                pattern: Pattern::Backbeat,
            },
        ),
    ]
}

fn build() -> TestLib {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("lib");
    let opts = FixtureOptions {
        include_long: false,
        ..FixtureOptions::default()
    };
    let manifest = generate(&root.join("fixtures"), &opts).expect("fixtures");
    for (name, content) in sounds() {
        write_variants(&root.join("dupes").join(name), &content);
    }
    let db = tmp.path().join("sonar.db");
    let mut store = Store::open(&db).expect("store");
    let library = store.add_library(&root).expect("library");
    scan_library(&mut store, library.id).expect("scan");
    jobs::start(&db, JobOptions::default())
        .expect("start")
        .wait()
        .expect("analyze");
    let ids: Vec<i64> = store
        .conn()
        .prepare("SELECT id FROM files")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .expect("file ids");
    let (mut labels, mut paths) = (HashMap::new(), HashMap::new());
    for id in ids {
        let path = store.file_path(id).expect("path").expect("known file");
        let rel = rel_path(&path, &library.root_path);
        let label = match rel.strip_prefix("fixtures/") {
            Some(f) => {
                let fx = manifest.fixtures.iter().find(|x| x.path == f);
                let ms = store.audio_meta(id).expect("meta").map(|m| m.duration_ms);
                fixture_label(fx.expect("manifest entry"), ms)
            }
            None => rel.split('/').nth(1).expect("dupes/<sound>/").to_owned(),
        };
        labels.insert(id, label);
        paths.insert(id, rel);
    }
    TestLib {
        _tmp: tmp,
        db,
        labels,
        paths,
    }
}

/// Fixtures that are the same sound share a label: every 1 s A4 sine
/// (format matrix, lossless containers, the −60 dBFS one), plucks of the
/// same note and length (tonal group and AIFF), the partially decodable
/// corrupt files (one base tone) and digital silence; everything else is
/// its own sound.
fn fixture_label(f: &Fixture, duration_ms: Option<u64>) -> String {
    let d = &f.description;
    let t = &f.truth;
    if d.starts_with("A4 sine 1 s") || d.starts_with("very quiet A4 sine") {
        "a4-sine-1s".into()
    } else if t.is_silent == Some(true) {
        "silence".into()
    } else if t.corrupt.is_some() {
        "corrupt-base".into()
    } else if t.category == Some(Category::Synth) && t.kind == Some(Kind::OneShot) {
        format!("pluck-{:?}-{duration_ms:?}", t.note_midi)
    } else {
        f.path.clone()
    }
}

fn rel_path(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).expect("under the root");
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

/// Writes every [`VARIANTS`] file of `content` into `dir`.
fn write_variants(dir: &Path, content: &Content) {
    fs::create_dir_all(dir.join("copy")).expect("mkdir");
    let spec = WavSpec::new(REF_RATE, 2, SampleFormat::Int16);
    let (audio, _) = render(content, &spec, &mut SplitMix64::new(7));
    let reference = peak_normalize(&audio.channels, -1.0);
    let write = |name: &str, bytes: &[u8]| fs::write(dir.join(name), bytes).expect("write");
    let orig = encode(&spec, &interleave(&reference)).bytes;
    write("orig.wav", &orig);
    write("copy/orig.wav", &orig);
    let mono = vec![mix_down(&reference)];
    let r48 = WavSpec::new(48_000, 1, SampleFormat::Int24);
    let x = interleave(&resample(&mono, REF_RATE, 48_000));
    write("r48_m24.wav", &encode(&r48, &x).bytes);
    let r96 = WavSpec::new(96_000, 2, SampleFormat::Float32);
    let x = interleave(&resample(&reference, REF_RATE, 96_000));
    write("r96_f32.wav", &encode(&r96, &x).bytes);
    let quieter = scale(&reference, 10f32.powf(-8.0 / 20.0));
    write("gain_m8.wav", &encode(&spec, &interleave(&quieter)).bytes);
    let pcm = Pcm::quantize(&spec, &interleave(&reference));
    write("flac16.flac", &flac::encode(&pcm));
    let lp = low_pass(&reference, 11_000.0, REF_RATE);
    write("lowpass.wav", &encode(&spec, &interleave(&lp)).bytes);
    // Lossy-decoder-like padding: 12 ms of encoder delay, 25 ms at the end.
    let padded = pad(&reference, ms_frames(12), ms_frames(25));
    write("padded.wav", &encode(&spec, &interleave(&padded)).bytes);
}

fn ms_frames(ms: u32) -> usize {
    (REF_RATE * ms / 1000) as usize
}

fn peak_normalize(chans: &[Vec<f32>], peak_db: f32) -> Vec<Vec<f32>> {
    let peak = chans
        .iter()
        .flatten()
        .fold(0.0f32, |m, &x| m.max(x.abs()))
        .max(1e-9);
    scale(chans, 10f32.powf(peak_db / 20.0) / peak)
}

fn scale(chans: &[Vec<f32>], g: f32) -> Vec<Vec<f32>> {
    chans
        .iter()
        .map(|c| c.iter().map(|&x| x * g).collect())
        .collect()
}

fn interleave(chans: &[Vec<f32>]) -> Vec<f32> {
    let n = chans.first().map_or(0, Vec::len);
    (0..n)
        .flat_map(|i| chans.iter().map(move |c| c[i]))
        .collect()
}

fn mix_down(chans: &[Vec<f32>]) -> Vec<f32> {
    let n = chans.first().map_or(0, Vec::len);
    let k = chans.len().max(1) as f32;
    (0..n)
        .map(|i| chans.iter().map(|c| c[i]).sum::<f32>() / k)
        .collect()
}

/// Linear-interpolation resampler (a crude but real rate conversion).
fn resample(chans: &[Vec<f32>], from: u32, to: u32) -> Vec<Vec<f32>> {
    chans
        .iter()
        .map(|c| {
            let n = (c.len() as u64 * u64::from(to) / u64::from(from)) as usize;
            (0..n)
                .map(|j| {
                    let t = j as f64 * f64::from(from) / f64::from(to);
                    let i = t.floor() as usize;
                    let f = (t - i as f64) as f32;
                    let a = c.get(i).copied().unwrap_or(0.0);
                    let b = c.get(i + 1).copied().unwrap_or(a);
                    a + (b - a) * f
                })
                .collect()
        })
        .collect()
}

/// Two cascaded one-pole low-passes at `hz` (an MP3-like top-end roll-off).
fn low_pass(chans: &[Vec<f32>], hz: f32, rate: u32) -> Vec<Vec<f32>> {
    let a = (-std::f32::consts::TAU * hz / rate as f32).exp();
    let pass = |x: &[f32]| -> Vec<f32> {
        let mut y = 0.0;
        x.iter()
            .map(|&v| {
                y = (1.0 - a) * v + a * y;
                y
            })
            .collect()
    };
    chans.iter().map(|c| pass(&pass(c))).collect()
}

fn pad(chans: &[Vec<f32>], front: usize, back: usize) -> Vec<Vec<f32>> {
    chans
        .iter()
        .map(|c| {
            let mut v = vec![0.0; front];
            v.extend_from_slice(c);
            v.resize(v.len() + back, 0.0);
            v
        })
        .collect()
}

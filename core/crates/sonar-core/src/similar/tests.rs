//! Fixture-library tests of similarity search: `sonar fixtures` output is
//! scanned and analyzed by the real job pipeline once per test binary,
//! then queried through [`similar`] / [`rank`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::*;
use crate::dsp::aspects::Aspect;
use crate::fixtures::manifest::{Category, Expect, Fixture, Group, Kind};
use crate::fixtures::{generate, FixtureOptions};
use crate::index::vectors::dir_for_db;
use crate::jobs::{self, JobOptions};
use crate::scan::scan_library;
use crate::Store;

/// The analyzed fixture library shared by every test.
struct Lib {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    db: PathBuf,
    /// Every scanned file (with or without vectors).
    fixtures: HashMap<i64, Fixture>,
    /// Nothing hidden.
    index: TextIndex,
    /// Every `formats/` file hidden.
    index_hidden: TextIndex,
}

impl Lib {
    fn vs(&self) -> VectorStore {
        VectorStore::open_read(dir_for_db(&self.db)).expect("vector store")
    }

    fn id_of(&self, rel: &str) -> i64 {
        self.fixtures
            .iter()
            .find(|(_, f)| f.path == rel)
            .map(|(&id, _)| id)
            .unwrap_or_else(|| panic!("{rel} not in the library"))
    }

    fn fixture(&self, id: i64) -> &Fixture {
        &self.fixtures[&id]
    }

    /// Sorted ids of files with vectors matching `keep`.
    fn ids(&self, keep: impl Fn(&Fixture) -> bool) -> Vec<i64> {
        let vs = self.vs();
        let mut ids: Vec<i64> = self
            .fixtures
            .iter()
            .filter(|(&id, f)| keep(f) && vs.row_of(id).is_some())
            .map(|(&id, _)| id)
            .collect();
        ids.sort_unstable();
        ids
    }
}

fn lib() -> &'static Lib {
    static LIB: OnceLock<Lib> = OnceLock::new();
    LIB.get_or_init(|| {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join("lib");
        let opts = FixtureOptions {
            include_long: false,
            ..FixtureOptions::default()
        };
        let manifest = generate(&root, &opts).expect("fixtures");
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
        let mut fixtures = HashMap::new();
        for id in ids {
            let path = store.file_path(id).expect("path").expect("known file");
            let rel = rel_path(&path, &library.root_path);
            let f = manifest.fixtures.iter().find(|f| f.path == rel);
            fixtures.insert(id, f.expect("manifest entry").clone());
        }
        let index = TextIndex::in_ram().expect("index");
        index.rebuild(&store).expect("index build");
        for (&id, f) in &fixtures {
            if f.group == Group::Formats {
                store
                    .conn()
                    .execute("INSERT INTO hidden (file_id) VALUES (?1)", [id])
                    .expect("hide");
            }
        }
        let index_hidden = TextIndex::in_ram().expect("index");
        index_hidden.rebuild(&store).expect("index build");
        Lib {
            _tmp: tmp,
            root,
            db,
            fixtures,
            index,
            index_hidden,
        }
    })
}

fn rel_path(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).expect("under the root");
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

/// Sound family from the manifest truth (what "similar" should mean on
/// synthetic fixtures).
fn family(f: &Fixture) -> &'static str {
    match (f.truth.category, f.truth.kind) {
        (Some(Category::Sine), Some(Kind::Tone)) => "sine",
        (Some(Category::Sine), _) => "sine burst",
        (Some(Category::Synth), Some(Kind::OneShot)) => "pluck",
        (Some(Category::Synth), Some(Kind::Loop)) => "chords",
        (Some(Category::Synth), _) => "pad",
        (Some(Category::Kick), _) => "kick",
        (Some(Category::Snare), _) => "snare",
        (Some(Category::HatClosed | Category::HatOpen), _) => "hat",
        (Some(Category::Drums), _) => "drum loop",
        (Some(Category::Click), _) => "click",
        (Some(Category::Noise), _) => "noise",
        _ => "other",
    }
}

fn run(seed: Seed, opts: &SimilarOptions) -> SimilarResults {
    let lib = lib();
    similar(&lib.vs(), &lib.index, &seed, opts).expect("similar")
}

fn opts(limit: usize, weights: AspectWeights) -> SimilarOptions {
    SimilarOptions {
        limit,
        weights,
        ..SimilarOptions::new()
    }
}

fn hit_ids(r: &SimilarResults) -> Vec<i64> {
    r.hits.iter().map(|h| h.file_id).collect()
}

/// AC sanity: for every tonal note (sines, plucks) and drum one-shot, the
/// nearest neighbour under the default weights has the seed's family
/// (sine / pluck / kick / snare / hat), and so do most of the top 3.
#[test]
fn same_family_ranks_first_on_fixtures() {
    let lib = lib();
    let seeds = lib.ids(|f| matches!(f.group, Group::Tonal | Group::Oneshots));
    let (mut top1, mut top3) = (0, 0);
    let mut misses = Vec::new();
    for &id in &seeds {
        let want = family(lib.fixture(id));
        let r = run(Seed::File(id), &opts(3, AspectWeights::default()));
        let fams: Vec<&str> = r
            .hits
            .iter()
            .map(|h| family(lib.fixture(h.file_id)))
            .collect();
        if fams.first() == Some(&want) {
            top1 += 1;
        } else {
            misses.push(format!("{} ({want}) → {fams:?}", lib.fixture(id).path));
        }
        top3 += usize::from(fams.iter().filter(|&&f| f == want).count() >= 2);
    }
    let n = seeds.len();
    eprintln!("similar families: top-1 {top1}/{n}, top-3 majority {top3}/{n}; misses {misses:?}");
    assert!(n >= 30, "{n} seeds");
    assert!(top1 + 1 >= n, "top-1 {top1}/{n}: {misses:?}");
    assert!(top3 * 10 >= n * 8, "top-3 majority {top3}/{n}");
}

/// Pitch-only weights among the pitched notes: the nearest note shares
/// the seed's pitch class (same note or octave) for most seeds.
#[test]
fn pitch_weight_finds_the_same_pitch_class() {
    let lib = lib();
    let notes = lib.ids(|f| f.truth.note_midi.is_some() && f.group != Group::Formats);
    let vs = lib.vs();
    let o = opts(1, AspectWeights::only(Aspect::Pitch));
    let tonal: Vec<i64> = notes
        .iter()
        .copied()
        .filter(|&id| lib.fixture(id).group == Group::Tonal)
        .collect();
    let mut same = 0;
    for &id in &tonal {
        let seed = vs.vectors(id).expect("vectors");
        let r = rank(&vs, &seed, Some(id), &notes, &o).expect("rank");
        let midi = |id: i64| lib.fixture(id).truth.note_midi.map_or(-1, i32::from);
        let hit = r.hits.first().map_or(-100, |h| midi(h.file_id));
        same += usize::from((hit - midi(id)).rem_euclid(12) == 0);
    }
    eprintln!("pitch-only: same pitch class top-1 {same}/{}", tonal.len());
    assert!(same * 10 >= tonal.len() * 8, "{same}/{}", tonal.len());
}

/// The reported numbers follow the documented formula, hits are sorted,
/// the seed is never a hit, and `candidates` counts every other file
/// with vectors.
#[test]
fn distances_follow_the_formula_and_exclude_the_seed() {
    let lib = lib();
    let id = lib.id_of("oneshots/hit_000.wav");
    let r = run(Seed::File(id), &opts(10, AspectWeights::default()));
    assert_eq!(r.hits.len(), 10);
    assert_eq!(r.seed_file_id, Some(id));
    assert!(!hit_ids(&r).contains(&id));
    assert_eq!(r.candidates, lib.ids(|_| true).len() - 1);
    for pair in r.hits.windows(2) {
        assert!(pair[0].distance <= pair[1].distance);
    }
    for h in &r.hits {
        let a = h.aspects;
        let mean = (a.spectrum + a.timbre + a.pitch + a.amplitude) / 4.0;
        assert!((h.distance - mean).abs() < 1e-5, "{h:?}");
    }
    let w: AspectWeights = "timbre=3,pitch=1".parse().expect("weights");
    for h in run(Seed::File(id), &opts(5, w)).hits {
        let want = (3.0 * h.aspects.timbre + h.aspects.pitch) / 4.0;
        assert!((h.distance - want).abs() < 1e-5, "{h:?}");
    }
}

/// Different weights, different neighbours; a single-aspect query ranks
/// by that aspect's distance alone.
#[test]
fn weights_change_the_ranking() {
    let lib = lib();
    let id = lib.id_of("tonal/tone_000.wav");
    let pitch = run(Seed::File(id), &opts(5, AspectWeights::only(Aspect::Pitch)));
    let amp = run(
        Seed::File(id),
        &opts(5, AspectWeights::only(Aspect::Amplitude)),
    );
    assert_ne!(hit_ids(&pitch), hit_ids(&amp));
    for h in &pitch.hits {
        assert!((h.distance - h.aspects.pitch).abs() < 1e-6, "{h:?}");
    }
    for h in &amp.hits {
        assert!((h.distance - h.aspects.amplitude).abs() < 1e-6, "{h:?}");
    }
    // Scaling every weight changes nothing.
    let a = run(Seed::File(id), &opts(5, AspectWeights::default()));
    let b = run(Seed::File(id), &opts(5, AspectWeights::overall(7.0)));
    assert_eq!(hit_ids(&a), hit_ids(&b));
}

/// An external copy of a library file (analyzed on the fly by the dsp
/// stage's code path) finds that file first at distance ≈ 0.
#[test]
fn external_copy_of_a_library_file_finds_it_first() {
    let lib = lib();
    for rel in [
        "oneshots/hit_000.wav",
        "tonal/tone_009.wav",
        "chords/prog_000.wav",
        "loops/loop_000.wav",
    ] {
        let id = lib.id_of(rel);
        let r = run(
            Seed::External(lib.root.join(rel)),
            &opts(3, AspectWeights::default()),
        );
        assert_eq!(r.seed_file_id, None);
        // First, or tied at ≈ 0 with an identical copy (tone_009 has one
        // in `containers/`; ties order by file id).
        let zero: Vec<i64> = r
            .hits
            .iter()
            .take_while(|h| h.distance < 1e-4)
            .map(|h| h.file_id)
            .collect();
        assert!(zero.contains(&id), "{rel}: {:?}", r.hits);
    }
}

/// Filters restrict the candidates exactly like text search; hidden files
/// only come back with `include_hidden`.
#[test]
fn filters_restrict_candidates() {
    let lib = lib();
    let vs = lib.vs();
    let id = lib.id_of("tonal/tone_013.wav");
    let filters = Filters {
        kinds: vec!["loop".into()],
        ..Filters::default()
    };
    let allowed = filtered_file_ids(&lib.index, &filters).expect("allowed");
    let o = SimilarOptions {
        limit: 1000,
        filters,
        ..SimilarOptions::new()
    };
    let r = run(Seed::File(id), &o);
    assert!(!r.hits.is_empty() && r.hits.len() < lib.ids(|_| true).len() / 2);
    assert!(r.hits.iter().all(|h| allowed.contains(&h.file_id)));

    let formats = lib.ids(|f| f.group == Group::Formats);
    let count_formats = |r: &SimilarResults| {
        r.hits
            .iter()
            .filter(|h| formats.contains(&h.file_id))
            .count()
    };
    let mut o = opts(1000, AspectWeights::default());
    let hidden = similar(&vs, &lib.index_hidden, &Seed::File(id), &o).expect("similar");
    assert_eq!(count_formats(&hidden), 0);
    o.filters.include_hidden = true;
    let shown = similar(&vs, &lib.index_hidden, &Seed::File(id), &o).expect("similar");
    assert_eq!(count_formats(&shown), formats.len());
    assert!(!formats.is_empty());
}

/// Bad seeds and parameters are error values, never panics.
#[test]
fn bad_seeds_and_options_are_errors() {
    let lib = lib();
    let vs = lib.vs();
    let o = SimilarOptions::new();
    let err = |seed: Seed, o: &SimilarOptions| similar(&vs, &lib.index, &seed, o).err();
    assert!(matches!(
        err(Seed::File(999_999), &o),
        Some(Error::NoVectors { file_id: 999_999 })
    ));
    // A corrupt file is in the store but has no vectors; on the fly it
    // fails to decode.
    let corrupt: Vec<(i64, &Fixture)> = lib
        .fixtures
        .iter()
        .filter(|(_, f)| {
            f.truth
                .corrupt
                .as_ref()
                .is_some_and(|c| c.expect == Expect::Error)
        })
        .map(|(&id, f)| (id, f))
        .collect();
    assert!(!corrupt.is_empty());
    for (id, f) in corrupt {
        assert!(matches!(
            err(Seed::File(id), &o),
            Some(Error::NoVectors { .. })
        ));
        let e = err(Seed::External(lib.root.join(&f.path)), &o);
        assert!(e.is_some(), "{}", f.path);
    }
    let missing = Seed::External(lib.root.join("nope.wav"));
    assert!(matches!(err(missing, &o), Some(Error::Io { .. })));
    let id = lib.id_of("oneshots/hit_000.wav");
    let zero = opts(5, AspectWeights::overall(0.0));
    assert!(matches!(
        err(Seed::File(id), &zero),
        Some(Error::InvalidQuery { .. })
    ));
    let cancelled = SimilarOptions {
        cancel: Some(Arc::new(AtomicBool::new(true))),
        ..SimilarOptions::new()
    };
    assert!(matches!(
        err(Seed::File(id), &cancelled),
        Some(Error::Cancelled)
    ));
}

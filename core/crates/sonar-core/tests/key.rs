//! Key and note accuracy on the generated fixtures (`sonar fixtures`), with
//! filenames hidden (no key prior, no kind hint), gated by the loop/one-shot
//! classifier exactly as the pipeline does. AC (T3.5, T10.2):
//! - key ≥ 90 % on fixtures with `truth.key` (tonic pitch class + mode);
//! - note ≥ 95 % (exact MIDI) on tones/one-shots with `truth.note_midi`;
//! - drums, clicks and noise get no note and a key below [`LOW_TIER`];
//!   silence gets neither;
//! - fusion with a filename key on fixture audio: agree / disagree /
//!   name-only / silence.
//!
//! Run with `--nocapture` to see the per-file table.

use std::path::Path;

use sonar_core::classify::{classify_kind, KindResult};
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::{Analyzer, Features};
use sonar_core::fixtures::manifest::{Category, Fixture, Kind as TruthKind, Mode as TruthMode};
use sonar_core::fixtures::{generate, FixtureOptions};
use sonar_core::key::{
    key_and_note, present_classes, tonal_chroma, KeyNote, KeySource, NoteTracker,
    NAME_CONF_EXPLICIT,
};
use sonar_core::text::{Key, KeyPrior, Mode, PriorSource};

/// Required key accuracy (tonic + mode).
const MIN_KEY_ACCURACY: f64 = 0.90;
/// Required note accuracy (exact MIDI).
const MIN_NOTE_ACCURACY: f64 = 0.95;
/// Stricter than the shipped low-tier bound (`KEY_TIER_MEDIUM` 0.60, T10.4b):
/// unpitched material must stay below it.
const LOW_TIER: f32 = 0.3;

/// Decoded, analysed and classified fixture.
struct Analysed {
    samples: Vec<f32>,
    feats: Features,
    kind: KindResult,
}

struct Ctx {
    decoder: Decoder,
    analyzer: Analyzer,
    tracker: NoteTracker,
}

impl Ctx {
    fn new() -> Self {
        Self {
            decoder: Decoder::new(),
            analyzer: Analyzer::new(ANALYSIS_RATE).expect("analyzer"),
            tracker: NoteTracker::new(ANALYSIS_RATE).expect("tracker"),
        }
    }

    fn analyse(&mut self, path: &Path) -> Analysed {
        let d = self
            .decoder
            .decode(path, &DecodeOptions::default())
            .unwrap_or_else(|e| panic!("decode {}: {e}", path.display()));
        let feats = self.analyzer.analyze(&d.samples).expect("features");
        let kind = classify_kind(&feats, d.source.duration_s, None);
        Analysed {
            samples: d.samples,
            feats,
            kind,
        }
    }

    fn key(&mut self, a: &Analysed, prior: Option<KeyPrior>) -> KeyNote {
        key_and_note(&a.kind, &a.feats, &a.samples, &mut self.tracker, prior).expect("key")
    }
}

fn unpitched(f: &Fixture) -> bool {
    matches!(f.truth.kind, Some(TruthKind::Noise))
        || matches!(
            f.truth.category,
            Some(
                Category::Kick
                    | Category::Snare
                    | Category::HatClosed
                    | Category::HatOpen
                    | Category::Drums
                    | Category::Click
                    | Category::Noise
            )
        )
}

fn truth_key(f: &Fixture) -> Option<Key> {
    f.truth.key.as_ref().map(|k| Key {
        root: k.tonic_pc,
        mode: Some(match k.mode {
            TruthMode::Major => Mode::Major,
            TruthMode::Minor => Mode::Minor,
        }),
    })
}

fn name(key: Key) -> Option<KeyPrior> {
    Some(KeyPrior {
        key,
        explicit: true,
        source: PriorSource::Filename,
    })
}

#[test]
fn fixture_keys_and_notes_without_filenames() {
    let tmp = tempfile::tempdir().expect("tmp");
    let manifest = generate(tmp.path(), &FixtureOptions::default()).expect("fixtures");
    let mut ctx = Ctx::new();
    let (mut keys, mut keys_ok, mut notes, mut notes_ok) = (0usize, 0usize, 0usize, 0usize);
    let (mut key_misses, mut note_misses, mut bad_unpitched) = (Vec::new(), Vec::new(), Vec::new());
    let (mut n_unpitched, mut max_unpitched) = (0usize, 0.0f32);
    for f in &manifest.fixtures {
        if f.truth.corrupt.is_some() {
            continue;
        }
        let a = ctx.analyse(&tmp.path().join(&f.path));
        let kn = ctx.key(&a, None);
        let tc = tonal_chroma(&a.feats.spectrogram);
        println!(
            "{:<26} {:?}/{:?} tonal {:.2} pcs {:>2} tuning {:+.2} key {:>5} conf {:>4} {:?} note {:>4} ({}) truth key {:?} note {:?}",
            f.path,
            f.truth.kind,
            a.kind.kind,
            tc.tonal_ratio,
            present_classes(&tc.chroma),
            tc.tuning,
            kn.key.map_or("-".into(), |k| k.key.to_string()),
            kn.key.map_or("-".into(), |k| format!("{:.2}", k.confidence)),
            kn.key.map(|k| k.source),
            kn.note.map_or("-".into(), |n| n.midi.to_string()),
            kn.note.map_or("-".into(), |n| format!("{:+.0}ct {:.2}", n.cents, n.confidence)),
            f.truth.key.as_ref().map(|k| (k.tonic_pc, k.mode)),
            f.truth.note_midi,
        );
        if let Some(want) = truth_key(f) {
            keys += 1;
            let ok = kn.key.is_some_and(|k| k.key == want);
            keys_ok += usize::from(ok);
            if !ok {
                key_misses.push(f.path.clone());
            }
        }
        let tonal_kind = matches!(f.truth.kind, Some(TruthKind::Tone | TruthKind::OneShot));
        if let (Some(want), true) = (f.truth.note_midi, tonal_kind) {
            notes += 1;
            let ok = kn.note.is_some_and(|n| n.midi == want);
            notes_ok += usize::from(ok);
            if !ok {
                note_misses.push(f.path.clone());
            }
        }
        if f.truth.kind == Some(TruthKind::Silence) && kn != KeyNote::default() {
            bad_unpitched.push(f.path.clone());
        } else if unpitched(f) {
            n_unpitched += 1;
            let conf = kn.key.map_or(0.0, |k| k.confidence);
            max_unpitched = max_unpitched.max(conf);
            if kn.note.is_some() || conf >= LOW_TIER || kn.key.is_none() {
                bad_unpitched.push(f.path.clone());
            }
        }
    }
    let key_acc = keys_ok as f64 / keys.max(1) as f64;
    let note_acc = notes_ok as f64 / notes.max(1) as f64;
    println!(
        "key: {keys_ok}/{keys} = {:.1} % misses {key_misses:?}\nnote: {notes_ok}/{notes} = {:.1} % misses {note_misses:?}\nunpitched: {n_unpitched}, max key confidence {max_unpitched:.3}; failing {bad_unpitched:?}",
        key_acc * 100.0,
        note_acc * 100.0
    );
    assert!(
        keys >= 16 && notes >= 40 && n_unpitched >= 10,
        "too few fixtures: {keys} keys, {notes} notes, {n_unpitched} unpitched"
    );
    assert!(
        bad_unpitched.is_empty(),
        "unpitched/silent with a note, no key or a key ≥ {LOW_TIER}: {bad_unpitched:?}"
    );
    assert!(
        key_acc >= MIN_KEY_ACCURACY,
        "key {key_acc:.3}: {key_misses:?}"
    );
    assert!(
        note_acc >= MIN_NOTE_ACCURACY,
        "note {note_acc:.3}: {note_misses:?}"
    );
}

/// Filename keys fused with fixture audio (ADR-041).
#[test]
fn fixture_audio_fused_with_names() {
    let tmp = tempfile::tempdir().expect("tmp");
    let manifest = generate(tmp.path(), &FixtureOptions::default()).expect("fixtures");
    let mut ctx = Ctx::new();
    let find = |pred: &dyn Fn(&Fixture) -> bool| {
        manifest
            .fixtures
            .iter()
            .find(|f| f.truth.corrupt.is_none() && pred(f))
            .expect("fixture")
    };

    // Agree / disagree on a keyed fixture.
    let keyed = find(&|f| truth_key(f).is_some());
    let want = truth_key(keyed).expect("truth");
    let a = ctx.analyse(&tmp.path().join(&keyed.path));
    let audio = ctx.key(&a, None).key.expect("audio key");
    assert_eq!((audio.key, audio.source), (want, KeySource::Detected));
    let agree = ctx.key(&a, name(want)).key.expect("agree");
    assert_eq!((agree.key, agree.source), (want, KeySource::Both));
    assert!(agree.confidence > audio.confidence, "{agree:?} {audio:?}");
    let tritone = Key {
        root: (want.root + 6) % 12,
        mode: want.mode,
    };
    let disagree = ctx.key(&a, name(tritone)).key.expect("disagree");
    assert_eq!(disagree.key, want, "{disagree:?}");
    assert_eq!(disagree.source, KeySource::Detected);
    assert!(
        disagree.confidence < audio.confidence,
        "{disagree:?} {audio:?}"
    );

    // Name only: a name on drum audio wins but stays low-medium.
    let drum = find(&|f| unpitched(f) && f.truth.kind != Some(TruthKind::Noise));
    let a = ctx.analyse(&tmp.path().join(&drum.path));
    let named = ctx.key(&a, name(tritone)).key.expect("name");
    assert_eq!((named.key, named.source), (tritone, KeySource::Filename));
    assert!(
        named.confidence >= LOW_TIER && named.confidence <= NAME_CONF_EXPLICIT,
        "{named:?}"
    );

    // Silence: nothing, even with a name.
    let silent = find(&|f| f.truth.kind == Some(TruthKind::Silence));
    let a = ctx.analyse(&tmp.path().join(&silent.path));
    assert_eq!(ctx.key(&a, name(want)), KeyNote::default());
}

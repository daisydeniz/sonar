//! Key and note accuracy on the generated fixtures (`sonar fixtures`), with
//! filenames hidden (no key prior, no kind hint), gated by the loop/one-shot
//! classifier exactly as the pipeline does. AC (T3.5):
//! - key ≥ 90 % on fixtures with `truth.key` (tonic pitch class + mode);
//! - note ≥ 95 % (exact MIDI) on tones/one-shots with `truth.note_midi`;
//! - drums, clicks, noise and silence get neither a key nor a note.
//!
//! Run with `--nocapture` to see the per-file table.

use sonar_core::classify::classify_kind;
use sonar_core::decode::{DecodeOptions, Decoder, ANALYSIS_RATE};
use sonar_core::dsp::Analyzer;
use sonar_core::fixtures::manifest::{Category, Kind as TruthKind, Mode as TruthMode};
use sonar_core::fixtures::{generate, FixtureOptions};
use sonar_core::key::{key_and_note, present_classes, tonal_chroma, NoteTracker};
use sonar_core::text::Mode;

/// Required key accuracy (tonic + mode).
const MIN_KEY_ACCURACY: f64 = 0.90;
/// Required note accuracy (exact MIDI).
const MIN_NOTE_ACCURACY: f64 = 0.95;

#[test]
fn fixture_keys_and_notes_without_filenames() {
    let tmp = tempfile::tempdir().expect("tmp");
    let manifest = generate(tmp.path(), &FixtureOptions::default()).expect("fixtures");
    let opts = DecodeOptions::default();
    let mut decoder = Decoder::new();
    let mut analyzer = Analyzer::new(ANALYSIS_RATE).expect("analyzer");
    let mut tracker = NoteTracker::new(ANALYSIS_RATE).expect("tracker");
    let (mut keys, mut keys_ok, mut notes, mut notes_ok) = (0usize, 0usize, 0usize, 0usize);
    let (mut key_misses, mut note_misses, mut unpitched) = (Vec::new(), Vec::new(), Vec::new());
    for f in &manifest.fixtures {
        if f.truth.corrupt.is_some() {
            continue;
        }
        let d = decoder
            .decode(&tmp.path().join(&f.path), &opts)
            .unwrap_or_else(|e| panic!("decode {}: {e}", f.path));
        let feats = analyzer.analyze(&d.samples).expect("features");
        let kind = classify_kind(&feats, d.source.duration_s, None);
        let kn = key_and_note(&kind, &feats, &d.samples, &mut tracker, None).expect("key");
        let tc = tonal_chroma(&feats.spectrogram);
        println!(
            "{:<26} {:?}/{:?} tonal {:.2} pcs {:>2} tuning {:+.2} key {:>5} conf {:>4} note {:>4} ({}) truth key {:?} note {:?}",
            f.path,
            f.truth.kind,
            kind.kind,
            tc.tonal_ratio,
            present_classes(&tc.chroma),
            tc.tuning,
            kn.key.map_or("-".into(), |k| k.key.to_string()),
            kn.key.map_or("-".into(), |k| format!("{:.2}", k.confidence)),
            kn.note.map_or("-".into(), |n| n.midi.to_string()),
            kn.note.map_or("-".into(), |n| format!("{:+.0}ct {:.2}", n.cents, n.confidence)),
            f.truth.key.as_ref().map(|k| (k.tonic_pc, k.mode)),
            f.truth.note_midi,
        );
        if let Some(want) = &f.truth.key {
            keys += 1;
            let want_mode = match want.mode {
                TruthMode::Major => Mode::Major,
                TruthMode::Minor => Mode::Minor,
            };
            let ok = kn
                .key
                .is_some_and(|k| k.key.root == want.tonic_pc && k.key.mode == Some(want_mode));
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
        let no_pitch = matches!(f.truth.kind, Some(TruthKind::Noise | TruthKind::Silence))
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
            );
        if no_pitch && (kn.key.is_some() || kn.note.is_some()) {
            unpitched.push(f.path.clone());
        }
    }
    let key_acc = keys_ok as f64 / keys.max(1) as f64;
    let note_acc = notes_ok as f64 / notes.max(1) as f64;
    println!(
        "key: {keys_ok}/{keys} = {:.1} % misses {key_misses:?}\nnote: {notes_ok}/{notes} = {:.1} % misses {note_misses:?}\nunpitched with key/note: {unpitched:?}",
        key_acc * 100.0,
        note_acc * 100.0
    );
    assert!(
        keys >= 16 && notes >= 40,
        "too few fixtures: {keys} keys, {notes} notes"
    );
    assert!(
        unpitched.is_empty(),
        "unpitched with key/note: {unpitched:?}"
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

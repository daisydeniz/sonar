//! Metric definitions of `docs/EVAL.md` ("Metrics"): pure functions plus an
//! [`Accumulator`] that scores observations against labels.
//!
//! - **Loop/one-shot**: accuracy over files labeled `Loop` or `OneShot`.
//! - **BPM** (labeled loops): correct within ±[`BPM_TOL`] BPM; the
//!   octave-error rate (metrical-level ratios, see [`OCTAVE_RATIOS`]) is
//!   reported separately; one-shot-labeled files that receive a BPM are
//!   counted (must be 0).
//! - **Key**: exact (tonic pitch class + mode) and the MIREX weighted score.
//! - **Note** (labeled tonal one-shots): exact pitch class and exact MIDI.

use serde::{Deserialize, Serialize};

use crate::classify::category::{T_DSP, T_FUSED};
use crate::classify::Kind;
use crate::text::{Key, Mode};

use super::category::{CategoryAccumulator, CategoryMetrics, CategoryObs};
use super::dataset::Labels;
use super::similarity::SimilarMetrics;

/// BPM tolerance (BPM) of "correct".
pub const BPM_TOL: f64 = 1.0;
/// Metrical-level ratios counted as octave errors (×½, ×2, ×⅓, ×3, ×⅔,
/// ×3⁄2), each within [`BPM_TOL`].
pub const OCTAVE_RATIOS: [f64; 6] = [0.5, 2.0, 1.0 / 3.0, 3.0, 2.0 / 3.0, 1.5];

/// What the analyzer produced for one file (names hidden).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    /// Loop/one-shot decision.
    pub kind: Kind,
    /// Tempo of a loop.
    pub bpm: Option<f64>,
    /// Key (`mode: None` for a one-shot's pitch class).
    pub key: Option<Key>,
    /// MIDI number of the detected one-shot note.
    pub note_midi: Option<u8>,
    /// Category decisions.
    pub category: Option<CategoryObs>,
}

/// How an estimated BPM relates to the truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpmOutcome {
    /// Within [`BPM_TOL`].
    Correct,
    /// Off by a metrical-level ratio ([`OCTAVE_RATIOS`]).
    Octave,
    /// Anything else, or no estimate.
    Wrong,
}

/// Compares an estimate with the truth BPM.
pub fn bpm_outcome(truth: f64, got: Option<f64>) -> BpmOutcome {
    let Some(g) = got else {
        return BpmOutcome::Wrong;
    };
    if (g - truth).abs() <= BPM_TOL {
        BpmOutcome::Correct
    } else if OCTAVE_RATIOS
        .iter()
        .any(|r| (g - truth * r).abs() <= BPM_TOL)
    {
        BpmOutcome::Octave
    } else {
        BpmOutcome::Wrong
    }
}

/// True if `got` is the same key as `truth`: equal tonic pitch class and,
/// when the truth names a mode, equal mode (a mode-less estimate is then
/// wrong). A mode-less truth only compares the tonic.
pub fn key_exact(truth: Key, got: Key) -> bool {
    truth.root % 12 == got.root % 12 && (truth.mode.is_none() || truth.mode == got.mode)
}

/// MIREX 2005+ weighted key score (MIREX "Audio Key Detection" task):
/// 1.0 correct, 0.5 perfect fifth (the estimate's tonic is a fifth above or
/// below the truth's, same mode), 0.3 relative major/minor, 0.2 parallel
/// major/minor, else 0. Tonics are compared as pitch classes. A mode-less
/// truth scores 1.0 or 0.0 by tonic only; a mode-less estimate of a moded
/// truth scores 0 (its relation is unknown).
pub fn mirex_key_score(truth: Key, got: Key) -> f64 {
    let (t, g) = (i32::from(truth.root % 12), i32::from(got.root % 12));
    let Some(tm) = truth.mode else {
        return f64::from(u8::from(t == g));
    };
    let Some(gm) = got.mode else {
        return 0.0;
    };
    let up = (g - t).rem_euclid(12);
    match (tm, gm) {
        (a, b) if a == b && up == 0 => 1.0,
        (a, b) if a == b && (up == 7 || up == 5) => 0.5,
        (Mode::Major, Mode::Minor) if up == 9 => 0.3,
        (Mode::Minor, Mode::Major) if up == 3 => 0.3,
        (_, _) if up == 0 => 0.2,
        _ => 0.0,
    }
}

/// `100 * num / den`, `None` when `den` is 0.
fn pct(num: usize, den: usize) -> Option<f64> {
    (den > 0).then(|| 100.0 * num as f64 / den as f64)
}

/// Loop/one-shot accuracy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KindMetrics {
    /// Files with a loop/one-shot label.
    pub total: usize,
    /// Correct decisions.
    pub correct: usize,
    /// Labeled loops decided one-shot.
    pub loop_as_one_shot: usize,
    /// Labeled one-shots decided loop.
    pub one_shot_as_loop: usize,
    /// `100 * correct / total`.
    pub accuracy_pct: f64,
}

/// BPM accuracy over labeled loops.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BpmMetrics {
    /// Files with a BPM label.
    pub total: usize,
    /// Within ±[`BPM_TOL`].
    pub correct: usize,
    /// Metrical-level errors (not counted in `correct`).
    pub octave_errors: usize,
    /// No estimate (counted wrong).
    pub missing: usize,
    /// `100 * correct / total`.
    pub accuracy_pct: f64,
    /// `100 * octave_errors / total`.
    pub octave_rate_pct: f64,
    /// One-shot-labeled files that were given a BPM (must be 0).
    pub one_shots_with_bpm: usize,
    /// Files labeled one-shot (the denominator of the line above).
    pub one_shots_checked: usize,
}

/// Key accuracy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyMetrics {
    /// Files with a key label.
    pub total: usize,
    /// Exact tonic + mode.
    pub exact: usize,
    /// Files that got any key.
    pub detected: usize,
    /// `100 * exact / total`.
    pub exact_pct: f64,
    /// `100 *` mean of [`mirex_key_score`] (no key = 0).
    pub mirex_pct: f64,
}

/// Note accuracy (tonal one-shots).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteMetrics {
    /// Files with a note label.
    pub total: usize,
    /// Pitch class right (octave ignored).
    pub pitch_class_correct: usize,
    /// MIDI number right.
    pub midi_correct: usize,
    /// `100 * pitch_class_correct / total`.
    pub pitch_class_pct: f64,
    /// `100 * midi_correct / total`.
    pub midi_pct: f64,
}

/// All metrics of a tier; `None` when no file carried that label.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// Loop/one-shot.
    pub kind: Option<KindMetrics>,
    /// BPM.
    pub bpm: Option<BpmMetrics>,
    /// Key.
    pub key: Option<KeyMetrics>,
    /// Note.
    pub note: Option<NoteMetrics>,
    /// Similarity (P@10, MRR), see [`super::similarity`]; set by the runner,
    /// not by the [`Accumulator`].
    #[serde(default)]
    pub similar: Option<SimilarMetrics>,
    /// Category (top-1, macro-F1, confusion, calibration), see
    /// [`super::category`].
    #[serde(default)]
    pub category: Option<CategoryMetrics>,
}

/// Scores observations one by one.
#[derive(Debug, Default)]
pub struct Accumulator {
    kind: KindMetrics,
    bpm: BpmMetrics,
    key: KeyMetrics,
    mirex_sum: f64,
    note: NoteMetrics,
    category: CategoryAccumulator,
}

impl Accumulator {
    /// Adds one analysed file (`id`: its dataset id) with its labels.
    pub fn add(&mut self, id: &str, labels: &Labels, got: &Observation) {
        if let (Some(want), Some(c)) = (labels.category, got.category) {
            self.category.add(id, want, c);
        }
        if let Some(want) = labels.kind {
            self.kind.total += 1;
            if want == got.kind {
                self.kind.correct += 1;
            } else if want == Kind::Loop {
                self.kind.loop_as_one_shot += 1;
            } else {
                self.kind.one_shot_as_loop += 1;
            }
            if want == Kind::OneShot {
                self.bpm.one_shots_checked += 1;
                self.bpm.one_shots_with_bpm += usize::from(got.bpm.is_some());
            }
        }
        if let Some(want) = labels.bpm {
            self.bpm.total += 1;
            self.bpm.missing += usize::from(got.bpm.is_none());
            match bpm_outcome(want, got.bpm) {
                BpmOutcome::Correct => self.bpm.correct += 1,
                BpmOutcome::Octave => self.bpm.octave_errors += 1,
                BpmOutcome::Wrong => {}
            }
        }
        if let Some(want) = labels.key {
            self.key.total += 1;
            self.key.detected += usize::from(got.key.is_some());
            if let Some(k) = got.key {
                self.key.exact += usize::from(key_exact(want, k));
                self.mirex_sum += mirex_key_score(want, k);
            }
        }
        if let Some(want) = labels.note_midi {
            self.note.total += 1;
            if let Some(m) = got.note_midi {
                self.note.midi_correct += usize::from(m == want);
                self.note.pitch_class_correct += usize::from(m % 12 == want % 12);
            }
        }
    }

    /// Finishes the derived percentages.
    pub fn finish(self) -> Metrics {
        let Self {
            mut kind,
            mut bpm,
            mut key,
            mirex_sum,
            mut note,
            category,
        } = self;
        let mut m = Metrics {
            category: category.finish(T_FUSED, T_DSP),
            ..Metrics::default()
        };
        if let Some(p) = pct(kind.correct, kind.total) {
            kind.accuracy_pct = p;
            m.kind = Some(kind);
        }
        if let Some(p) = pct(bpm.correct, bpm.total) {
            bpm.accuracy_pct = p;
            bpm.octave_rate_pct = pct(bpm.octave_errors, bpm.total).unwrap_or(0.0);
            m.bpm = Some(bpm);
        } else if bpm.one_shots_checked > 0 {
            // No BPM labels, but the one-shot guard still applies.
            m.bpm = Some(bpm);
        }
        if let Some(p) = pct(key.exact, key.total) {
            key.exact_pct = p;
            key.mirex_pct = 100.0 * mirex_sum / key.total as f64;
            m.key = Some(key);
        }
        if let Some(p) = pct(note.midi_correct, note.total) {
            note.midi_pct = p;
            note.pitch_class_pct = pct(note.pitch_class_correct, note.total).unwrap_or(0.0);
            m.note = Some(note);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(root: u8, mode: Option<Mode>) -> Key {
        Key { root, mode }
    }
    const MAJ: Option<Mode> = Some(Mode::Major);
    const MIN: Option<Mode> = Some(Mode::Minor);

    #[test]
    fn mirex_weights_are_exact() {
        // Truth C major (0).
        let t = k(0, MAJ);
        let cases = [
            (k(0, MAJ), 1.0),
            (k(7, MAJ), 0.5), // G major: fifth above
            (k(5, MAJ), 0.5), // F major: fifth below
            (k(9, MIN), 0.3), // A minor: relative
            (k(0, MIN), 0.2), // C minor: parallel
            (k(3, MIN), 0.0), // C minor's relative major is not C's
            (k(2, MAJ), 0.0),
            (k(7, MIN), 0.0), // fifth with wrong mode
        ];
        for (got, want) in cases {
            assert_eq!(mirex_key_score(t, got), want, "{got:?}");
        }
        // Truth A minor (9): relative is C major, fifths E / D minor.
        let t = k(9, MIN);
        assert_eq!(mirex_key_score(t, k(0, MAJ)), 0.3);
        assert_eq!(mirex_key_score(t, k(4, MIN)), 0.5);
        assert_eq!(mirex_key_score(t, k(2, MIN)), 0.5);
        assert_eq!(mirex_key_score(t, k(9, MAJ)), 0.2);
        assert_eq!(mirex_key_score(t, k(3, MAJ)), 0.0);
    }

    #[test]
    fn keys_compare_as_pitch_classes_and_modes() {
        // Eb minor spelled as D#m is the same pitch class (3).
        assert!(key_exact(k(3, MIN), k(3 + 12, MIN)));
        assert!(!key_exact(k(3, MIN), k(3, MAJ)));
        assert!(!key_exact(k(3, MIN), k(3, None)), "mode-less is wrong");
        assert!(key_exact(k(3, None), k(3, MIN)), "mode-less truth: tonic");
        assert_eq!(mirex_key_score(k(3, None), k(4, MIN)), 0.0);
        assert_eq!(mirex_key_score(k(3, None), k(3, None)), 1.0);
        assert_eq!(mirex_key_score(k(3, MIN), k(3, None)), 0.0);
    }

    #[test]
    fn bpm_outcomes() {
        use BpmOutcome::*;
        let cases = [
            (120.0, Some(120.9), Correct),
            (120.0, Some(121.0), Correct),
            (120.0, Some(121.1), Wrong),
            (120.0, Some(60.5), Octave),
            (120.0, Some(240.0), Octave),
            (120.0, Some(360.0), Octave),
            (120.0, Some(80.0), Octave),
            (120.0, Some(180.0), Octave),
            (120.0, Some(100.0), Wrong),
            (120.0, None, Wrong),
        ];
        for (truth, got, want) in cases {
            assert_eq!(bpm_outcome(truth, got), want, "{truth} {got:?}");
        }
    }

    fn obs(kind: Kind, bpm: Option<f64>, key: Option<Key>, note: Option<u8>) -> Observation {
        Observation {
            kind,
            bpm,
            key,
            note_midi: note,
            category: None,
        }
    }

    /// Hand-computed: 4 kind labels (3 right), 3 BPM labels (1 right, 1
    /// octave, 1 missing), 2 keys (exact, relative), 2 notes (exact, one
    /// octave off), one one-shot given a BPM.
    #[test]
    fn accumulator_matches_hand_computation() {
        let mut a = Accumulator::default();
        let loop_label = |bpm| Labels {
            kind: Some(Kind::Loop),
            bpm: Some(bpm),
            ..Labels::default()
        };
        a.add(
            "f",
            &loop_label(100.0),
            &obs(Kind::Loop, Some(100.4), None, None),
        );
        a.add(
            "f",
            &loop_label(100.0),
            &obs(Kind::Loop, Some(200.0), None, None),
        );
        a.add(
            "f",
            &loop_label(100.0),
            &obs(Kind::OneShot, None, None, None),
        );
        let shot = Labels {
            kind: Some(Kind::OneShot),
            ..Labels::default()
        };
        a.add("f", &shot, &obs(Kind::Loop, Some(90.0), None, None));
        let keyed = |key| Labels {
            key: Some(key),
            ..Labels::default()
        };
        a.add(
            "f",
            &keyed(k(0, MAJ)),
            &obs(Kind::Loop, None, Some(k(0, MAJ)), None),
        );
        a.add(
            "f",
            &keyed(k(0, MAJ)),
            &obs(Kind::Loop, None, Some(k(9, MIN)), None),
        );
        let noted = |m| Labels {
            note_midi: Some(m),
            ..Labels::default()
        };
        a.add("f", &noted(60), &obs(Kind::OneShot, None, None, Some(60)));
        a.add("f", &noted(60), &obs(Kind::OneShot, None, None, Some(72)));
        let m = a.finish();
        let kind = m.kind.unwrap();
        assert_eq!((kind.total, kind.correct), (4, 2));
        assert_eq!((kind.loop_as_one_shot, kind.one_shot_as_loop), (1, 1));
        assert_eq!(kind.accuracy_pct, 50.0);
        let bpm = m.bpm.unwrap();
        assert_eq!((bpm.total, bpm.correct, bpm.octave_errors), (3, 1, 1));
        assert_eq!(bpm.missing, 1);
        assert!((bpm.accuracy_pct - 100.0 / 3.0).abs() < 1e-9);
        assert_eq!((bpm.one_shots_checked, bpm.one_shots_with_bpm), (1, 1));
        let key = m.key.unwrap();
        assert_eq!((key.total, key.exact), (2, 1));
        assert_eq!(key.exact_pct, 50.0);
        assert!((key.mirex_pct - 65.0).abs() < 1e-9, "(1 + 0.3) / 2");
        let note = m.note.unwrap();
        assert_eq!((note.pitch_class_pct, note.midi_pct), (100.0, 50.0));
    }

    #[test]
    fn missing_labels_give_no_metric() {
        let m = Accumulator::default().finish();
        assert_eq!(m, Metrics::default());
    }
}

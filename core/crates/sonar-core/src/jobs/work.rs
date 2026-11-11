//! Per-file work, run on rayon workers: decode + measure for one stage
//! (`dsp` also classifies loop/one-shot, estimates the tempo of loops,
//! ADR-021, and the key and one-shot note, ADR-022).
//!
//! Workers never touch the database; they return a [`FileResult`] for the
//! coordinator to commit. Each worker thread keeps one [`Decoder`], one
//! DSP [`Analyzer`], one [`NoteTracker`] and one [`AspectExtractor`] (FFT
//! plans and scratch buffers are reused across files). The `dsp` stage also
//! returns the file's raw aspect vectors, which the coordinator appends to
//! the vector store (ADR-023).

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use super::progress::Shared;
use super::queue::Claim;
use super::stage::{DspAnalysis, FileResult, Stage};
use super::BeforeFileHook;
use crate::classify::{classify_kind, kind_hint};
use crate::decode::{DecodeOptions, Decoded, Decoder};
use crate::dsp::aspects::{AspectExtractor, AspectVectors};
use crate::dsp::Analyzer;
use crate::key::{key_and_note, NoteTracker};
use crate::meta::MetaOptions;
use crate::store::analysis::KindTempo;
use crate::store::key_pitch::KeyPitch;
use crate::tempo::loop_tempo;
use crate::text::filename_prior;
use crate::{quiet_panic, Error};

/// Worker-side knobs.
#[derive(Clone, Default)]
pub(crate) struct WorkOptions {
    pub delay: Duration,
    pub before_file: Option<BeforeFileHook>,
}

thread_local! {
    static DECODER: RefCell<Decoder> = RefCell::new(Decoder::new());
    /// Built on first use (at the decoded rate).
    static ANALYZER: RefCell<Option<Analyzer>> = const { RefCell::new(None) };
    /// Built on first use (at the decoded rate).
    static NOTE: RefCell<Option<NoteTracker>> = const { RefCell::new(None) };
    /// Built on first use (at the decoded rate).
    static ASPECTS: RefCell<Option<AspectExtractor>> = const { RefCell::new(None) };
}

/// Processes one claimed file for `stage`. Never panics: a panic (also from
/// the test hook) becomes `Failed("internal error: ...")`.
pub(crate) fn process(
    stage: Stage,
    claim: &Claim,
    shared: &Shared,
    opts: &WorkOptions,
) -> FileResult {
    // Pause and cancel stop new files from starting; the coordinator commits
    // what finished and re-claims the `Unstarted` files after a resume.
    if !shared.should_run() {
        return FileResult::Unstarted;
    }
    let run = quiet_panic::guarded(|| {
        if let Some(hook) = &opts.before_file {
            hook(stage, &claim.path);
        }
        if !opts.delay.is_zero() {
            std::thread::sleep(opts.delay);
        }
        let cancel = shared.cancel_flag();
        with_workers(|w| run_stage(w, stage, &claim.path, cancel))
    });
    run.unwrap_or_else(|msg| {
        reset_workers();
        FileResult::Failed(format!("internal error: {msg}"))
    })
}

/// Drops this thread's decoder/analyzer/note state after a caught panic
/// (it may be mid-update), so the next file starts clean.
pub(crate) fn reset_workers() {
    DECODER.with(|d| *d.borrow_mut() = Decoder::new());
    ANALYZER.with(|a| *a.borrow_mut() = None);
    NOTE.with(|n| *n.borrow_mut() = None);
    ASPECTS.with(|x| *x.borrow_mut() = None);
}

/// Runs `f` with this thread's reusable decoder, analyzer and note tracker
/// (also used by `sonar eval`, which must measure the same code path).
pub(crate) fn with_workers<R>(f: impl FnOnce(&mut Workers<'_>) -> R) -> R {
    DECODER.with(|d| {
        ANALYZER.with(|a| {
            NOTE.with(|n| {
                ASPECTS.with(|x| {
                    let mut workers = Workers {
                        decoder: &mut d.borrow_mut(),
                        analyzer: &mut a.borrow_mut(),
                        note: &mut n.borrow_mut(),
                        aspects: &mut x.borrow_mut(),
                    };
                    f(&mut workers)
                })
            })
        })
    })
}

/// One worker thread's reusable state.
pub(crate) struct Workers<'a> {
    pub decoder: &'a mut Decoder,
    /// `None` until the first `dsp` file.
    pub analyzer: &'a mut Option<Analyzer>,
    /// `None` until the first `dsp` file.
    pub note: &'a mut Option<NoteTracker>,
    /// `None` until the first `dsp` file.
    pub aspects: &'a mut Option<AspectExtractor>,
}

/// Decode options per stage (ADR-013): `meta` probes with zero windows (no
/// analysis buffer, no meter; header length for PCM WAV/AIFF, ADR-015);
/// `dsp` decodes the 30 s analysis window, measures peak/loudness over the
/// default meta window, and classifies + estimates tempo, key and note on
/// the window.
fn run_stage(
    w: &mut Workers<'_>,
    stage: Stage,
    path: &Path,
    cancel: Arc<AtomicBool>,
) -> FileResult {
    let (decode, meta) = match stage {
        Stage::Meta => (
            DecodeOptions {
                max_analysis_secs: Some(0.0),
                cancel: Some(cancel),
                ..DecodeOptions::default()
            },
            MetaOptions { max_meta_secs: 0.0 },
        ),
        Stage::Dsp => (
            DecodeOptions {
                cancel: Some(cancel),
                ..DecodeOptions::default()
            },
            MetaOptions::default(),
        ),
        // The runner never schedules it; fail loudly rather than skip silently.
        Stage::Embed => return FileResult::Failed("embed stage is not implemented".into()),
    };
    match w.decoder.decode_with_meta(path, &decode, &meta) {
        Ok((decoded, meta)) => {
            let analysis = match stage {
                Stage::Dsp => match dsp_analysis(w, &decoded, path) {
                    Ok(a) => Some(Box::new(a)),
                    Err(e) => return FileResult::Failed(e.to_string()),
                },
                _ => None,
            };
            FileResult::Done { meta, analysis }
        }
        // Offline drive, permissions, vanished file: may go away, so the
        // file stays pending (ADR-013).
        Err(Error::Io { path, source }) => {
            tracing::debug!(path = %path.display(), error = %source, "i/o error, file left pending");
            FileResult::Skipped
        }
        Err(Error::Cancelled) => FileResult::Skipped,
        Err(e) => FileResult::Failed(e.to_string()),
    }
}

/// DSP features of the analysis window, loop/one-shot decision, (loops
/// only) tempo, key and (one-shots only) note, with the file and folder
/// names as lower-weight priors (ADR-020, ADR-021, ADR-022), plus the raw
/// aspect vectors (ADR-019, ADR-023).
fn dsp_analysis(w: &mut Workers<'_>, decoded: &Decoded, path: &Path) -> crate::Result<DspAnalysis> {
    let a = analyze_window(w, decoded, kind_hint(path), filename_prior(path), true)?;
    Ok(DspAnalysis {
        kind_tempo: KindTempo::new(&a.kind, a.tempo.as_ref()),
        key_pitch: KeyPitch::new(&a.key_note),
        aspects: a.aspects.ok_or_else(|| Error::Internal {
            reason: "aspect vectors missing".into(),
        })?,
    })
}

/// Everything the `dsp` stage derives from a decoded window.
pub(crate) struct WindowAnalysis {
    pub kind: crate::classify::KindResult,
    pub tempo: Option<crate::tempo::Tempo>,
    pub key_note: crate::key::KeyNote,
    /// Raw aspect vectors (only when asked for).
    pub aspects: Option<AspectVectors>,
}

/// Loop/one-shot, tempo, key and note of `decoded` with the given name
/// hint and priors (`None`/default = names hidden, as `sonar eval` runs it),
/// and the raw aspect vectors if `with_aspects`.
pub(crate) fn analyze_window(
    w: &mut Workers<'_>,
    decoded: &Decoded,
    hint: Option<crate::classify::Kind>,
    prior: crate::text::FilenamePrior,
    with_aspects: bool,
) -> crate::Result<WindowAnalysis> {
    let sr = decoded.sample_rate;
    let slot = &mut *w.analyzer;
    let analyzer = match slot {
        Some(a) if a.sample_rate() == sr => a,
        _ => slot.insert(Analyzer::new(sr)?),
    };
    let slot = &mut *w.note;
    let tracker = match slot {
        Some(t) if t.sample_rate() == sr => t,
        _ => slot.insert(NoteTracker::new(sr)?),
    };
    let features = analyzer.analyze(&decoded.samples)?;
    let duration = decoded.source.duration_s;
    let kind = classify_kind(&features, duration, hint);
    let tempo = loop_tempo(&kind, &features, duration, prior.bpm);
    let key_note = key_and_note(&kind, &features, &decoded.samples, tracker, prior.key)?;
    let aspects = if with_aspects {
        let slot = &mut *w.aspects;
        let extractor = match slot {
            Some(x) if x.sample_rate() == sr => x,
            _ => slot.insert(AspectExtractor::new(sr)?),
        };
        Some(extractor.extract(&decoded.samples, &features)?)
    } else {
        None
    };
    Ok(WindowAnalysis {
        kind,
        tempo,
        key_note,
        aspects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::manifest::Expect;
    use crate::fixtures::{generate, FixtureOptions};

    /// The meta stage (header length for intact PCM WAV/AIFF, ADR-015;
    /// counted otherwise) on every fixture: intact files are `Done` with
    /// the manifest's format and exact length, corrupt ones follow their
    /// `expect` (`docs/EVAL.md`).
    #[test]
    fn meta_stage_outcomes_follow_the_fixture_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = FixtureOptions {
            include_long: false,
            ..FixtureOptions::default()
        };
        let manifest = generate(tmp.path(), &opts).unwrap();
        let mut dec = Decoder::new();
        let (mut analyzer, mut note, mut aspects) = (None, None, None);
        let mut w = Workers {
            decoder: &mut dec,
            analyzer: &mut analyzer,
            note: &mut note,
            aspects: &mut aspects,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (mut ok, mut corrupt) = (0, 0);
        for f in &manifest.fixtures {
            let path = tmp.path().join(&f.path);
            let result = match (&f.format, &f.truth.corrupt) {
                (None, None) => continue,
                _ => run_stage(&mut w, Stage::Meta, &path, cancel.clone()),
            };
            match (&f.format, &f.truth.corrupt, result) {
                (Some(fmt), None, FileResult::Done { meta, analysis }) => {
                    assert!(!meta.partial, "{}: {:?}", f.path, meta.problem);
                    assert_eq!(analysis, None, "{}", f.path);
                    assert_eq!(meta.num_frames, fmt.num_frames, "{}", f.path);
                    assert_eq!(meta.sample_rate, fmt.sample_rate, "{}", f.path);
                    assert_eq!(meta.channels, fmt.channels, "{}", f.path);
                    assert_eq!(meta.bit_depth, fmt.bit_depth.map(u32::from), "{}", f.path);
                    assert_eq!((meta.peak_dbfs, meta.measured_s), (None, 0.0), "{}", f.path);
                    ok += 1;
                }
                (_, Some(_), FileResult::Failed(_)) => corrupt += 1,
                (_, Some(c), FileResult::Done { .. }) if c.expect == Expect::ErrorOrPartial => {
                    corrupt += 1;
                }
                (_, c, r) => panic!("{}: expected {c:?}, got {r:?}", f.path),
            }
        }
        assert!(ok >= 100 && corrupt >= 15, "{ok} ok, {corrupt} corrupt");
    }

    /// The dsp stage stores kind + tempo + key + note: fixture loops get
    /// their BPM (names are neutral, so these are audio estimates),
    /// one-shots none; chord loops their key, tonal one-shots their note
    /// (and its pitch class as key), noise and silence neither.
    #[test]
    fn dsp_stage_classifies_and_gates_tempo_key_and_note() {
        use crate::classify::Kind;
        use crate::fixtures::manifest::Kind as TruthKind;
        let tmp = tempfile::tempdir().unwrap();
        let opts = FixtureOptions {
            include_long: false,
            ..FixtureOptions::default()
        };
        let manifest = generate(tmp.path(), &opts).unwrap();
        let mut dec = Decoder::new();
        let (mut analyzer, mut note, mut aspects) = (None, None, None);
        let mut w = Workers {
            decoder: &mut dec,
            analyzer: &mut analyzer,
            note: &mut note,
            aspects: &mut aspects,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (mut loops, mut one_shots, mut keys, mut notes) = (0, 0, 0, 0);
        for f in &manifest.fixtures {
            let (Some(truth), None) = (f.truth.kind, &f.truth.corrupt) else {
                continue;
            };
            let path = tmp.path().join(&f.path);
            let FileResult::Done { analysis, .. } =
                run_stage(&mut w, Stage::Dsp, &path, cancel.clone())
            else {
                panic!("{}: not done", f.path);
            };
            let analysis = analysis.unwrap_or_else(|| panic!("{}: no analysis", f.path));
            let dims = analysis
                .aspects
                .parts()
                .map(|(asp, v)| (v.len(), asp.dim()));
            assert!(dims.iter().all(|(n, d)| n == d), "{}: {dims:?}", f.path);
            let (a, kp) = (analysis.kind_tempo, analysis.key_pitch);
            if let Some(want) = &f.truth.key {
                let got = kp.key.unwrap_or_else(|| panic!("{}: no key", f.path));
                assert_eq!(got.root, want.tonic_pc, "{}", f.path);
                keys += 1;
            }
            match (truth, f.truth.note_midi) {
                (TruthKind::Tone | TruthKind::OneShot, Some(m)) if f.path.starts_with("tonal/") => {
                    assert_eq!(kp.pitch_midi, Some(m), "{}", f.path);
                    assert_eq!(kp.key.map(|k| k.root), Some(m % 12), "{}", f.path);
                    notes += 1;
                }
                (TruthKind::Noise | TruthKind::Silence, _) => {
                    assert_eq!(kp, KeyPitch::default(), "{}", f.path);
                }
                _ => {}
            }
            if a.kind == Some(Kind::OneShot) || a.kind.is_none() || truth == TruthKind::OneShot {
                assert_eq!(
                    (a.bpm, a.bpm_conf, a.bpm_source),
                    (None, None, None),
                    "{}",
                    f.path
                );
            }
            match (truth, f.truth.bpm) {
                // 140 is the half-time pattern (ambiguous octave, ADR-021).
                (TruthKind::Loop, Some(bpm)) if f.path.starts_with("loops/") && bpm < 140.0 => {
                    assert_eq!(a.kind, Some(Kind::Loop), "{}", f.path);
                    let got = f64::from(a.bpm.unwrap_or(0.0));
                    assert!((got - bpm).abs() <= 1.0, "{}: {got} vs {bpm}", f.path);
                    loops += 1;
                }
                (TruthKind::OneShot, _) => one_shots += 1,
                _ => {}
            }
        }
        assert!(
            loops >= 8 && one_shots >= 10 && keys >= 16 && notes >= 20,
            "{loops} loops, {one_shots} one-shots, {keys} keys, {notes} notes"
        );
    }
}

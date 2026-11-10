//! Per-file work, run on rayon workers: decode + measure for one stage.
//!
//! Workers never touch the database; they return a [`FileResult`] for the
//! coordinator to commit. Each worker thread keeps one [`Decoder`] (FFT
//! plans and scratch buffers are reused across files).

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use super::progress::Shared;
use super::queue::Claim;
use super::stage::{FileResult, Stage};
use super::BeforeFileHook;
use crate::decode::{DecodeOptions, Decoder};
use crate::meta::MetaOptions;
use crate::{quiet_panic, Error};

/// Worker-side knobs.
#[derive(Clone, Default)]
pub(crate) struct WorkOptions {
    pub delay: Duration,
    pub before_file: Option<BeforeFileHook>,
}

thread_local! {
    static DECODER: RefCell<Decoder> = RefCell::new(Decoder::new());
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
        DECODER.with(|d| run_stage(&mut d.borrow_mut(), stage, &claim.path, cancel))
    });
    run.unwrap_or_else(|msg| {
        // The thread's decoder may be mid-state: start over.
        DECODER.with(|d| *d.borrow_mut() = Decoder::new());
        FileResult::Failed(format!("internal error: {msg}"))
    })
}

/// Decode options per stage (ADR-013): `meta` probes with zero windows (no
/// analysis buffer, no meter; header length for PCM WAV/AIFF, ADR-015);
/// `dsp` decodes the 30 s analysis window and measures peak/loudness over
/// the default meta window.
fn run_stage(
    decoder: &mut Decoder,
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
    // T3.x: the decoded analysis window (`_decoded.samples`) feeds the DSP
    // features here; for now it is only decoded to validate the file.
    match decoder.decode_with_meta(path, &decode, &meta) {
        Ok((_decoded, meta)) => FileResult::Done { meta },
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
        let cancel = Arc::new(AtomicBool::new(false));
        let (mut ok, mut corrupt) = (0, 0);
        for f in &manifest.fixtures {
            let path = tmp.path().join(&f.path);
            let result = match (&f.format, &f.truth.corrupt) {
                (None, None) => continue,
                _ => run_stage(&mut dec, Stage::Meta, &path, cancel.clone()),
            };
            match (&f.format, &f.truth.corrupt, result) {
                (Some(fmt), None, FileResult::Done { meta }) => {
                    assert!(!meta.partial, "{}: {:?}", f.path, meta.problem);
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
}

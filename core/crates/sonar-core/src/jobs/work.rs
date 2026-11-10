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
    // Pause blocks here (Condvar); cancel stops new files from starting.
    if !shared.wait_runnable() {
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
/// analysis buffer, no meter); `dsp` decodes the 30 s analysis window and
/// measures peak/loudness over the default meta window.
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

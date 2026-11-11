//! External similarity seeds: a file outside the library, decoded and
//! analyzed on the fly exactly like the `dsp` stage does (30 s analysis
//! window at [`crate::decode::ANALYSIS_RATE`], same analyzer and aspect
//! extractor), so its raw vectors are comparable with the stored rows.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::decode::DecodeOptions;
use crate::dsp::aspects::AspectVectors;
use crate::jobs::work::{reset_workers, window_aspects, with_workers};
use crate::{quiet_panic, Error, Result};

/// Raw aspect vectors of the audio file at `path`.
///
/// Uses this thread's reusable decoder / analyzer (the same thread-locals
/// the job workers use). Never panics: a panic inside decoding or analysis
/// becomes [`Error::Internal`] and the thread's analysis state is reset.
///
/// # Errors
/// [`Error::Io`], [`Error::Decode`], [`Error::UnsupportedFormat`] for
/// unreadable or corrupt files; [`Error::Cancelled`] if `cancel` is set
/// while decoding.
pub fn analyze_file(path: &Path, cancel: Option<Arc<AtomicBool>>) -> Result<AspectVectors> {
    let opts = DecodeOptions {
        cancel,
        ..DecodeOptions::default()
    };
    let run = quiet_panic::guarded(|| {
        with_workers(|w| {
            let decoded = w.decoder.decode(path, &opts)?;
            window_aspects(w, &decoded)
        })
    });
    run.unwrap_or_else(|msg| {
        reset_workers();
        Err(Error::Internal {
            reason: format!("analysis of {} failed: {msg}", path.display()),
        })
    })
}

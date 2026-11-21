//! Region seeds (ADR-040): a time range of an audio file, decoded and
//! analyzed on the fly exactly like a whole external file (ADR-024), so
//! "find similar to this selection" compares the selected sound alone with
//! the stored whole-file rows.
//!
//! The decoder reads from the start of the file, so the file is decoded up
//! to the region's end (at most [`DEFAULT_MAX_ANALYSIS_SECS`] of region,
//! the same analysis window as the `dsp` stage) and the range is cut out of
//! the analysis-rate samples. Frames past the window are only counted, so
//! the true duration is known for the range check.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::Serialize;

use super::widen::{region_samples, widen_samples, Widen, DEFAULT_WIDEN, WIDEN_TO_SECS};
use crate::decode::{DecodeOptions, ANALYSIS_RATE, DEFAULT_MAX_ANALYSIS_SECS};
use crate::dsp::aspects::AspectVectors;
use crate::jobs::work::{reset_workers, window_aspects, with_workers};
use crate::{quiet_panic, Error, Result};

/// Shortest region that can be a seed (s): one sample at
/// [`ANALYSIS_RATE`]. Regions shorter than [`WIDEN_TO_SECS`] are widened
/// for analysis ([`super::widen`]).
pub const MIN_REGION_SECS: f64 = 1.0 / ANALYSIS_RATE as f64;
/// A region may end this far (s) past the file's end (UI rounding of the
/// waveform edge); its end is then clamped to the duration.
pub const END_TOLERANCE_SECS: f64 = 0.01;

/// A time range of a file, in seconds from its start.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TimeRange {
    /// Start (s, ≥ 0).
    pub start_s: f64,
    /// End (s, > start; at most the file's duration + [`END_TOLERANCE_SECS`]).
    pub end_s: f64,
}

impl TimeRange {
    /// A range from `start_s` to `end_s`.
    pub fn new(start_s: f64, end_s: f64) -> Self {
        Self { start_s, end_s }
    }

    /// Checks what can be checked without the file: finite, start ≥ 0,
    /// end > start, length ≥ [`MIN_REGION_SECS`] (one analysis sample).
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] naming the problem.
    pub fn validate(&self) -> Result<()> {
        let (s, e) = (self.start_s, self.end_s);
        let reason = if !s.is_finite() || !e.is_finite() {
            "region start and end must be finite numbers".to_owned()
        } else if s < 0.0 {
            format!("region start {s} s is negative")
        } else if e <= s {
            format!("region end {e} s is not after its start {s} s")
        } else if e - s < MIN_REGION_SECS * 0.5 {
            // Half a sample of slack: a one-sample region must not fail on
            // float rounding.
            format!(
                "region of {:.3} ms is shorter than one sample ({:.3} ms)",
                (e - s) * 1e3,
                MIN_REGION_SECS * 1e3
            )
        } else {
            return Ok(());
        };
        Err(Error::InvalidQuery { reason })
    }

    /// The range checked against a file of `duration_s`: an end within
    /// [`END_TOLERANCE_SECS`] past the end is clamped to it.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] if the range is invalid, ends past the file
    /// or is empty (shorter than [`MIN_REGION_SECS`]) after clamping.
    pub fn within(&self, duration_s: f64) -> Result<Self> {
        self.validate()?;
        if self.end_s > duration_s + END_TOLERANCE_SECS {
            return Err(Error::InvalidQuery {
                reason: format!(
                    "region {:.3}–{:.3} s ends past the end of the file ({duration_s:.3} s)",
                    self.start_s, self.end_s
                ),
            });
        }
        let clamped = Self::new(self.start_s, self.end_s.min(duration_s));
        clamped.validate()?;
        Ok(clamped)
    }
}

/// Raw aspect vectors of `range` of the audio file at `path`, computed as
/// [`super::seed::analyze_file`] computes them for a whole file (same
/// thread-local decoder, analyzer and extractor). A region longer than the
/// analysis window is analyzed over its first
/// [`DEFAULT_MAX_ANALYSIS_SECS`]; one shorter than [`WIDEN_TO_SECS`] is
/// widened by [`DEFAULT_WIDEN`] for analysis. Never panics (a panic becomes
/// [`Error::Internal`]).
///
/// # Errors
/// [`Error::InvalidQuery`] for an invalid or out-of-range region (see
/// [`TimeRange::within`]); decode errors as for a whole external file;
/// [`Error::Cancelled`] if `cancel` is set while decoding.
pub fn analyze_region(
    path: &Path,
    range: TimeRange,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<AspectVectors> {
    analyze_region_with(path, range, cancel, DEFAULT_WIDEN)
}

/// [`analyze_region`] with an explicit widening strategy (tests).
pub(crate) fn analyze_region_with(
    path: &Path,
    range: TimeRange,
    cancel: Option<Arc<AtomicBool>>,
    strategy: Widen,
) -> Result<AspectVectors> {
    range.validate()?;
    let window_end = range.end_s.min(range.start_s + DEFAULT_MAX_ANALYSIS_SECS);
    // A short region may take up to `W` of context after its end (or from
    // the file start); decoding a little more is cheap.
    // One sample of slack so a region that rounds below `W` samples is
    // always decoded with its context.
    let decode_end = if range.end_s - range.start_s < WIDEN_TO_SECS + MIN_REGION_SECS {
        window_end + 2.0 * WIDEN_TO_SECS
    } else {
        window_end
    };
    let opts = DecodeOptions {
        max_analysis_secs: Some(decode_end),
        cancel,
        ..DecodeOptions::default()
    };
    let run = quiet_panic::guarded(|| {
        with_workers(|w| {
            let mut decoded = w.decoder.decode(path, &opts)?;
            let range = range.within(decoded.source.duration_s)?;
            let sr = f64::from(decoded.sample_rate);
            let n = decoded.samples.len();
            let first = sample_index(range.start_s, sr, n);
            let mut last = sample_index(range.end_s.min(window_end), sr, n);
            // A region under one sample after rounding keeps one sample.
            if last == first && first < n {
                last = first + 1;
            }
            // A partial (damaged) file can decode fewer samples than its
            // counted length.
            if last <= first {
                return Err(Error::InvalidQuery {
                    reason: format!(
                        "region {:.3}–{:.3} s has no readable audio in {}",
                        range.start_s,
                        range.end_s,
                        path.display()
                    ),
                });
            }
            let w_len = widen_samples(decoded.sample_rate);
            decoded.samples = region_samples(&decoded.samples, first, last, w_len, strategy);
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

/// Index of the sample at `secs` (rounded), at most `len`.
fn sample_index(secs: f64, sample_rate: f64, len: usize) -> usize {
    // `secs` is finite and ≥ 0 here; the float→int cast saturates.
    ((secs * sample_rate).round() as usize).min(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(r: Result<TimeRange>) -> String {
        match r {
            Err(Error::InvalidQuery { reason }) => reason,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn validates_and_clamps_ranges() {
        assert!(TimeRange::new(0.0, 0.05).validate().is_ok());
        assert!(TimeRange::new(0.0, 0.001).validate().is_ok());
        assert!(TimeRange::new(1.0, 1.0001).validate().is_ok());
        for (s, e) in [
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
            (-0.1, 1.0),
            (1.0, 1.0),
            (1.0, 0.5),
            (0.0, 0.00001),
        ] {
            assert!(
                matches!(
                    TimeRange::new(s, e).validate(),
                    Err(Error::InvalidQuery { .. })
                ),
                "{s}..{e}"
            );
        }
        let r = TimeRange::new(0.5, 2.005)
            .within(2.0)
            .expect("within tolerance");
        assert_eq!(r, TimeRange::new(0.5, 2.0));
        assert!(invalid(TimeRange::new(0.5, 2.02).within(2.0)).contains("past the end"));
        // Clamping keeps a short region (it is widened for analysis)...
        assert_eq!(
            TimeRange::new(1.96, 2.009)
                .within(2.0)
                .expect("short region"),
            TimeRange::new(1.96, 2.0)
        );
        // ...but can make it empty.
        assert!(invalid(TimeRange::new(2.005, 2.009).within(2.0)).contains("not after"));
        assert_eq!(sample_index(1.0, 22_050.0, 100), 100);
        assert_eq!(sample_index(0.001, 22_050.0, 100), 22);
    }
}

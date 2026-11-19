//! Analysis window past a long leading silence (T10.7).
//!
//! Construction-kit stems and vocal takes are exported from bar 1 of the
//! song, so a part that enters at bar 17 starts with 30+ s of digital
//! silence: the default analysis window (the file's first
//! [`super::DEFAULT_MAX_ANALYSIS_SECS`]) then holds nothing to classify or
//! measure. For a file **longer than its window** whose window starts with
//! at least [`LEAD_MIN_S`] of silence, [`Decoder::skip_leading_silence`]
//! re-decodes so the window starts just before the first sound (at most
//! [`LEAD_MAX_S`] into the file). Shorter files keep their leading silence:
//! there the window is the whole file and its length is the loop length
//! (`crate::tempo`). Deterministic; the extra decode only happens for such
//! long files.

use std::path::Path;

use super::{DecodeOptions, Decoded, Decoder};
use crate::{Error, Result};

/// A sample is "sound" above this level (dBFS).
pub const LEAD_SILENCE_DBFS: f32 = -60.0;
/// Leading silence shorter than this (s) is kept.
pub const LEAD_MIN_S: f64 = 1.0;
/// The window never starts later than this (s) into the file.
pub const LEAD_MAX_S: f64 = 90.0;
/// The window starts this long (s) before the first sound, so the first
/// attack keeps its onset.
pub const PRE_ROLL_S: f64 = 0.05;

/// Index of the first sample whose magnitude exceeds
/// [`LEAD_SILENCE_DBFS`]; `None` when there is none (NaN counts as silence).
pub fn first_sound(samples: &[f32]) -> Option<usize> {
    let thr = 10f32.powf(LEAD_SILENCE_DBFS / 20.0);
    samples.iter().position(|x| x.abs() > thr)
}

impl Decoder {
    /// `decoded` (from [`Decoder::decode`] / [`Decoder::decode_with_meta`]
    /// of `path` with `opts`), or — when the file is longer than the
    /// analysis window and the window starts with at least [`LEAD_MIN_S`]
    /// of silence — the same window length starting [`PRE_ROLL_S`] before
    /// the first sound, at most [`LEAD_MAX_S`] in
    /// ([`Decoded::window_start_s`]). A file silent for the whole search
    /// span keeps its first window, and so does one whose re-decode fails
    /// with a decode error (the first decode already succeeded).
    ///
    /// # Errors
    /// [`Error::Cancelled`] and [`Error::Io`] from the re-decode are
    /// returned, so a cancel or an offline drive leaves the file pending
    /// (ADR-013) instead of storing the silent first window.
    pub fn skip_leading_silence(
        &mut self,
        path: &Path,
        opts: &DecodeOptions,
        decoded: Decoded,
    ) -> Result<Decoded> {
        match self.try_skip(path, opts, &decoded) {
            Ok(Some(wide)) => Ok(Decoded {
                source: decoded.source,
                partial: decoded.partial,
                problem: decoded.problem,
                ..wide
            }),
            Err(e @ (Error::Cancelled | Error::Io { .. })) => Err(e),
            Ok(None) | Err(_) => Ok(decoded),
        }
    }

    /// The moved window (with the re-decode's metadata), `None` when the
    /// window stays.
    fn try_skip(
        &mut self,
        path: &Path,
        opts: &DecodeOptions,
        decoded: &Decoded,
    ) -> Result<Option<Decoded>> {
        let Some(window_s) = opts.max_analysis_secs.filter(|s| *s > 0.0) else {
            return Ok(None);
        };
        let rate = f64::from(decoded.sample_rate);
        if !decoded.truncated_for_analysis || rate <= 0.0 {
            return Ok(None);
        }
        let lead = first_sound(&decoded.samples).unwrap_or(decoded.samples.len());
        if (lead as f64) < LEAD_MIN_S * rate {
            return Ok(None);
        }
        // Decode far enough to find the first sound and a full window after it.
        let search_s = (lead as f64 / rate).min(LEAD_MAX_S);
        let longer = DecodeOptions {
            max_analysis_secs: Some(if lead < decoded.samples.len() {
                search_s + window_s
            } else {
                LEAD_MAX_S + window_s
            }),
            ..opts.clone()
        };
        let mut wide = self.decode(path, &longer)?;
        let Some(first) = first_sound(&wide.samples) else {
            return Ok(None);
        };
        let max_start = (LEAD_MAX_S * rate) as usize;
        let start = first
            .saturating_sub((PRE_ROLL_S * rate) as usize)
            .min(max_start);
        let len = (window_s * rate).ceil() as usize;
        let end = (start + len).min(wide.samples.len());
        if start >= end {
            return Ok(None);
        }
        wide.samples.truncate(end);
        wide.samples.drain(..start);
        wide.window_start_s = start as f64 / rate;
        // Still a part of a longer file.
        wide.truncated_for_analysis = true;
        Ok(Some(wide))
    }
}

//! Widening of very short region seeds (T13.1, ADR-040 addendum).
//!
//! A selection shorter than [`WIDEN_TO_SECS`] is shorter than the STFT
//! half-window (`N_FFT / 2` = 1024 samples = 46.4 ms at 22.05 kHz with
//! centred framing, see [`crate::dsp::stft`]): its single analysis frame
//! is mostly the frame's own zero padding, so the frame-level statistics
//! the aspect vectors are built from say little about the selected sound.
//! For **analysis only** such a region can be widened to `W` samples:
//!
//! - [`Widen::Context`]: the file's own audio around the region, centred on
//!   it, shifted inward at the file start / end, the whole file if it is
//!   shorter than `W`;
//! - [`Widen::ZeroPad`]: the selection centred in `W` samples of silence;
//! - [`Widen::None`]: the selection as it is.
//!
//! Regions of at least `W` samples are never changed, so every region
//! accepted before T13.1 (≥ 50 ms) is analysed exactly as before.
//! [`DEFAULT_WIDEN`] was chosen with `sonar eval --short-regions`
//! ([`crate::eval::short_regions`]).

/// Regions shorter than this (s) are widened to it for analysis: 50 ms,
/// the shortest region accepted before T13.1 (≈ the 46.4 ms STFT
/// half-window rounded up), so longer regions keep their exact analysis.
pub const WIDEN_TO_SECS: f64 = 0.05;

/// How a region shorter than [`WIDEN_TO_SECS`] is turned into the samples
/// that are analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widen {
    /// Surrounding audio of the file, centred on the region.
    Context,
    /// The region centred in silence.
    ZeroPad,
    /// The region as it is.
    None,
}

impl Widen {
    /// Every strategy, in report order.
    pub const ALL: [Self; 3] = [Self::Context, Self::ZeroPad, Self::None];

    /// Lowercase name for reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::ZeroPad => "zeropad",
            Self::None => "none",
        }
    }
}

/// The strategy region seeds use (measured, ADR-040 T13.1 addendum).
pub const DEFAULT_WIDEN: Widen = Widen::Context;

/// `W` in samples at `sample_rate` (at least 1).
pub fn widen_samples(sample_rate: u32) -> usize {
    ((WIDEN_TO_SECS * f64::from(sample_rate)).round() as usize).max(1)
}

/// Samples to analyse for the region `first..last` of `samples` (the
/// file's analysis-rate signal from its start), widened to `w` samples by
/// `strategy` when shorter. `first < last <= samples.len()` is expected;
/// other inputs are clamped (never panics).
pub fn region_samples(
    samples: &[f32],
    first: usize,
    last: usize,
    w: usize,
    strategy: Widen,
) -> Vec<f32> {
    let len = samples.len();
    let last = last.min(len);
    let first = first.min(last);
    let n = last - first;
    if n >= w {
        return samples[first..last].to_vec();
    }
    match strategy {
        Widen::None => samples[first..last].to_vec(),
        Widen::Context => {
            let (lo, hi) = context_span(first, last, len, w);
            samples[lo..hi].to_vec()
        }
        Widen::ZeroPad => {
            let before = (w - n) / 2;
            let mut out = vec![0.0; w];
            out[before..before + n].copy_from_slice(&samples[first..last]);
            out
        }
    }
}

/// `lo..hi` of `w` samples centred on `first..last` (`last - first < w`),
/// shifted to lie inside `0..len`; `0..len` when `len <= w`.
pub fn context_span(first: usize, last: usize, len: usize, w: usize) -> (usize, usize) {
    if len <= w {
        return (0, len);
    }
    let extra = w.saturating_sub(last.saturating_sub(first));
    let lo = first.saturating_sub(extra / 2).min(len - w);
    (lo, lo + w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_span_centres_and_shifts_inward() {
        // Centred: 10 samples in a 100-sample window.
        assert_eq!(context_span(500, 510, 2000, 100), (455, 555));
        // At the start: extends only to the right.
        assert_eq!(context_span(0, 10, 2000, 100), (0, 100));
        assert_eq!(context_span(20, 30, 2000, 100), (0, 100));
        // At the end: extends only to the left.
        assert_eq!(context_span(1990, 2000, 2000, 100), (1900, 2000));
        // File shorter than W: the whole file.
        assert_eq!(context_span(10, 20, 60, 100), (0, 60));
    }

    #[test]
    fn region_samples_widen_only_short_regions() {
        let s: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let w = 100;
        // ≥ W: unchanged for every strategy.
        for st in Widen::ALL {
            assert_eq!(region_samples(&s, 200, 350, w, st), s[200..350].to_vec());
        }
        assert_eq!(region_samples(&s, 500, 510, w, Widen::None).len(), 10);
        let ctx = region_samples(&s, 500, 510, w, Widen::Context);
        assert_eq!((ctx.len(), ctx[0], ctx[99]), (100, 455.0, 554.0));
        let zp = region_samples(&s, 500, 510, w, Widen::ZeroPad);
        assert_eq!(zp.len(), 100);
        assert_eq!(&zp[45..55], &s[500..510]);
        assert!(zp[..45].iter().chain(&zp[55..]).all(|&x| x == 0.0));
        // Out-of-range input is clamped, not a panic.
        assert!(region_samples(&s, 2000, 3000, w, Widen::None).is_empty());
        assert_eq!(widen_samples(22_050), 1103);
    }
}

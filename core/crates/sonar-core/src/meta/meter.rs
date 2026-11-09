//! Streaming sample peak and ITU-R BS.1770-4 integrated loudness.
//!
//! Fed with planar `f32` blocks at the file's own rate (all channels,
//! before any downmix or resampling) by the decode loop, or directly.
//!
//! Loudness (BS.1770-4 §2 / Annex 1, EBU R 128 / Tech 3341 conformance):
//! 1. K-weight every channel ([`super::kweight`]);
//! 2. accumulate `Σ_c G_c Σ y_c²` per 100 ms step ([`step_frames`]); a
//!    gating block is 4 consecutive steps = 400 ms with 75 % overlap, its
//!    mean-square power `z_j = Σ_c G_c · mean(y_c²)`, its loudness
//!    `l_j = −0.691 + 10·log10(z_j)`; an incomplete trailing step is not
//!    part of any block;
//! 3. absolute gate: keep blocks with `l_j > −70` LUFS; relative gate:
//!    `Γ_r` = loudness of the mean power of those blocks − 10 LU; the
//!    result is the loudness of the mean power of blocks above both gates.
//!
//! Spans shorter than one 400 ms block (one-shots) are measured as a
//! single block over the whole span (mean power of everything measured),
//! still subject to the absolute gate: an approximation outside the
//! standard, which defines no value for them.
//!
//! Memory: one `f64` per 100 ms step (6,000 for 10 minutes); no
//! allocation per sample or packet.

use std::ops::Range;

use super::kweight::{filter_energy, filter_energy_pair, k_weighting, Biquad, FilterState};

/// Calibration offset of BS.1770 (LUFS of `z = 1`, cancels the K-filter's
/// gain at 1 kHz).
pub const LOUDNESS_OFFSET: f64 = -0.691;
/// Absolute gate (LUFS).
pub const ABSOLUTE_GATE_LUFS: f64 = -70.0;
/// Relative gate below the absolutely-gated loudness (LU).
pub const RELATIVE_GATE_LU: f64 = -10.0;
/// Steps (100 ms) per 400 ms gating block (75 % overlap).
const STEPS_PER_BLOCK: usize = 4;

/// Frames in one 100 ms gating step at `rate` (rounded to nearest, as
/// libebur128 does for rates that are not multiples of 10 Hz).
pub fn step_frames(rate: u32) -> usize {
    ((rate + 5) / 10).max(1) as usize
}

/// Peak + loudness accumulator for one file; reusable across files via
/// [`Meter::reset`] (keeps its buffers).
#[derive(Debug, Clone)]
pub struct Meter {
    rate: u32,
    max_frames: u64,
    measured: u64,
    filters: [Biquad; 2],
    weights: Vec<f64>,
    state: Vec<FilterState>,
    step_len: usize,
    step_pos: usize,
    step_acc: f64,
    steps: Vec<f64>,
    peak: f32,
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

/// Zeros fed through the filters for undecodable (silenced) packets.
static ZEROS: [f32; 1024] = [0.0; 1024];
/// Step buffers larger than this (1 h at 100 ms) are released on reset.
const RETAIN_STEPS: usize = 36_000;

impl Meter {
    /// An empty meter; call [`Meter::reset`] before feeding it.
    pub fn new() -> Self {
        Self {
            rate: 0,
            max_frames: 0,
            measured: 0,
            filters: k_weighting(48_000.0),
            weights: Vec::new(),
            state: Vec::new(),
            step_len: step_frames(48_000),
            step_pos: 0,
            step_acc: 0.0,
            steps: Vec::new(),
            peak: 0.0,
        }
    }

    /// Starts a new measurement at `rate` Hz with one BS.1770 weight per
    /// channel (see [`super::channels`]); at most `max_frames` frames are
    /// measured, later input is ignored.
    pub fn reset(&mut self, rate: u32, weights: &[f64], max_frames: u64) {
        let rate = rate.max(1);
        if rate != self.rate {
            self.filters = k_weighting(f64::from(rate));
            self.step_len = step_frames(rate);
            self.rate = rate;
        }
        self.max_frames = max_frames;
        self.measured = 0;
        self.weights.clear();
        self.weights.extend_from_slice(weights);
        self.state.clear();
        self.state.resize(weights.len(), [0.0; 6]);
        self.step_pos = 0;
        self.step_acc = 0.0;
        self.steps.clear();
        if self.steps.capacity() > RETAIN_STEPS {
            self.steps = Vec::new();
        }
        self.peak = 0.0;
    }

    /// No more input will be measured.
    pub fn is_full(&self) -> bool {
        self.measured >= self.max_frames
    }

    /// Frames measured so far.
    pub fn measured_frames(&self) -> u64 {
        self.measured
    }

    /// Seconds measured so far.
    pub fn measured_secs(&self) -> f64 {
        self.measured as f64 / f64::from(self.rate)
    }

    fn take(&self, frames: usize) -> usize {
        let left = self.max_frames.saturating_sub(self.measured);
        usize::try_from(left).map_or(frames, |l| l.min(frames))
    }

    /// Measures `planes[c][range]` for every channel `c` (planes shorter
    /// than `range.end` shorten it; never panics). Channels beyond the
    /// weights given to [`Meter::reset`] weigh 1.0.
    pub fn push(&mut self, planes: &[Vec<f32>], range: Range<usize>) {
        let shortest = planes.iter().map(Vec::len).min().unwrap_or(0);
        let end = range.end.min(shortest);
        let start = range.start.min(end);
        let n = self.take(end - start);
        if n == 0 {
            return;
        }
        if self.state.len() < planes.len() {
            self.state.resize(planes.len(), [0.0; 6]);
        }
        for p in planes {
            self.peak = peak_of(&p[start..start + n], self.peak);
        }
        let mut pos = start;
        while pos < start + n {
            let seg = (start + n - pos).min(self.step_len - self.step_pos);
            self.filter_segment(planes, pos..pos + seg);
            self.advance(seg);
            pos += seg;
        }
    }

    /// K-weights `planes[..][r]` and adds their weighted energy to the
    /// current step; channels go through the filter two at a time.
    fn filter_segment(&mut self, planes: &[Vec<f32>], r: Range<usize>) {
        let f = &self.filters;
        let weight = |c: usize| self.weights.get(c).copied().unwrap_or(1.0);
        let mut acc = 0.0;
        let mut c = 0;
        while c < planes.len() {
            let (ga, gb) = (weight(c), weight(c + 1));
            if c + 1 < planes.len() && ga != 0.0 && gb != 0.0 {
                let (sa, sb) = self.state[c..].split_at_mut(1);
                let (ea, eb) = filter_energy_pair(
                    &planes[c][r.clone()],
                    &planes[c + 1][r.clone()],
                    &mut sa[0],
                    &mut sb[0],
                    f,
                );
                acc += ga * ea + gb * eb;
                c += 2;
            } else {
                if ga != 0.0 {
                    acc += ga * filter_energy(&planes[c][r.clone()], &mut self.state[c], f);
                }
                c += 1;
            }
        }
        self.step_acc += acc;
    }

    /// Measures `frames` of digital silence on every channel (stand-in for
    /// an undecodable packet; keeps later audio time-aligned).
    pub fn push_silence(&mut self, frames: usize) {
        let mut left = self.take(frames);
        while left > 0 {
            let seg = left.min(ZEROS.len()).min(self.step_len - self.step_pos);
            for (c, st) in self.state.iter_mut().enumerate() {
                let g = self.weights.get(c).copied().unwrap_or(1.0);
                if g != 0.0 {
                    self.step_acc += g * filter_energy(&ZEROS[..seg], st, &self.filters);
                }
            }
            self.advance(seg);
            left -= seg;
        }
    }

    fn advance(&mut self, frames: usize) {
        self.measured += frames as u64;
        self.step_pos += frames;
        if self.step_pos == self.step_len {
            self.steps.push(self.step_acc);
            self.step_acc = 0.0;
            self.step_pos = 0;
        }
    }

    /// Sample peak `20·log10(max |x|)` over all channels (dBFS); `None` for
    /// digital silence or nothing measured. Non-finite samples are ignored.
    pub fn peak_dbfs(&self) -> Option<f64> {
        let db = 20.0 * f64::from(self.peak).log10();
        (self.peak > 0.0 && db.is_finite()).then_some(db)
    }

    /// Gated integrated loudness (LUFS); `None` if nothing is above the
    /// absolute gate (silence, very quiet or nothing measured).
    pub fn integrated_lufs(&self) -> Option<f64> {
        let abs_gate = lufs_to_power(ABSOLUTE_GATE_LUFS);
        let block_len = (STEPS_PER_BLOCK * self.step_len) as f64;
        if self.steps.len() < STEPS_PER_BLOCK {
            // Shorter than one gating block: the whole span is one block.
            if self.measured == 0 {
                return None;
            }
            let z = (self.steps.iter().sum::<f64>() + self.step_acc) / self.measured as f64;
            return (z > abs_gate)
                .then(|| power_to_lufs(z))
                .filter(|l| l.is_finite());
        }
        let blocks = || {
            self.steps
                .windows(STEPS_PER_BLOCK)
                .map(move |w| w.iter().sum::<f64>() / block_len)
        };
        let mean_above = |gate: f64| {
            let (sum, n) = blocks()
                .filter(|&z| z > gate)
                .fold((0.0, 0u64), |(s, n), z| (s + z, n + 1));
            (n > 0).then(|| sum / n as f64)
        };
        let ungated = mean_above(abs_gate)?;
        let rel_gate = lufs_to_power(power_to_lufs(ungated) + RELATIVE_GATE_LU);
        let gated = mean_above(abs_gate.max(rel_gate))?;
        Some(power_to_lufs(gated)).filter(|l| l.is_finite())
    }
}

/// Loudness of mean-square power `z` (LUFS).
fn power_to_lufs(z: f64) -> f64 {
    LOUDNESS_OFFSET + 10.0 * z.log10()
}

/// Mean-square power whose loudness is `lufs`.
fn lufs_to_power(lufs: f64) -> f64 {
    10f64.powf((lufs - LOUDNESS_OFFSET) / 10.0)
}

/// `max(acc, max |x|)` ignoring non-finite samples (branch-free select, so
/// the loop vectorises).
fn peak_of(x: &[f32], acc: f32) -> f32 {
    x.iter().fold(acc, |m, &s| {
        let a = s.abs();
        m.max(if a < f32::INFINITY { a } else { 0.0 })
    })
}

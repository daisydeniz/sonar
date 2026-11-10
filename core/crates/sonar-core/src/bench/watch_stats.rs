//! Pure statistics for `sonar bench watch` (no I/O, unit-tested).

use std::time::{Duration, Instant};

use serde::Serialize;

/// Distribution of the watcher's scan durations in one phase.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScanStats {
    /// Number of scans.
    pub count: usize,
    /// Shortest scan, milliseconds.
    pub min_ms: u64,
    /// Median scan (upper median for even counts), milliseconds.
    pub median_ms: u64,
    /// Longest scan, milliseconds.
    pub max_ms: u64,
    /// Sum of all scans, milliseconds.
    pub total_ms: u64,
}

impl ScanStats {
    /// Statistics of `durations_ms` (all zero when empty).
    pub fn of(durations_ms: &[u64]) -> Self {
        let mut v = durations_ms.to_vec();
        v.sort_unstable();
        Self {
            count: v.len(),
            min_ms: v.first().copied().unwrap_or(0),
            median_ms: v.get(v.len() / 2).copied().unwrap_or(0),
            max_ms: v.last().copied().unwrap_or(0),
            total_ms: v.iter().sum(),
        }
    }
}

/// A closed time interval `(start, end)`.
pub type Span = (Instant, Instant);

/// Length of the intersection of two spans (zero if disjoint).
pub fn intersection(a: Span, b: Span) -> Duration {
    let (start, end) = (a.0.max(b.0), a.1.min(b.1));
    end.saturating_duration_since(start)
}

/// Total time of `spans` (assumed pairwise disjoint) inside `window`.
pub fn busy_within(spans: &[Span], window: Span) -> Duration {
    spans.iter().map(|s| intersection(*s, window)).sum()
}

/// Share of `window` covered by `spans` (disjoint), 0..=1.
pub fn duty_within(spans: &[Span], window: Span) -> f64 {
    let len = window.1.saturating_duration_since(window.0).as_secs_f64();
    if len > 0.0 {
        (busy_within(spans, window).as_secs_f64() / len).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// How many of `spans` overlap `others` (disjoint) and for how long in
/// total.
pub fn overlap(spans: &[Span], others: &[Span]) -> (usize, Duration) {
    let mut count = 0;
    let mut total = Duration::ZERO;
    for s in spans {
        let t = busy_within(others, *s);
        if !t.is_zero() {
            count += 1;
            total += t;
        }
    }
    (count, total)
}

/// Longest gap between consecutive instants, starting from `start`
/// (so a first event that comes late counts). Zero for no instants.
pub fn max_gap(start: Instant, times: &[Instant]) -> Duration {
    let mut prev = start;
    let mut gap = Duration::ZERO;
    for &t in times {
        gap = gap.max(t.saturating_duration_since(prev));
        prev = prev.max(t);
    }
    gap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_of_durations() {
        let s = ScanStats::of(&[300, 100, 200, 900]);
        assert_eq!(
            s,
            ScanStats {
                count: 4,
                min_ms: 100,
                median_ms: 300,
                max_ms: 900,
                total_ms: 1500
            }
        );
        assert_eq!(ScanStats::of(&[7]).median_ms, 7);
    }

    #[test]
    fn empty_stats_are_zero() {
        let s = ScanStats::of(&[]);
        assert_eq!(
            (s.count, s.min_ms, s.median_ms, s.max_ms, s.total_ms),
            (0, 0, 0, 0, 0)
        );
    }

    #[test]
    fn spans_clip_and_overlap() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let scans = [(at(0), at(100)), (at(300), at(400)), (at(900), at(1000))];
        // Window 50..350 clips the first two scans: 50 + 50 ms.
        let w = (at(50), at(350));
        assert_eq!(busy_within(&scans, w), Duration::from_millis(100));
        assert!((duty_within(&scans, w) - 100.0 / 300.0).abs() < 1e-9);
        assert_eq!(duty_within(&scans, (at(5), at(5))), 0.0);
        // Pipeline active 80..320 touches the first two scans only.
        let (n, d) = overlap(&scans, &[(at(80), at(320))]);
        assert_eq!((n, d), (2, Duration::from_millis(20 + 20)));
        assert_eq!(overlap(&scans, &[]), (0, Duration::ZERO));
    }

    #[test]
    fn max_gap_includes_the_wait_for_the_first_event() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        assert_eq!(max_gap(t0, &[]), Duration::ZERO);
        assert_eq!(
            max_gap(t0, &[at(10), at(40), at(50)]),
            Duration::from_millis(30)
        );
        assert_eq!(max_gap(t0, &[at(200), at(210)]), Duration::from_millis(200));
    }
}

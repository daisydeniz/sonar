//! Per-library debounce as a pure state machine: callers pass `now`, so
//! tests need no real time.
//!
//! A burst of events for a library is coalesced into one rescan that fires
//! after `quiet` without new events, but never later than `max_delay` after
//! the first event of the burst (a long copy still gets scanned
//! periodically). An *urgent* request (root appeared or vanished, manual
//! rescan) is due immediately, and [`Debouncer::retry_at`] defers a rescan
//! to a fixed time (back-off after an error).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct Entry {
    first: Instant,
    last: Instant,
    urgent: bool,
    not_before: Instant,
}

impl Entry {
    /// Due time; `cooldown` (end of the post-scan spacing) only delays
    /// non-urgent entries.
    fn deadline(&self, quiet: Duration, max_delay: Duration, cooldown: Option<Instant>) -> Instant {
        let base = if self.urgent {
            self.not_before
        } else {
            let b = (self.last + quiet).min(self.first + max_delay);
            cooldown.map_or(b, |c| b.max(c))
        };
        base.max(self.not_before)
    }
}

/// Coalesces filesystem events into "scan library X now" decisions.
#[derive(Debug)]
pub struct Debouncer {
    quiet: Duration,
    max_delay: Duration,
    pending: BTreeMap<i64, Entry>,
    /// Per library: no event-driven scan before this instant.
    cooldown: BTreeMap<i64, Instant>,
}

impl Debouncer {
    fn deadline_of(&self, id: i64, e: &Entry) -> Instant {
        e.deadline(self.quiet, self.max_delay, self.cooldown.get(&id).copied())
    }

    /// Records that a scan of `library` finished at `now` after `took`:
    /// event-driven (non-urgent) rescans of it wait until
    /// `now + 2 * took` (on top of the usual quiet period), so scanning
    /// takes at most ~1/3 of the time during a continuous copy into a large
    /// library, while small libraries (ms scans) still react within `quiet`.
    /// Urgent requests ignore this.
    pub fn scan_finished(&mut self, library: i64, now: Instant, took: Duration) {
        self.cooldown.insert(library, now + took.saturating_mul(2));
    }

    /// `quiet`: silence required before a scan; `max_delay`: cap on how long
    /// a continuous burst can postpone it (raised to at least `quiet`).
    pub fn new(quiet: Duration, max_delay: Duration) -> Self {
        Self {
            quiet,
            max_delay: max_delay.max(quiet),
            pending: BTreeMap::new(),
            cooldown: BTreeMap::new(),
        }
    }

    /// Records a relevant event for `library` at `now`.
    pub fn note(&mut self, library: i64, now: Instant) {
        self.pending
            .entry(library)
            .and_modify(|e| e.last = now)
            .or_insert(Entry {
                first: now,
                last: now,
                urgent: false,
                not_before: now,
            });
    }

    /// Requests a scan of `library` as soon as possible.
    pub fn urgent(&mut self, library: i64, now: Instant) {
        let e = self.pending.entry(library).or_insert(Entry {
            first: now,
            last: now,
            urgent: true,
            not_before: now,
        });
        e.urgent = true;
    }

    /// Requests a scan of `library` at `at` (or later), replacing any
    /// shorter wait: used to back off after a failed scan.
    pub fn retry_at(&mut self, library: i64, now: Instant, at: Instant) {
        self.pending.insert(
            library,
            Entry {
                first: now,
                last: now,
                urgent: true,
                not_before: at,
            },
        );
    }

    /// Drops any pending scan for `library` (library removed).
    pub fn forget(&mut self, library: i64) {
        self.pending.remove(&library);
        self.cooldown.remove(&library);
    }

    /// Removes and returns the libraries due at `now`, ordered by id.
    pub fn take_due(&mut self, now: Instant) -> Vec<i64> {
        let due: Vec<i64> = self
            .pending
            .iter()
            .filter(|(id, e)| self.deadline_of(**id, e) <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in &due {
            self.pending.remove(id);
        }
        due
    }

    /// Time until the earliest pending scan is due (zero if overdue), or
    /// `None` if nothing is pending.
    pub fn next_wait(&self, now: Instant) -> Option<Duration> {
        self.pending
            .iter()
            .map(|(id, e)| self.deadline_of(*id, e))
            .min()
            .map(|d| d.saturating_duration_since(now))
    }

    /// True if a scan of `library` is pending.
    pub fn is_pending(&self, library: i64) -> bool {
        self.pending.contains_key(&library)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn deb() -> (Debouncer, Instant) {
        (Debouncer::new(ms(400), ms(5000)), Instant::now())
    }

    #[test]
    fn fires_after_quiet_period() {
        let (mut d, t0) = deb();
        d.note(1, t0);
        assert!(d.take_due(t0 + ms(399)).is_empty());
        assert_eq!(d.take_due(t0 + ms(400)), vec![1]);
        assert!(d.take_due(t0 + ms(9000)).is_empty(), "taken once");
    }

    #[test]
    fn new_events_extend_the_quiet_period() {
        let (mut d, t0) = deb();
        d.note(1, t0);
        d.note(1, t0 + ms(300));
        assert!(d.take_due(t0 + ms(500)).is_empty());
        assert_eq!(d.take_due(t0 + ms(700)), vec![1]);
    }

    #[test]
    fn continuous_burst_is_capped_by_max_delay() {
        let (mut d, t0) = deb();
        let mut t = t0;
        while t < t0 + ms(4900) {
            d.note(1, t);
            assert!(d.take_due(t).is_empty());
            t += ms(100);
        }
        d.note(1, t0 + ms(4950));
        assert_eq!(d.take_due(t0 + ms(5000)), vec![1]);
    }

    #[test]
    fn libraries_are_independent_and_ordered() {
        let (mut d, t0) = deb();
        d.note(2, t0);
        d.note(1, t0 + ms(100));
        assert_eq!(d.take_due(t0 + ms(400)), vec![2]);
        assert_eq!(d.take_due(t0 + ms(500)), vec![1]);
        d.note(3, t0);
        d.note(2, t0);
        assert_eq!(d.take_due(t0 + ms(400)), vec![2, 3]);
    }

    #[test]
    fn urgent_is_due_immediately_and_overrides_waiting() {
        let (mut d, t0) = deb();
        d.note(1, t0);
        d.urgent(1, t0 + ms(10));
        assert_eq!(d.take_due(t0 + ms(10)), vec![1]);
        d.urgent(2, t0);
        assert_eq!(d.next_wait(t0), Some(Duration::ZERO));
    }

    #[test]
    fn retry_waits_until_the_given_time_even_with_new_events() {
        let (mut d, t0) = deb();
        d.retry_at(1, t0, t0 + ms(3000));
        d.note(1, t0 + ms(100));
        assert!(d.take_due(t0 + ms(2999)).is_empty());
        assert_eq!(d.take_due(t0 + ms(3000)), vec![1]);
    }

    #[test]
    fn next_wait_tracks_earliest_and_forget_clears() {
        let (mut d, t0) = deb();
        assert_eq!(d.next_wait(t0), None);
        d.note(1, t0);
        d.note(2, t0 + ms(200));
        assert_eq!(d.next_wait(t0 + ms(100)), Some(ms(300)));
        assert_eq!(d.next_wait(t0 + ms(900)), Some(Duration::ZERO));
        d.forget(1);
        assert!(!d.is_pending(1));
        assert_eq!(d.next_wait(t0 + ms(200)), Some(ms(400)));
    }

    #[test]
    fn scans_are_spaced_by_twice_the_scan_time() {
        let (mut d, t0) = deb();
        // A fast scan: spacing 2 ms < quiet, so only the quiet period counts.
        d.scan_finished(1, t0, ms(1));
        d.note(1, t0);
        assert!(d.take_due(t0 + ms(399)).is_empty());
        assert_eq!(d.take_due(t0 + ms(400)), vec![1]);

        // A slow scan: 2 x 4 s = 8 s, beyond quiet and max_delay.
        d.scan_finished(1, t0, ms(4000));
        d.note(1, t0);
        assert!(d.take_due(t0 + ms(7999)).is_empty());
        assert_eq!(d.take_due(t0 + ms(8000)), vec![1]);
    }

    #[test]
    fn urgent_ignores_the_spacing_and_other_libraries_are_unaffected() {
        let (mut d, t0) = deb();
        d.scan_finished(1, t0, ms(100));
        d.urgent(1, t0);
        d.note(2, t0);
        assert_eq!(d.take_due(t0 + ms(400)), vec![1, 2]);
        d.scan_finished(1, t0, ms(100));
        d.forget(1);
        d.note(1, t0);
        assert_eq!(d.take_due(t0 + ms(400)), vec![1], "forget clears spacing");
    }

    #[test]
    fn max_delay_is_never_below_quiet() {
        let mut d = Debouncer::new(ms(400), ms(100));
        let t0 = Instant::now();
        d.note(1, t0);
        assert!(d.take_due(t0 + ms(399)).is_empty());
        assert_eq!(d.take_due(t0 + ms(400)), vec![1]);
    }
}

//! The quiet hook swallows messages only inside `guarded`. Lives in its own
//! test binary because the panic hook is process-global: a single test
//! installs a counting hook first, so the order is deterministic.

use std::panic;
use std::sync::atomic::{AtomicUsize, Ordering};

use sonar_core::quiet_panic;

static PRINTED: AtomicUsize = AtomicUsize::new(0);

#[test]
fn guarded_panics_are_silent_and_others_reach_the_previous_hook() {
    // Stands in for the default hook (which prints).
    panic::set_hook(Box::new(|_| {
        PRINTED.fetch_add(1, Ordering::SeqCst);
    }));

    let r = quiet_panic::guarded(|| -> u8 { panic!("inside") });
    assert_eq!(r, Err("inside".to_owned()));
    assert_eq!(PRINTED.load(Ordering::SeqCst), 0, "guarded panic is silent");

    // A decode of garbage that panics internally would take the same path;
    // here: a plain panic outside any guard still reaches the old hook.
    let outside = panic::catch_unwind(|| -> u8 { panic!("outside") });
    assert!(outside.is_err());
    assert_eq!(PRINTED.load(Ordering::SeqCst), 1, "unguarded panic prints");

    // Still silent afterwards (depth restored after the unwind).
    let _ = quiet_panic::guarded(|| -> u8 { panic!("again") });
    assert_eq!(PRINTED.load(Ordering::SeqCst), 1);
}

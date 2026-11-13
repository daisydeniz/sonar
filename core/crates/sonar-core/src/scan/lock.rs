//! Process-wide per-library scan lock.
//!
//! Two scans of the same library (the watcher's and a manual one, say) would
//! both plan "add this file" against the same snapshot and the second insert
//! would fail on `UNIQUE(library_id, rel_path)`. Scans of one library on one
//! database therefore run one at a time; scans of different libraries stay
//! parallel.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

type Key = (Option<PathBuf>, i64);

static HELD: Mutex<Option<HashSet<Key>>> = Mutex::new(None);
static FREED: Condvar = Condvar::new();

fn held() -> MutexGuard<'static, Option<HashSet<Key>>> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Holds the scan slot of one library until dropped.
#[derive(Debug)]
pub(crate) struct ScanSlot(Key);

impl ScanSlot {
    /// Waits for the slot of `(db, library_id)`; `None` if `cancel` was set
    /// while waiting.
    pub(crate) fn acquire(
        db: Option<PathBuf>,
        library_id: i64,
        cancel: &AtomicBool,
    ) -> Option<Self> {
        let key = (db, library_id);
        let mut guard = held();
        loop {
            let set = guard.get_or_insert_with(HashSet::new);
            if set.insert(key.clone()) {
                return Some(Self(key));
            }
            if cancel.load(Ordering::SeqCst) {
                return None;
            }
            guard = FREED
                .wait_timeout(guard, Duration::from_millis(50))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for ScanSlot {
    fn drop(&mut self) {
        if let Some(set) = held().as_mut() {
            set.remove(&self.0);
        }
        FREED.notify_all();
    }
}

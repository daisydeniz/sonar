//! Quiet panic hook for guarded sections (decode and the job workers).
//!
//! Code that wraps third-party parsers in `catch_unwind` turns a panic into
//! an error value, but the default panic hook still prints
//! `thread '...' panicked at ...` to stderr for every damaged file. This
//! module installs, once per process, a hook that wraps the previous one and
//! stays silent (a `tracing` warning instead) while the current thread is
//! inside [`guarded`]. Panics anywhere else still reach the previous hook
//! and print normally.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;

thread_local! {
    /// Nesting depth of [`guarded`] on this thread.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

static INSTALL: Once = Once::new();

/// Installs the wrapping hook (idempotent). Called by [`guarded`]; exposed
/// so an application can install it up front, before other hooks.
pub fn install() {
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if is_guarded() {
                tracing::warn!("recovered panic in guarded section: {info}");
            } else {
                previous(info);
            }
        }));
    });
}

/// True while the current thread runs inside [`guarded`].
pub fn is_guarded() -> bool {
    DEPTH.with(|d| d.get() > 0)
}

/// Decrements the depth even if unwinding passes through.
struct DepthGuard;

impl DepthGuard {
    fn enter() -> Self {
        DEPTH.with(|d| d.set(d.get() + 1));
        Self
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Runs `f` under `catch_unwind` with panic messages suppressed; a panic
/// becomes `Err` carrying its message.
pub fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    install();
    let _depth = DepthGuard::enter();
    panic::catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_returns_value_or_panic_message() {
        assert_eq!(guarded(|| 7), Ok(7));
        assert_eq!(
            guarded(|| -> u8 { panic!("boom {}", 1) }),
            Err("boom 1".into())
        );
        assert_eq!(guarded(|| -> u8 { panic!("static") }), Err("static".into()));
    }

    #[test]
    fn flag_is_set_only_inside_and_survives_nesting() {
        assert!(!is_guarded());
        let _ = guarded(|| {
            assert!(is_guarded());
            let _ = guarded(|| -> u8 { panic!("inner") });
            assert!(is_guarded(), "outer section still guarded");
        });
        assert!(!is_guarded());
    }
}

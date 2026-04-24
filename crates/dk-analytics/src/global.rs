//! Process-global analytics sink handle.
//!
//! Callers that don't want to plumb a [`SinkHandle`] through every layer can
//! set a shared handle once at startup and let downstream code call
//! [`emit`]. When no handle has been installed, `emit` is a cheap no-op, so
//! instrumented call sites stay safe even in unit tests.

use std::sync::OnceLock;

use crate::events::AnalyticsEvent;
use crate::sink::SinkHandle;

static GLOBAL: OnceLock<SinkHandle> = OnceLock::new();

/// Install the process-global handle. Subsequent calls are ignored — the
/// first installer wins. Safe to call from any thread.
pub fn set(handle: SinkHandle) -> bool {
    GLOBAL.set(handle).is_ok()
}

/// Borrow the global handle, if any.
pub fn get() -> Option<&'static SinkHandle> {
    GLOBAL.get()
}

/// Emit an event via the global handle, or drop it if none is installed.
///
/// This is the primary entry point for instrumented call sites. It is
/// intentionally infallible — analytics must never fail the hot path.
pub fn emit(event: AnalyticsEvent) {
    if let Some(h) = GLOBAL.get() {
        h.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_is_noop_without_handle_installed() {
        // We can't guarantee ordering with other tests, but if no handle was
        // installed this invocation should just silently return.
        emit(crate::events::AnalyticsEvent::Verification(
            crate::test_fixtures::verification_run_fixture(),
        ));
    }
}

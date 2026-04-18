//! In-process counters for the release-locks-at-submit feature.
//!
//! PR1 uses `AtomicU64` counters rather than pulling in a full metrics crate.
//! The values are emitted through `tracing::info!` events with a standardized
//! `metric` field so existing log-based tooling can aggregate them, and the
//! counters themselves are readable from tests and any future Prometheus
//! exporter we might bolt on.

use std::sync::atomic::{AtomicU64, Ordering};

/// Number of symbol locks released on `dk_submit` (flag-gated site).
/// Zero while `DKOD_RELEASE_ON_SUBMIT=0`; strictly monotonic while on.
static LOCKS_RELEASED_ON_SUBMIT_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Number of `dk_file_write` calls rejected by the STALE_OVERLAY pre-check.
/// A non-zero value in the testbed is a signal to inspect the agent prompt
/// / harness flow, not the engine — the check is a backstop, not a primary
/// correctness mechanism.
static STALE_OVERLAY_REJECTED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Increment the "locks released on submit" counter and emit a structured
/// tracing event so log-based aggregators can surface it.
pub fn incr_locks_released_on_submit(count: u64) {
    if count == 0 {
        return;
    }
    LOCKS_RELEASED_ON_SUBMIT_TOTAL.fetch_add(count, Ordering::Relaxed);
    tracing::info!(
        metric = "dkod_engine_locks_released_on_submit_total",
        increment = count,
        "metrics counter"
    );
}

/// Increment the "STALE_OVERLAY rejected" counter.
pub fn incr_stale_overlay_rejected() {
    STALE_OVERLAY_REJECTED_TOTAL.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        metric = "dkod_engine_stale_overlay_rejected_total",
        increment = 1,
        "metrics counter"
    );
}

/// Snapshot of the "locks released on submit" counter (for tests + future
/// scrape endpoints).
pub fn locks_released_on_submit_total() -> u64 {
    LOCKS_RELEASED_ON_SUBMIT_TOTAL.load(Ordering::Relaxed)
}

/// Snapshot of the "STALE_OVERLAY rejected" counter.
pub fn stale_overlay_rejected_total() -> u64 {
    STALE_OVERLAY_REJECTED_TOTAL.load(Ordering::Relaxed)
}

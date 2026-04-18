//! In-process counters and gauges for workspace lifecycle (Epic B).
//!
//! Follows the same pattern as `dk-protocol/src/metrics.rs`: `AtomicU64` /
//! `AtomicI64` counters backed by `tracing::info!` events with a `metric`
//! field so log-based aggregators can surface them.  The counters are also
//! readable from tests and any future Prometheus exporter.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

// ── Counters ──────────────────────────────────────────────────────────────────

/// Workspaces skipped by GC because the pin rule applied.
static WORKSPACE_PINNED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Workspaces transitioned to stranded state.
static WORKSPACE_STRANDED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Resume attempts, by outcome label.
static WORKSPACE_RESUMED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Workspaces permanently abandoned.
static WORKSPACE_ABANDONED_TOTAL: AtomicU64 = AtomicU64::new(0);

// ── Gauge ─────────────────────────────────────────────────────────────────────

/// Rows where `stranded_at IS NOT NULL AND abandoned_at IS NULL`.
static WORKSPACE_STRANDED_ACTIVE: AtomicI64 = AtomicI64::new(0);

// ── Public helpers ────────────────────────────────────────────────────────────

/// Increment "workspace pinned" counter.
pub fn incr_workspace_pinned(reason: &str) {
    WORKSPACE_PINNED_TOTAL.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        metric = "dkod_workspace_pinned_total",
        reason,
        increment = 1,
        "metrics counter"
    );
}

/// Increment "workspace stranded" counter.
pub fn incr_workspace_stranded(reason: &str) {
    WORKSPACE_STRANDED_TOTAL.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        metric = "dkod_workspace_stranded_total",
        reason,
        increment = 1,
        "metrics counter"
    );
}

/// Increment "workspace resumed" counter.
pub fn incr_workspace_resumed(outcome: &str) {
    WORKSPACE_RESUMED_TOTAL.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        metric = "dkod_workspace_resumed_total",
        outcome,
        increment = 1,
        "metrics counter"
    );
}

/// Increment "workspace abandoned" counter.
pub fn incr_workspace_abandoned(reason: &str) {
    WORKSPACE_ABANDONED_TOTAL.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        metric = "dkod_workspace_abandoned_total",
        reason,
        increment = 1,
        "metrics counter"
    );
}

/// Set the stranded-active gauge to `n`
/// (`COUNT(*) WHERE stranded_at IS NOT NULL AND abandoned_at IS NULL`).
pub fn set_workspace_stranded_active(n: i64) {
    WORKSPACE_STRANDED_ACTIVE.store(n, Ordering::Relaxed);
    tracing::info!(
        metric = "dkod_workspace_stranded_active",
        value = n,
        "metrics gauge"
    );
}

// ── Snapshot helpers (tests + future scrape) ──────────────────────────────────

pub fn workspace_pinned_total() -> u64 {
    WORKSPACE_PINNED_TOTAL.load(Ordering::Relaxed)
}
pub fn workspace_stranded_total() -> u64 {
    WORKSPACE_STRANDED_TOTAL.load(Ordering::Relaxed)
}
pub fn workspace_resumed_total() -> u64 {
    WORKSPACE_RESUMED_TOTAL.load(Ordering::Relaxed)
}
pub fn workspace_abandoned_total() -> u64 {
    WORKSPACE_ABANDONED_TOTAL.load(Ordering::Relaxed)
}
pub fn workspace_stranded_active() -> i64 {
    WORKSPACE_STRANDED_ACTIVE.load(Ordering::Relaxed)
}

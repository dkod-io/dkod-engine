//! In-process counters and gauges for workspace lifecycle (Epic B).
//!
//! Follows the same pattern as `dk-protocol/src/metrics.rs`: per-label counters
//! backed by `tracing::info!` events with a `metric` field so log-based
//! aggregators can surface them.  Each labeled counter family is stored in a
//! `DashMap<String, AtomicU64>` so a future Prometheus exporter can iterate
//! over all observed label values.  The gauge remains a plain `AtomicI64`
//! because it has no label dimension.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::LazyLock;

use dashmap::DashMap;

// ── Labeled counter families ──────────────────────────────────────────────────

/// Workspaces skipped by GC because the pin rule applied, keyed by `reason`.
static WORKSPACE_PINNED: LazyLock<DashMap<String, AtomicU64>> = LazyLock::new(DashMap::new);

/// Workspaces transitioned to stranded state, keyed by `reason`.
static WORKSPACE_STRANDED: LazyLock<DashMap<String, AtomicU64>> = LazyLock::new(DashMap::new);

/// Resume attempts, keyed by `outcome`.
static WORKSPACE_RESUMED: LazyLock<DashMap<String, AtomicU64>> = LazyLock::new(DashMap::new);

/// Workspaces permanently abandoned, keyed by `reason`.
static WORKSPACE_ABANDONED: LazyLock<DashMap<String, AtomicU64>> = LazyLock::new(DashMap::new);

// ── Gauge ─────────────────────────────────────────────────────────────────────

/// Rows where `stranded_at IS NOT NULL AND abandoned_at IS NULL`.
static WORKSPACE_STRANDED_ACTIVE: AtomicI64 = AtomicI64::new(0);

// ── Private helper ────────────────────────────────────────────────────────────

#[inline]
fn incr_labeled(map: &DashMap<String, AtomicU64>, label: &str) {
    map.entry(label.to_string())
        .or_insert_with(|| AtomicU64::new(0))
        .fetch_add(1, Ordering::Relaxed);
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// Increment "workspace pinned" counter for the given `reason` label.
pub fn incr_workspace_pinned(reason: &str) {
    incr_labeled(&WORKSPACE_PINNED, reason);
    tracing::info!(
        metric = "dkod_workspace_pinned_total",
        reason,
        increment = 1,
        "metrics counter"
    );
}

/// Increment "workspace stranded" counter for the given `reason` label.
pub fn incr_workspace_stranded(reason: &str) {
    incr_labeled(&WORKSPACE_STRANDED, reason);
    tracing::info!(
        metric = "dkod_workspace_stranded_total",
        reason,
        increment = 1,
        "metrics counter"
    );
}

/// Increment "workspace resumed" counter for the given `outcome` label.
pub fn incr_workspace_resumed(outcome: &str) {
    incr_labeled(&WORKSPACE_RESUMED, outcome);
    tracing::info!(
        metric = "dkod_workspace_resumed_total",
        outcome,
        increment = 1,
        "metrics counter"
    );
}

/// Increment "workspace abandoned" counter for the given `reason` label.
pub fn incr_workspace_abandoned(reason: &str) {
    incr_labeled(&WORKSPACE_ABANDONED, reason);
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

/// Total pinned events across all reason labels.
pub fn workspace_pinned_total() -> u64 {
    WORKSPACE_PINNED
        .iter()
        .map(|e| e.value().load(Ordering::Relaxed))
        .sum()
}

/// Total stranded events across all reason labels.
pub fn workspace_stranded_total() -> u64 {
    WORKSPACE_STRANDED
        .iter()
        .map(|e| e.value().load(Ordering::Relaxed))
        .sum()
}

/// Total resumed events across all outcome labels.
pub fn workspace_resumed_total() -> u64 {
    WORKSPACE_RESUMED
        .iter()
        .map(|e| e.value().load(Ordering::Relaxed))
        .sum()
}

/// Total abandoned events across all reason labels.
pub fn workspace_abandoned_total() -> u64 {
    WORKSPACE_ABANDONED
        .iter()
        .map(|e| e.value().load(Ordering::Relaxed))
        .sum()
}

pub fn workspace_stranded_active() -> i64 {
    WORKSPACE_STRANDED_ACTIVE.load(Ordering::Relaxed)
}

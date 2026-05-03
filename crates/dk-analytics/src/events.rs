//! Strongly-typed event structs matching the ClickHouse tables declared in
//! [`crate::schema`].
//!
//! The [`clickhouse::Row`] derive serialises these into ClickHouse's
//! `RowBinary` wire format; the column order of each struct MUST match the
//! CREATE TABLE column order for the corresponding table.

use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Wall-clock ClickHouse `DateTime64(3)` value used across all event types.
/// We serialise as a unix-ms timestamp via `serde(with = "clickhouse::serde::chrono::datetime64::millis")`
/// so the clickhouse crate round-trips correctly.
pub type Timestamp = DateTime<Utc>;

/// `session_events` row.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct SessionEvent {
    #[serde(with = "clickhouse::serde::uuid")]
    pub event_id: Uuid,
    pub event_type: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub session_id: Uuid,
    pub agent_id: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub repo_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid::option")]
    pub changeset_id: Option<Uuid>,
    pub details: String,
    pub affected_symbols: Vec<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: Timestamp,
}

/// `changeset_lifecycle` row — one per state transition.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct ChangesetLifecycle {
    #[serde(with = "clickhouse::serde::uuid")]
    pub changeset_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    pub repo_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    pub session_id: Uuid,
    pub agent_id: String,
    pub state: String,
    pub previous_state: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub transition_at: Timestamp,
    pub duration_ms: Option<u64>,
}

/// `verification_runs` row — one per verification step.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct VerificationRun {
    #[serde(with = "clickhouse::serde::uuid")]
    pub run_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    pub changeset_id: Uuid,
    pub step_name: String,
    pub status: String,
    pub duration_ms: u64,
    pub stdout: String,
    pub findings_count: u32,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: Timestamp,
}

/// `review_results` row — one per completed deep review.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct ReviewResult {
    #[serde(with = "clickhouse::serde::uuid")]
    pub review_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    pub changeset_id: Uuid,
    pub provider: String,
    pub model: String,
    pub score: Option<i32>,
    pub findings_count: u32,
    pub verdict: String,
    pub duration_ms: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: Timestamp,
}

/// Tagged union of all event types the sink can accept. Each variant is
/// routed to the corresponding ClickHouse table.
#[derive(Debug, Clone)]
pub enum AnalyticsEvent {
    Session(SessionEvent),
    Changeset(ChangesetLifecycle),
    Verification(VerificationRun),
    Review(ReviewResult),
}

impl AnalyticsEvent {
    pub fn table_name(&self) -> &'static str {
        match self {
            AnalyticsEvent::Session(_) => "session_events",
            AnalyticsEvent::Changeset(_) => "changeset_lifecycle",
            AnalyticsEvent::Verification(_) => "verification_runs",
            AnalyticsEvent::Review(_) => "review_results",
        }
    }
}

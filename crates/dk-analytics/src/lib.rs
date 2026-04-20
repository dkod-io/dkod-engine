//! `dk-analytics` — ClickHouse event pipeline for dkod.
//!
//! This crate is intentionally optional at runtime: when `CLICKHOUSE_URL`
//! is unset, [`sink::SinkHandle::noop`] returns a handle whose `emit` is a
//! drop, and no background task is spawned. Call sites should always go
//! through [`sink::SinkHandle`] so adding analytics to a code path is a
//! zero-change operation for users who haven't configured ClickHouse.
//!
//! ## Layout
//!
//! | Module | Role |
//! |--------|------|
//! | [`client`] | Thin wrapper around `clickhouse::Client` with env-driven config |
//! | [`events`] | Row structs matching the ClickHouse schema |
//! | [`schema`] | DDL + migrator (also exported as `schema.sql`) |
//! | [`sink`] | Async mpsc-based batching writer |
//! | [`pytorch_bridge`] | Poll pytorch/test-infra CI signals and normalise to `verification_runs` |
//!
//! ## Quick start
//!
//! ```no_run
//! # async fn example() -> anyhow::Result<()> {
//! let (handle, task) = dk_analytics::sink::spawn_from_env().await?;
//! handle.emit(dk_analytics::events::AnalyticsEvent::Verification(
//!     dk_analytics::events::VerificationRun {
//!         run_id: uuid::Uuid::new_v4(),
//!         changeset_id: uuid::Uuid::new_v4(),
//!         step_name: "clippy".into(),
//!         status: "pass".into(),
//!         duration_ms: 1234,
//!         stdout: "".into(),
//!         findings_count: 0,
//!         created_at: chrono::Utc::now(),
//!     },
//! ));
//! drop(handle);
//! if let Some(t) = task { t.await?; }
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod events;
pub mod global;
pub mod pytorch_bridge;
pub mod schema;
pub mod sink;

pub use client::{AnalyticsClient, AnalyticsConfig};
pub use events::{AnalyticsEvent, ChangesetLifecycle, ReviewResult, SessionEvent, VerificationRun};
pub use sink::{SinkConfig, SinkHandle};

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod test_fixtures {
    use chrono::Utc;
    use uuid::Uuid;

    use crate::events::{ChangesetLifecycle, ReviewResult, SessionEvent, VerificationRun};

    pub fn session_event_fixture() -> SessionEvent {
        SessionEvent {
            event_id: Uuid::nil(),
            event_type: "test".into(),
            session_id: Uuid::nil(),
            agent_id: "agent".into(),
            repo_id: Uuid::nil(),
            changeset_id: None,
            details: "{}".into(),
            affected_symbols: Vec::new(),
            created_at: Utc::now(),
        }
    }

    pub fn changeset_lifecycle_fixture() -> ChangesetLifecycle {
        ChangesetLifecycle {
            changeset_id: Uuid::nil(),
            repo_id: Uuid::nil(),
            session_id: Uuid::nil(),
            agent_id: "agent".into(),
            state: "verified".into(),
            previous_state: Some("submitted".into()),
            transition_at: Utc::now(),
            duration_ms: Some(1000),
        }
    }

    pub fn verification_run_fixture() -> VerificationRun {
        VerificationRun {
            run_id: Uuid::nil(),
            changeset_id: Uuid::nil(),
            step_name: "clippy".into(),
            status: "pass".into(),
            duration_ms: 42,
            stdout: "".into(),
            findings_count: 0,
            created_at: Utc::now(),
        }
    }

    pub fn review_result_fixture() -> ReviewResult {
        ReviewResult {
            review_id: Uuid::nil(),
            changeset_id: Uuid::nil(),
            provider: "anthropic".into(),
            model: "claude-opus-4-7".into(),
            score: Some(4),
            findings_count: 1,
            verdict: "approve".into(),
            duration_ms: 15_000,
            created_at: Utc::now(),
        }
    }
}

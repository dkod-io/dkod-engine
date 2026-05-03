//! Async analytics event sink.
//!
//! Call sites hand a cheap [`SinkHandle`] around. When ClickHouse is
//! configured (`CLICKHOUSE_URL` present), events are forwarded over an
//! `mpsc` channel to a background task that batches them by table and
//! writes via `INSERT ... RowBinary` once either:
//!
//!   * the batch reaches [`SinkConfig::batch_size`] rows, OR
//!   * [`SinkConfig::flush_interval`] has elapsed since the oldest buffered row.
//!
//! When ClickHouse is not configured, [`SinkHandle::noop`] returns a handle
//! whose `emit` is a cheap drop — no task is spawned.

use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::client::AnalyticsClient;
use crate::events::{
    AnalyticsEvent, ChangesetLifecycle, ReviewResult, SessionEvent, VerificationRun,
};

#[derive(Clone, Debug)]
pub struct SinkConfig {
    pub batch_size: usize,
    pub flush_interval: Duration,
    /// Max events buffered in the channel. When full, `emit` drops the event
    /// rather than applying backpressure to business logic.
    pub channel_capacity: usize,
}

impl Default for SinkConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            flush_interval: Duration::from_secs(5),
            channel_capacity: 10_000,
        }
    }
}

#[derive(Clone)]
enum Inner {
    NoOp,
    Active { tx: mpsc::Sender<AnalyticsEvent> },
}

/// Cheap cloneable handle for emitting events.
#[derive(Clone)]
pub struct SinkHandle {
    inner: Inner,
}

impl SinkHandle {
    /// A handle that drops every event. Used when `CLICKHOUSE_URL` is unset.
    pub fn noop() -> Self {
        Self { inner: Inner::NoOp }
    }

    pub fn is_noop(&self) -> bool {
        matches!(self.inner, Inner::NoOp)
    }

    /// Fire-and-forget. Returns `true` when enqueued, `false` when the sink
    /// is no-op or the channel is full (event dropped). Never blocks on a
    /// slow ClickHouse backend.
    pub fn emit(&self, event: AnalyticsEvent) -> bool {
        match &self.inner {
            Inner::NoOp => false,
            Inner::Active { tx } => match tx.try_send(event) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!(target: "dk_analytics", "sink channel full — dropping event");
                    false
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    tracing::debug!(target: "dk_analytics", "sink channel closed — ignoring event");
                    false
                }
            },
        }
    }
}

/// Spawn the background flusher task. Returns a [`SinkHandle`] for the
/// application to use, plus a [`tokio::task::JoinHandle`] so callers can
/// await graceful shutdown.
pub fn spawn(
    client: AnalyticsClient,
    config: SinkConfig,
) -> (SinkHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<AnalyticsEvent>(config.channel_capacity);
    let handle = tokio::spawn(run_sink(client, config, rx));
    (
        SinkHandle {
            inner: Inner::Active { tx },
        },
        handle,
    )
}

/// Top-level entrypoint: construct a handle from the environment, spawning
/// the background task when `CLICKHOUSE_URL` is set. Returns `Ok(noop())`
/// when unset so callers can treat analytics as always-available.
pub async fn spawn_from_env() -> anyhow::Result<(SinkHandle, Option<tokio::task::JoinHandle<()>>)> {
    let Some(client) = AnalyticsClient::from_env()? else {
        return Ok((SinkHandle::noop(), None));
    };
    let (handle, task) = spawn(client, SinkConfig::default());
    Ok((handle, Some(task)))
}

#[derive(Default)]
struct Batch {
    session: Vec<SessionEvent>,
    changeset: Vec<ChangesetLifecycle>,
    verification: Vec<VerificationRun>,
    review: Vec<ReviewResult>,
    first_pushed_at: Option<Instant>,
}

impl Batch {
    fn push(&mut self, event: AnalyticsEvent) {
        if self.first_pushed_at.is_none() {
            self.first_pushed_at = Some(Instant::now());
        }
        match event {
            AnalyticsEvent::Session(r) => self.session.push(r),
            AnalyticsEvent::Changeset(r) => self.changeset.push(r),
            AnalyticsEvent::Verification(r) => self.verification.push(r),
            AnalyticsEvent::Review(r) => self.review.push(r),
        }
    }

    fn len(&self) -> usize {
        self.session.len() + self.changeset.len() + self.verification.len() + self.review.len()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn age(&self) -> Duration {
        self.first_pushed_at
            .map(|t| t.elapsed())
            .unwrap_or_default()
    }

    fn take(&mut self) -> Self {
        Batch {
            session: std::mem::take(&mut self.session),
            changeset: std::mem::take(&mut self.changeset),
            verification: std::mem::take(&mut self.verification),
            review: std::mem::take(&mut self.review),
            first_pushed_at: self.first_pushed_at.take(),
        }
    }
}

async fn run_sink(
    client: AnalyticsClient,
    config: SinkConfig,
    mut rx: mpsc::Receiver<AnalyticsEvent>,
) {
    let mut batch = Batch::default();
    let mut ticker = tokio::time::interval(config.flush_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        batch.push(event);
                        if batch.len() >= config.batch_size {
                            flush(&client, batch.take()).await;
                        }
                    }
                    None => {
                        // Sender side closed — final flush then exit.
                        if !batch.is_empty() {
                            flush(&client, batch.take()).await;
                        }
                        break;
                    }
                }
            }
            _ = ticker.tick() => {
                if !batch.is_empty() && batch.age() >= config.flush_interval {
                    flush(&client, batch.take()).await;
                }
            }
        }
    }
}

// The `insert` helper is split into one async fn per table because
// `clickhouse::Row` uses a GAT for borrowed row views, which makes a single
// `T: Row + Serialize` generic unwieldy (every call site would have to prove
// `for<'a> T::Value<'a>: Serialize`). Four trivial specialisations are
// clearer than the HRTB dance.
async fn flush(client: &AnalyticsClient, batch: Batch) {
    if !batch.session.is_empty() {
        let n = batch.session.len();
        let res = flush_session(client, batch.session).await;
        log_flush_result("session_events", n, res);
    }
    if !batch.changeset.is_empty() {
        let n = batch.changeset.len();
        let res = flush_changeset(client, batch.changeset).await;
        log_flush_result("changeset_lifecycle", n, res);
    }
    if !batch.verification.is_empty() {
        let n = batch.verification.len();
        let res = flush_verification(client, batch.verification).await;
        log_flush_result("verification_runs", n, res);
    }
    if !batch.review.is_empty() {
        let n = batch.review.len();
        let res = flush_review(client, batch.review).await;
        log_flush_result("review_results", n, res);
    }
}

fn log_flush_result(table: &str, count: usize, res: Result<(), clickhouse::error::Error>) {
    match res {
        Ok(()) => tracing::debug!(
            target: "dk_analytics",
            "flushed {count} row{} to {table}",
            if count == 1 { "" } else { "s" }
        ),
        Err(e) => tracing::error!(
            target: "dk_analytics",
            "insert commit failed for {table} ({count} row{}): {e}",
            if count == 1 { "" } else { "s" }
        ),
    }
}

async fn flush_session(
    client: &AnalyticsClient,
    rows: Vec<SessionEvent>,
) -> Result<(), clickhouse::error::Error> {
    let mut inserter = client
        .inner()
        .insert::<SessionEvent>("session_events")
        .await?;
    for row in &rows {
        inserter.write(row).await?;
    }
    inserter.end().await
}

async fn flush_changeset(
    client: &AnalyticsClient,
    rows: Vec<ChangesetLifecycle>,
) -> Result<(), clickhouse::error::Error> {
    let mut inserter = client
        .inner()
        .insert::<ChangesetLifecycle>("changeset_lifecycle")
        .await?;
    for row in &rows {
        inserter.write(row).await?;
    }
    inserter.end().await
}

async fn flush_verification(
    client: &AnalyticsClient,
    rows: Vec<VerificationRun>,
) -> Result<(), clickhouse::error::Error> {
    let mut inserter = client
        .inner()
        .insert::<VerificationRun>("verification_runs")
        .await?;
    for row in &rows {
        inserter.write(row).await?;
    }
    inserter.end().await
}

async fn flush_review(
    client: &AnalyticsClient,
    rows: Vec<ReviewResult>,
) -> Result<(), clickhouse::error::Error> {
    let mut inserter = client
        .inner()
        .insert::<ReviewResult>("review_results")
        .await?;
    for row in &rows {
        inserter.write(row).await?;
    }
    inserter.end().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_handle_is_cheap() {
        let h = SinkHandle::noop();
        assert!(h.is_noop());
    }

    #[test]
    fn batch_tracks_age_and_counts() {
        let mut b = Batch::default();
        assert!(b.is_empty());
        b.push(AnalyticsEvent::Session(
            crate::test_fixtures::session_event_fixture(),
        ));
        assert_eq!(b.len(), 1);
        assert!(b.first_pushed_at.is_some());
        let taken = b.take();
        assert_eq!(taken.len(), 1);
        assert!(b.is_empty());
    }
}

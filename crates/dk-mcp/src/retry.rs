//! Client-side retry interceptor for gRPC calls.
//!
//! Provides [`RetryPolicy`] for configuring retry behaviour and [`with_retry`]
//! for wrapping an async closure with exponential-backoff retries.
//!
//! ## Motivation
//!
//! During zero-disruption pod scale-down or crashes, gRPC calls fail with
//! `UNAVAILABLE`. When a pod is in graceful-drain mode it returns `ABORTED`.
//! `with_retry` handles both transparently so callers never need to embed
//! retry logic themselves.

use std::time::Duration;
use tonic::Code;

/// Policy that controls how gRPC calls are retried on transient failures.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (not counting the initial call).
    pub max_retries: u32,

    /// Backoff duration for the first retry.
    pub initial_backoff: Duration,

    /// Maximum backoff duration regardless of the multiplier.
    pub max_backoff: Duration,

    /// Multiplier applied to the previous backoff on each successive attempt.
    /// A value of `2.0` doubles the wait time with every retry.
    pub backoff_multiplier: f64,

    /// The set of gRPC status codes that are considered transient and worth retrying.
    pub retryable_codes: Vec<Code>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(50),
            max_backoff: Duration::from_millis(500),
            backoff_multiplier: 2.0,
            retryable_codes: vec![Code::Unavailable, Code::Aborted],
        }
    }
}

impl RetryPolicy {
    /// Returns `true` if `code` is in [`Self::retryable_codes`].
    #[inline]
    pub fn is_retryable(&self, code: Code) -> bool {
        self.retryable_codes.contains(&code)
    }

    /// Returns `true` when more retries are allowed after `attempt` failures.
    ///
    /// `attempt` is zero-based: `0` means we have already seen one failure and
    /// are deciding whether to make the first retry.
    #[inline]
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }

    /// Computes the backoff [`Duration`] before retry number `attempt`.
    ///
    /// Uses the formula `initial_backoff * multiplier^attempt`, capped at
    /// [`Self::max_backoff`].
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        let base_ms = self.initial_backoff.as_millis() as f64;
        let scaled = base_ms * self.backoff_multiplier.powi(attempt as i32);
        let capped = scaled.min(self.max_backoff.as_millis() as f64);
        Duration::from_millis(capped as u64)
    }
}

/// Calls `f` repeatedly, retrying on transient gRPC failures according to `policy`.
///
/// # Behaviour
///
/// 1. Calls `f()`.
/// 2. On success, returns the value immediately.
/// 3. On error, checks whether the status code is retryable and whether there are
///    attempts remaining.  If so, sleeps for the computed backoff duration, logs
///    the retry attempt, and tries again.
/// 4. Returns the last error once retries are exhausted or the error is not retryable.
///
/// # Example
///
/// ```rust,no_run
/// # use dk_mcp::retry::{RetryPolicy, with_retry};
/// # async fn example() {
/// let policy = RetryPolicy::default();
/// let result = with_retry(&policy, || async {
///     // some_grpc_client.call().await
///     Ok::<(), tonic::Status>(())
/// }).await;
/// # }
/// ```
pub async fn with_retry<F, Fut, T>(policy: &RetryPolicy, mut f: F) -> Result<T, tonic::Status>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, tonic::Status>>,
{
    let mut attempt = 0u32;
    loop {
        match f().await {
            Ok(value) => return Ok(value),
            Err(status) => {
                if !policy.is_retryable(status.code()) || !policy.should_retry(attempt) {
                    return Err(status);
                }
                let backoff = policy.backoff_for(attempt);
                tracing::debug!(
                    attempt = attempt + 1,
                    max_retries = policy.max_retries,
                    code = ?status.code(),
                    backoff_ms = backoff.as_millis(),
                    "retrying gRPC call after transient error: {}",
                    status.message(),
                );
                tokio::time::sleep(backoff).await;
                attempt += 1;
            }
        }
    }
}

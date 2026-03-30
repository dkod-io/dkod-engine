//! Tests for the dk-mcp retry interceptor.
//!
//! Validates RetryPolicy configuration, backoff math, and the with_retry
//! async function behavior under retryable and non-retryable gRPC status codes.

use dk_mcp::retry::{with_retry, RetryPolicy};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::time::Duration;
use tonic::Code;

// ===========================================================================
// 1. is_retryable — retryable codes
// ===========================================================================

#[test]
fn retryable_codes_are_retryable() {
    let policy = RetryPolicy::default();
    assert!(
        policy.is_retryable(Code::Unavailable),
        "UNAVAILABLE should be retryable"
    );
    assert!(
        policy.is_retryable(Code::Aborted),
        "ABORTED should be retryable"
    );
}

// ===========================================================================
// 2. is_retryable — non-retryable codes
// ===========================================================================

#[test]
fn non_retryable_codes() {
    let policy = RetryPolicy::default();
    assert!(
        !policy.is_retryable(Code::PermissionDenied),
        "PERMISSION_DENIED should not be retryable"
    );
    assert!(
        !policy.is_retryable(Code::NotFound),
        "NOT_FOUND should not be retryable"
    );
    assert!(
        !policy.is_retryable(Code::InvalidArgument),
        "INVALID_ARGUMENT should not be retryable"
    );
    assert!(!policy.is_retryable(Code::Ok), "OK should not be retryable");
    assert!(
        !policy.is_retryable(Code::DeadlineExceeded),
        "DEADLINE_EXCEEDED should not be retryable"
    );
}

// ===========================================================================
// 3. should_retry — respects max_retries
// ===========================================================================

#[test]
fn max_retries_respected() {
    let policy = RetryPolicy::default(); // max_retries = 3
    assert!(
        policy.should_retry(0),
        "attempt 0 should be allowed (1st retry)"
    );
    assert!(
        policy.should_retry(1),
        "attempt 1 should be allowed (2nd retry)"
    );
    assert!(
        policy.should_retry(2),
        "attempt 2 should be allowed (3rd retry)"
    );
    assert!(
        !policy.should_retry(3),
        "attempt 3 should not be allowed (exhausted)"
    );
}

// ===========================================================================
// 4. backoff_for — exponential growth
// ===========================================================================

#[test]
fn backoff_increases_exponentially() {
    let policy = RetryPolicy::default(); // initial=50ms, multiplier=2.0
    assert_eq!(
        policy.backoff_for(0),
        Duration::from_millis(50),
        "attempt 0 → 50ms"
    );
    assert_eq!(
        policy.backoff_for(1),
        Duration::from_millis(100),
        "attempt 1 → 100ms"
    );
    assert_eq!(
        policy.backoff_for(2),
        Duration::from_millis(200),
        "attempt 2 → 200ms"
    );
}

// ===========================================================================
// 5. backoff_for — cap at max_backoff
// ===========================================================================

#[test]
fn backoff_caps_at_max() {
    let policy = RetryPolicy::default(); // max_backoff = 500ms
                                         // attempt 3 → 50 * 2^3 = 400ms (under cap)
    assert_eq!(policy.backoff_for(3), Duration::from_millis(400));
    // attempt 4 → 50 * 2^4 = 800ms → capped at 500ms
    assert_eq!(policy.backoff_for(4), Duration::from_millis(500));
    // even higher attempts stay at cap
    assert_eq!(policy.backoff_for(10), Duration::from_millis(500));
}

// ===========================================================================
// 6. with_retry — succeeds immediately (no retries)
// ===========================================================================

#[tokio::test]
async fn with_retry_succeeds_immediately() {
    let policy = RetryPolicy::default();
    let call_count = Arc::new(AtomicU32::new(0));
    let cc = call_count.clone();

    let result: Result<&str, tonic::Status> = with_retry(&policy, || {
        let c = cc.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok("hello")
        }
    })
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "hello");
    assert_eq!(call_count.load(Ordering::SeqCst), 1, "called exactly once");
}

// ===========================================================================
// 7. with_retry — retries on retryable code then succeeds
// ===========================================================================

#[tokio::test]
async fn with_retry_retries_then_succeeds() {
    let policy = RetryPolicy {
        max_retries: 3,
        initial_backoff: Duration::from_millis(1), // fast for tests
        max_backoff: Duration::from_millis(10),
        backoff_multiplier: 2.0,
        retryable_codes: vec![Code::Unavailable, Code::Aborted],
    };

    let call_count = Arc::new(AtomicU32::new(0));
    let cc = call_count.clone();

    let result: Result<u32, tonic::Status> = with_retry(&policy, || {
        let c = cc.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                // Fail twice with UNAVAILABLE, then succeed
                Err(tonic::Status::unavailable("pod down"))
            } else {
                Ok(42)
            }
        }
    })
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);
    assert_eq!(call_count.load(Ordering::SeqCst), 3, "initial + 2 retries");
}

// ===========================================================================
// 8. with_retry — stops on non-retryable code
// ===========================================================================

#[tokio::test]
async fn with_retry_stops_on_non_retryable() {
    let policy = RetryPolicy {
        max_retries: 3,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(10),
        backoff_multiplier: 2.0,
        retryable_codes: vec![Code::Unavailable, Code::Aborted],
    };

    let call_count = Arc::new(AtomicU32::new(0));
    let cc = call_count.clone();

    let result: Result<u32, tonic::Status> = with_retry(&policy, || {
        let c = cc.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err(tonic::Status::permission_denied("not allowed"))
        }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), Code::PermissionDenied);
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "should not retry non-retryable errors"
    );
}

// ===========================================================================
// 9. with_retry — exhausts all retries and returns last error
// ===========================================================================

#[tokio::test]
async fn with_retry_exhausts_retries() {
    let policy = RetryPolicy {
        max_retries: 2,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(10),
        backoff_multiplier: 2.0,
        retryable_codes: vec![Code::Unavailable],
    };

    let call_count = Arc::new(AtomicU32::new(0));
    let cc = call_count.clone();

    let result: Result<u32, tonic::Status> = with_retry(&policy, || {
        let c = cc.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err(tonic::Status::unavailable("always down"))
        }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), Code::Unavailable);
    // 1 initial attempt + max_retries(2) = 3 total calls
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        3,
        "should try 1 + max_retries times"
    );
}

// ===========================================================================
// 10. custom RetryPolicy fields
// ===========================================================================

#[test]
fn custom_policy_fields() {
    let policy = RetryPolicy {
        max_retries: 5,
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(1000),
        backoff_multiplier: 3.0,
        retryable_codes: vec![Code::Unavailable],
    };

    assert_eq!(policy.max_retries, 5);
    assert!(policy.should_retry(4));
    assert!(!policy.should_retry(5));

    // 10 * 3^0 = 10ms
    assert_eq!(policy.backoff_for(0), Duration::from_millis(10));
    // 10 * 3^1 = 30ms
    assert_eq!(policy.backoff_for(1), Duration::from_millis(30));
    // 10 * 3^2 = 90ms
    assert_eq!(policy.backoff_for(2), Duration::from_millis(90));

    assert!(policy.is_retryable(Code::Unavailable));
    assert!(!policy.is_retryable(Code::Aborted)); // not in custom list
}

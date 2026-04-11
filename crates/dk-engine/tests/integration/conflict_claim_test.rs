//! Integration tests for the [`ClaimTracker`] trait.
//!
//! These tests exercise the `acquire_lock` / `release_lock` exclusive-locking
//! semantics through the trait interface, complementing the `record_claim` /
//! `check_conflicts` unit tests in `claim_tracker.rs`.
//!
//! We use `LocalClaimTracker` here because it needs no external services.
//! Valkey-specific tests (Lua script atomicity, TTL behaviour, session-index
//! cleanup) should be added behind `#[cfg(feature = "valkey")]` with a live
//! Valkey instance (e.g. via `testcontainers`).

use chrono::Utc;
use dk_core::SymbolKind;
use dk_engine::conflict::{AcquireOutcome, ClaimTracker, LocalClaimTracker, SymbolClaim};
use uuid::Uuid;

fn make_claim(session_id: Uuid, agent: &str, name: &str, kind: SymbolKind) -> SymbolClaim {
    SymbolClaim {
        session_id,
        agent_name: agent.to_string(),
        qualified_name: name.to_string(),
        kind,
        first_touched_at: Utc::now(),
    }
}

/// Helper that exercises `acquire_lock` through the trait object, ensuring
/// the tests validate the trait contract rather than a concrete type.
fn tracker() -> Box<dyn ClaimTracker> {
    Box::new(LocalClaimTracker::new())
}

#[tokio::test]
async fn acquire_fresh_lock_returns_fresh() {
    let t = tracker();
    let repo = Uuid::new_v4();
    let session = Uuid::new_v4();

    let outcome = t
        .acquire_lock(
            repo,
            "src/lib.rs",
            make_claim(session, "agent-1", "fn_main", SymbolKind::Function),
        )
        .await
        .expect("fresh lock should succeed");
    assert_eq!(outcome, AcquireOutcome::Fresh);
}

#[tokio::test]
async fn reacquire_same_session_returns_reacquired() {
    let t = tracker();
    let repo = Uuid::new_v4();
    let session = Uuid::new_v4();

    t.acquire_lock(
        repo,
        "src/lib.rs",
        make_claim(session, "agent-1", "fn_main", SymbolKind::Function),
    )
    .await
    .unwrap();

    let outcome = t
        .acquire_lock(
            repo,
            "src/lib.rs",
            make_claim(session, "agent-1", "fn_main", SymbolKind::Function),
        )
        .await
        .expect("same session reacquire should succeed");
    assert_eq!(outcome, AcquireOutcome::ReAcquired);
}

#[tokio::test]
async fn cross_session_acquire_is_blocked() {
    let t = tracker();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    t.acquire_lock(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_main", SymbolKind::Function),
    )
    .await
    .unwrap();

    let err = t
        .acquire_lock(
            repo,
            "src/lib.rs",
            make_claim(session_b, "agent-2", "fn_main", SymbolKind::Function),
        )
        .await
        .expect_err("cross-session acquire should be blocked");

    assert_eq!(err.qualified_name, "fn_main");
    assert_eq!(err.locked_by_session, session_a);
    assert_eq!(err.locked_by_agent, "agent-1");
}

#[tokio::test]
async fn release_lock_unblocks_other_session() {
    let t = tracker();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    t.acquire_lock(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_main", SymbolKind::Function),
    )
    .await
    .unwrap();

    // Release session A's lock
    t.release_lock(repo, "src/lib.rs", session_a, "fn_main")
        .await;

    // Session B should now succeed
    let outcome = t
        .acquire_lock(
            repo,
            "src/lib.rs",
            make_claim(session_b, "agent-2", "fn_main", SymbolKind::Function),
        )
        .await
        .expect("should succeed after release");
    assert_eq!(outcome, AcquireOutcome::Fresh);
}

#[tokio::test]
async fn release_locks_returns_all_released_and_unblocks() {
    let t = tracker();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    // Session A locks two symbols across two files
    t.acquire_lock(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    )
    .await
    .unwrap();
    t.acquire_lock(
        repo,
        "src/api.rs",
        make_claim(session_a, "agent-1", "handler", SymbolKind::Function),
    )
    .await
    .unwrap();

    // Session B is blocked on fn_a
    assert!(t
        .acquire_lock(
            repo,
            "src/lib.rs",
            make_claim(session_b, "agent-2", "fn_a", SymbolKind::Function),
        )
        .await
        .is_err());

    // Release all of session A's locks
    let released = t.release_locks(repo, session_a).await;
    assert_eq!(released.len(), 2);

    let names: Vec<&str> = released.iter().map(|r| r.qualified_name.as_str()).collect();
    assert!(names.contains(&"fn_a"));
    assert!(names.contains(&"handler"));

    // Session B should now succeed
    assert!(t
        .acquire_lock(
            repo,
            "src/lib.rs",
            make_claim(session_b, "agent-2", "fn_a", SymbolKind::Function),
        )
        .await
        .is_ok());
}

#[tokio::test]
async fn clear_session_releases_all_repos() {
    let t = tracker();
    let repo_1 = Uuid::new_v4();
    let repo_2 = Uuid::new_v4();
    let session = Uuid::new_v4();
    let other = Uuid::new_v4();

    t.acquire_lock(
        repo_1,
        "src/lib.rs",
        make_claim(session, "agent-1", "fn_a", SymbolKind::Function),
    )
    .await
    .unwrap();
    t.acquire_lock(
        repo_2,
        "src/main.rs",
        make_claim(session, "agent-1", "main", SymbolKind::Function),
    )
    .await
    .unwrap();

    // Both are blocked for other session
    assert!(t
        .acquire_lock(
            repo_1,
            "src/lib.rs",
            make_claim(other, "agent-2", "fn_a", SymbolKind::Function),
        )
        .await
        .is_err());

    // Clear entire session
    let released = t.clear_session(session).await;
    assert_eq!(released.len(), 2);

    // Both repos should now be unblocked
    assert!(t
        .acquire_lock(
            repo_1,
            "src/lib.rs",
            make_claim(other, "agent-2", "fn_a", SymbolKind::Function),
        )
        .await
        .is_ok());
    assert!(t
        .acquire_lock(
            repo_2,
            "src/main.rs",
            make_claim(other, "agent-2", "main", SymbolKind::Function),
        )
        .await
        .is_ok());
}

#[tokio::test]
async fn different_symbols_same_file_no_conflict() {
    let t = tracker();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    t.acquire_lock(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    )
    .await
    .unwrap();

    // Different symbol in same file should succeed
    let outcome = t
        .acquire_lock(
            repo,
            "src/lib.rs",
            make_claim(session_b, "agent-2", "fn_b", SymbolKind::Function),
        )
        .await
        .expect("different symbols should not conflict");
    assert_eq!(outcome, AcquireOutcome::Fresh);
}

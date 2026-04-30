//! Epic B integration tests — eviction + resume.
//!
//! Requires a live PostgreSQL database (DATABASE_URL env var).
//! Run with:
//!   DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test cargo test -p dk-engine

use dk_engine::changeset::ChangesetState;
use dk_engine::conflict::{ClaimTracker, LocalClaimTracker, SymbolClaim};
use dk_engine::workspace::session_manager::{
    AbandonReason, ResumeResult, StrandReason, WorkspaceManager,
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

// ── Test helper ───────────────────────────────────────────────────────

/// Insert a session_workspaces row + matching changesets row.
/// Returns the session_id UUID.
///
/// Mirrors the same helper in `session_gc_test.rs`. Duplicated here to keep
/// each test module self-contained and avoid a shared helper module.
async fn insert_workspace_with_changeset(pool: &PgPool, state: ChangesetState) -> Uuid {
    let session_id = Uuid::new_v4();
    let changeset_id = Uuid::new_v4();
    let repo_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO repositories (id, name, path, created_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT DO NOTHING",
    )
    .bind(repo_id)
    .bind(format!("test-repo-{}", session_id))
    .bind(format!("/tmp/repo-{}", session_id))
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO changesets (id, repo_id, number, state)
         VALUES ($1, $2, 1, $3)",
    )
    .bind(changeset_id)
    .bind(repo_id)
    .bind(state.as_str())
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO session_workspaces (session_id, repo_id, agent_id, changeset_id,
                                         base_commit_hash, intent)
         VALUES ($1, $2, 'agent-test', $3, 'initial', 'test')",
    )
    .bind(session_id)
    .bind(repo_id)
    .bind(changeset_id)
    .execute(pool)
    .await
    .unwrap();

    session_id
}

// ── Tests ─────────────────────────────────────────────────────────────

#[sqlx::test]
async fn resume_happy_path_rehydrates(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool.clone());
    let dead = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;

    // Strand the workspace so it's eligible for resume.
    mgr.strand(&dead, StrandReason::IdleTtl).await.unwrap();

    let new_session = Uuid::new_v4();
    let result = mgr.resume(&dead, new_session, "agent-test").await.unwrap();
    assert!(
        matches!(result, ResumeResult::Ok(_)),
        "expected ResumeResult::Ok, got {result:?}"
    );

    // Verify DB row was rotated: session_id == new_session, stranded_at cleared,
    // superseded_by set to new_session.
    let (stranded_at, superseded_by, session_id): (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<Uuid>,
        Uuid,
    ) = sqlx::query_as(
        "SELECT stranded_at, superseded_by, session_id
           FROM session_workspaces
          WHERE session_id = $1",
    )
    .bind(new_session)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        stranded_at.is_none(),
        "stranded_at must be cleared after resume"
    );
    assert_eq!(
        superseded_by,
        Some(new_session),
        "superseded_by must be set to new_session"
    );
    assert_eq!(
        session_id, new_session,
        "session_id must be rotated to new_session"
    );

    // Verify the workspace is in the in-memory map under new_session.
    assert!(
        mgr.get_workspace(&new_session).is_some(),
        "resumed workspace must be present in the active map"
    );
}

#[sqlx::test]
async fn resume_after_abandon_returns_abandoned(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool.clone());
    let dead = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    mgr.strand(&dead, StrandReason::IdleTtl).await.unwrap();
    mgr.abandon_stranded(&dead, AbandonReason::AutoTtl)
        .await
        .unwrap();

    let new_session = Uuid::new_v4();
    let result = mgr.resume(&dead, new_session, "agent-test").await.unwrap();
    assert!(
        matches!(result, ResumeResult::Abandoned),
        "expected ResumeResult::Abandoned, got {result:?}"
    );
}

#[sqlx::test]
async fn resume_wrong_agent_returns_error(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool.clone());
    let dead = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    mgr.strand(&dead, StrandReason::IdleTtl).await.unwrap();

    let new_session = Uuid::new_v4();
    let err = mgr
        .resume(&dead, new_session, "different-agent")
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("agent_id"),
        "error must mention agent_id, got: {err}"
    );
}

#[sqlx::test]
async fn resume_not_stranded_returns_not_stranded(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool.clone());
    // Insert workspace that is NOT stranded (stranded_at IS NULL).
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;

    let new_session = Uuid::new_v4();
    let result = mgr
        .resume(&session_id, new_session, "agent-test")
        .await
        .unwrap();
    assert!(
        matches!(result, ResumeResult::NotStranded),
        "expected ResumeResult::NotStranded, got {result:?}"
    );
}

#[sqlx::test]
async fn resume_terminal_changeset_returns_abandoned(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool.clone());
    // Workspace with a terminal changeset state.
    let dead = insert_workspace_with_changeset(&pool, ChangesetState::Merged).await;
    // Manually set stranded_at so it passes the stranded check and hits the
    // terminal-changeset guard.
    sqlx::query("UPDATE session_workspaces SET stranded_at = now() WHERE session_id = $1")
        .bind(dead)
        .execute(&pool)
        .await
        .unwrap();

    let new_session = Uuid::new_v4();
    let result = mgr.resume(&dead, new_session, "agent-test").await.unwrap();
    assert!(
        matches!(result, ResumeResult::Abandoned),
        "expected ResumeResult::Abandoned for terminal changeset, got {result:?}"
    );
}

/// Scenario: workspace A is stranded while session B holds a lock on the same
/// symbol. Resuming A should detect contention and return Contended, leaving
/// A re-stranded rather than partially live.
///
/// This test requires the symbol to already be in A's graph (written to the
/// overlay + parsed before strand) AND for session B to hold the lock in the
/// shared ClaimTracker. Because the overlay is empty at DB-level in this test
/// setup (no session_overlay_files rows inserted), the Contended path cannot
/// be triggered via `reindex_from_overlay` alone. The test is therefore marked
/// `#[ignore]` and left as a placeholder for when a helper exists that writes
/// overlay rows to the DB.
///
/// To run when a DB-overlay helper is available:
///   DATABASE_URL=... cargo test -p dk-engine resume_contended_stays_stranded -- --ignored
#[sqlx::test]
#[ignore]
async fn resume_contended_stays_stranded(pool: PgPool) {
    use dk_core::SymbolKind;

    let tracker = Arc::new(LocalClaimTracker::new());
    let mgr = WorkspaceManager::with_deps(
        pool.clone(),
        Arc::new(dk_engine::workspace::cache::NoOpCache),
        tracker.clone(),
        Arc::new(dk_engine::workspace::session_manager::NoOpEventPublisher),
    );

    // 1. Insert workspace A with a non-terminal changeset.
    let dead_session = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    let repo_id: Uuid =
        sqlx::query_scalar("SELECT repo_id FROM session_workspaces WHERE session_id = $1")
            .bind(dead_session)
            .fetch_one(&pool)
            .await
            .unwrap();

    // 2. Strand A.
    mgr.strand(&dead_session, StrandReason::IdleTtl)
        .await
        .unwrap();

    // 3. Session B acquires the lock for the same symbol.
    let session_b = Uuid::new_v4();
    tracker
        .acquire_lock(
            repo_id,
            "src/lib.rs",
            SymbolClaim {
                session_id: session_b,
                agent_name: "agent-b".to_string(),
                qualified_name: "conflict_fn".to_string(),
                kind: SymbolKind::Function,
                first_touched_at: chrono::Utc::now(),
            },
        )
        .await
        .unwrap();

    // TODO: insert overlay rows for `src/lib.rs` in the DB for workspace A
    // so that reindex_from_overlay picks up `conflict_fn` and tries to
    // re-acquire — that triggers the Contended path.

    // 4. Resume A — with overlay rows present this should return Contended.
    let new_session = Uuid::new_v4();
    let result = mgr
        .resume(&dead_session, new_session, "agent-test")
        .await
        .unwrap();
    assert!(
        matches!(result, ResumeResult::Contended(_)),
        "expected ResumeResult::Contended, got {result:?}"
    );

    // 5. The original dead_session row should still be stranded after the failed resume.
    // The new_session was never fully committed; we're verifying the original stranded row
    // stayed stranded (strand() is idempotent and re-strand on contention is a no-op here).
    let stranded_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT stranded_at FROM session_workspaces WHERE session_id = $1")
            .bind(dead_session)
            .fetch_optional(&pool)
            .await
            .unwrap()
            .flatten();
    assert!(
        stranded_at.is_some(),
        "A should remain stranded after contended resume"
    );
}

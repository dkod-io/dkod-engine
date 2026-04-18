//! Tests for WorkspaceManager::gc_expired_sessions — activity-based session GC.

use std::time::Duration;

use dk_engine::workspace::session_manager::WorkspaceManager;
use dk_engine::workspace::session_workspace::{SessionWorkspace, WorkspaceMode};
use sqlx::PgPool;
use uuid::Uuid;

/// Helper: create a WorkspaceManager with a lazy (non-connecting) PgPool.
fn test_manager() -> WorkspaceManager {
    let db = PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();
    WorkspaceManager::new(db)
}

/// Helper: create an in-memory test workspace.
fn make_workspace(session_id: Uuid) -> SessionWorkspace {
    SessionWorkspace::new_test(
        session_id,
        Uuid::new_v4(),
        "test-agent".to_string(),
        "test intent".to_string(),
        "abc123".to_string(),
        WorkspaceMode::Ephemeral,
    )
}

#[tokio::test(start_paused = true)]
async fn test_gc_expires_idle_sessions() {
    let mgr = test_manager();
    let session_id = Uuid::new_v4();

    // Insert a workspace — created "now" (paused time = 0).
    let ws = make_workspace(session_id);
    mgr.insert_test_workspace(ws);
    assert_eq!(mgr.total_active(), 1);

    // Advance time by 31 minutes so the session is idle beyond the 30-min TTL.
    tokio::time::advance(Duration::from_secs(31 * 60)).await;

    let expired = mgr.gc_expired_sessions(
        Duration::from_secs(30 * 60),  // idle_ttl = 30 min
        Duration::from_secs(4 * 3600), // max_ttl = 4 hours
    );

    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0], session_id);
    assert_eq!(mgr.total_active(), 0, "workspace should be removed from map");
}

#[tokio::test(start_paused = true)]
async fn test_gc_preserves_active_sessions() {
    let mgr = test_manager();
    let session_id = Uuid::new_v4();

    let ws = make_workspace(session_id);
    mgr.insert_test_workspace(ws);

    // Advance only 5 minutes — well within idle_ttl.
    tokio::time::advance(Duration::from_secs(5 * 60)).await;

    let expired = mgr.gc_expired_sessions(
        Duration::from_secs(30 * 60),
        Duration::from_secs(4 * 3600),
    );

    assert!(expired.is_empty(), "active session should not be expired");
    assert_eq!(mgr.total_active(), 1, "workspace should remain in map");
}

#[tokio::test(start_paused = true)]
async fn test_gc_hard_max_ttl() {
    let mgr = test_manager();
    let session_id = Uuid::new_v4();

    mgr.insert_test_workspace(make_workspace(session_id));

    // Advance 4 hours + 1 second — beyond max_ttl.
    tokio::time::advance(Duration::from_secs(4 * 3600 + 1)).await;

    // Touch the session so last_active is recent (simulate continuous activity).
    if let Some(mut ws_ref) = mgr.get_workspace_mut(&session_id) {
        ws_ref.touch();
    }

    let expired = mgr.gc_expired_sessions(
        Duration::from_secs(30 * 60),  // idle_ttl = 30 min
        Duration::from_secs(4 * 3600), // max_ttl = 4 hours
    );

    assert_eq!(expired.len(), 1, "session should expire due to max_ttl despite recent activity");
    assert_eq!(expired[0], session_id);
    assert_eq!(mgr.total_active(), 0);
}

#[tokio::test(start_paused = true)]
async fn test_gc_mixed_sessions() {
    let mgr = test_manager();
    let idle_id = Uuid::new_v4();
    let active_id = Uuid::new_v4();

    // Insert idle_id at time 0.
    mgr.insert_test_workspace(make_workspace(idle_id));

    // Advance 20 minutes, then insert active_id (it's newer).
    tokio::time::advance(Duration::from_secs(20 * 60)).await;
    mgr.insert_test_workspace(make_workspace(active_id));

    // Advance another 15 minutes (total: 35 min).
    // idle_id: last_active 35 min ago → expired (idle_ttl = 30 min).
    // active_id: last_active 15 min ago → still alive.
    tokio::time::advance(Duration::from_secs(15 * 60)).await;

    // Touch active_id to simulate activity.
    if let Some(mut ws) = mgr.get_workspace_mut(&active_id) {
        ws.touch();
    }

    let expired = mgr.gc_expired_sessions(
        Duration::from_secs(30 * 60),
        Duration::from_secs(4 * 3600),
    );

    assert_eq!(expired.len(), 1, "only idle session should expire");
    assert_eq!(expired[0], idle_id);
    assert_eq!(mgr.total_active(), 1, "active session should remain");
    assert!(mgr.get_workspace(&active_id).is_some());
}

// ── should_pin integration tests (require DATABASE_URL / sqlx::test) ──

#[sqlx::test]
async fn should_pin_returns_true_for_non_terminal_states(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    assert!(mgr.should_pin(&session_id).await);
}

#[sqlx::test]
async fn should_pin_returns_false_for_terminal_states(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    for state in [ChangesetState::Merged, ChangesetState::Rejected, ChangesetState::Closed] {
        let session_id = insert_workspace_with_changeset(&pool, state).await;
        assert!(!mgr.should_pin(&session_id).await, "state {state:?} should not pin");
    }
}

#[sqlx::test]
async fn should_pin_returns_false_when_session_has_no_changeset(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool);
    let missing = Uuid::new_v4();
    assert!(!mgr.should_pin(&missing).await);
}

#[sqlx::test]
async fn strand_sets_stranded_at_and_is_idempotent(pool: PgPool) {
    use dk_engine::workspace::session_manager::StrandReason;
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;

    mgr.strand(&session_id, StrandReason::IdleTtl).await.unwrap();

    let (stranded_at, reason): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) = sqlx::query_as(
        "SELECT stranded_at, stranded_reason FROM session_workspaces WHERE session_id = $1",
    )
    .bind(session_id).fetch_one(&pool).await.unwrap();
    assert!(stranded_at.is_some());
    assert_eq!(reason.as_deref(), Some("idle_ttl"));

    let first_ts = stranded_at.unwrap();
    mgr.strand(&session_id, StrandReason::IdleTtl).await.unwrap();
    let (stranded_at2, _): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("SELECT stranded_at, stranded_reason FROM session_workspaces WHERE session_id = $1")
            .bind(session_id).fetch_one(&pool).await.unwrap();
    assert_eq!(stranded_at2, Some(first_ts));
}

/// Test helper. Inserts a session_workspaces row + matching changesets row, returns session_id.
async fn insert_workspace_with_changeset(pool: &PgPool, state: dk_engine::changeset::ChangesetState) -> Uuid {
    let session_id = Uuid::new_v4();
    let changeset_id = Uuid::new_v4();
    let repo_id = Uuid::new_v4();
    // Insert a repo row first (repositories is referenced by changesets and session_workspaces).
    sqlx::query(
        "INSERT INTO repositories (id, name, path, created_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT DO NOTHING",
    )
    .bind(repo_id)
    .bind(format!("test-repo-{}", session_id))
    .bind(format!("/tmp/repo-{}", session_id))
    .execute(pool).await.unwrap();
    // Insert the changeset (required by FK if session_workspaces.changeset_id references it).
    sqlx::query(
        "INSERT INTO changesets (id, repo_id, number, state)
         VALUES ($1, $2, 1, $3)",
    )
    .bind(changeset_id).bind(repo_id).bind(state.as_str())
    .execute(pool).await.unwrap();
    // Insert the session workspace pointing at the changeset.
    sqlx::query(
        "INSERT INTO session_workspaces (session_id, repo_id, agent_id, changeset_id,
                                         base_commit_hash, intent)
         VALUES ($1, $2, 'agent-test', $3, 'initial', 'test')",
    )
    .bind(session_id).bind(repo_id).bind(changeset_id)
    .execute(pool).await.unwrap();
    session_id
}

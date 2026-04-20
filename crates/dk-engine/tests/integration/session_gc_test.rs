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
    assert_eq!(
        mgr.total_active(),
        0,
        "workspace should be removed from map"
    );
}

#[tokio::test(start_paused = true)]
async fn test_gc_preserves_active_sessions() {
    let mgr = test_manager();
    let session_id = Uuid::new_v4();

    let ws = make_workspace(session_id);
    mgr.insert_test_workspace(ws);

    // Advance only 5 minutes — well within idle_ttl.
    tokio::time::advance(Duration::from_secs(5 * 60)).await;

    let expired =
        mgr.gc_expired_sessions(Duration::from_secs(30 * 60), Duration::from_secs(4 * 3600));

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

    assert_eq!(
        expired.len(),
        1,
        "session should expire due to max_ttl despite recent activity"
    );
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

    let expired =
        mgr.gc_expired_sessions(Duration::from_secs(30 * 60), Duration::from_secs(4 * 3600));

    assert_eq!(expired.len(), 1, "only idle session should expire");
    assert_eq!(expired[0], idle_id);
    assert_eq!(mgr.total_active(), 1, "active session should remain");
    assert!(mgr.get_workspace(&active_id).is_some());
}

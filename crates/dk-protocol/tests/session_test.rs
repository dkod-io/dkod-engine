use dk_protocol::session::{SessionManager, SessionSnapshot};
use std::time::Duration;

#[test]
fn test_create_and_get_session() {
    let mgr = SessionManager::new(Duration::from_secs(60));
    let sid = mgr.create_session(
        "claude-v3".into(),
        "org/repo".into(),
        "Refactor auth".into(),
        "abc123".into(),
    );

    let session = mgr.get_session(&sid).unwrap();
    assert_eq!(session.agent_id, "claude-v3");
    assert_eq!(session.codebase, "org/repo");
    assert_eq!(session.intent, "Refactor auth");
    assert_eq!(session.codebase_version, "abc123");
}

#[test]
fn test_session_not_found() {
    let mgr = SessionManager::new(Duration::from_secs(60));
    let fake_id = uuid::Uuid::new_v4();
    assert!(mgr.get_session(&fake_id).is_none());
}

#[test]
fn test_touch_session() {
    let mgr = SessionManager::new(Duration::from_secs(60));
    let sid = mgr.create_session("agent".into(), "repo".into(), "test".into(), "v1".into());
    assert!(mgr.touch_session(&sid));
    assert!(!mgr.touch_session(&uuid::Uuid::new_v4()));
}

#[test]
fn test_remove_session() {
    let mgr = SessionManager::new(Duration::from_secs(60));
    let sid = mgr.create_session("agent".into(), "repo".into(), "test".into(), "v1".into());
    assert!(mgr.remove_session(&sid));
    assert!(mgr.get_session(&sid).is_none());
}

#[test]
fn test_expired_session() {
    let mgr = SessionManager::new(Duration::from_millis(1));
    let sid = mgr.create_session("agent".into(), "repo".into(), "test".into(), "v1".into());
    std::thread::sleep(Duration::from_millis(10));
    assert!(mgr.get_session(&sid).is_none());
}

#[test]
fn test_save_and_take_snapshot() {
    let mgr = SessionManager::new(Duration::from_secs(60));
    let sid = mgr.create_session("agent".into(), "repo".into(), "test".into(), "v1".into());

    mgr.save_snapshot(&sid, SessionSnapshot {
        agent_id: "agent".into(),
        codebase: "repo".into(),
        intent: "test".into(),
        codebase_version: "v1".into(),
    });

    let snap = mgr.take_snapshot(&sid).unwrap();
    assert_eq!(snap.agent_id, "agent");
    assert_eq!(snap.codebase, "repo");

    // Second take returns None (consumed)
    assert!(mgr.take_snapshot(&sid).is_none());
}

#[test]
fn test_cleanup_expired_saves_snapshots() {
    let mgr = SessionManager::new(Duration::from_millis(1));
    let sid = mgr.create_session("agent".into(), "repo".into(), "test".into(), "v1".into());
    std::thread::sleep(Duration::from_millis(10));

    mgr.cleanup_expired();

    // Session should be gone
    assert!(mgr.get_session(&sid).is_none());

    // But a snapshot should have been saved
    let snap = mgr.take_snapshot(&sid).unwrap();
    assert_eq!(snap.agent_id, "agent");
    assert_eq!(snap.codebase, "repo");
    assert_eq!(snap.intent, "test");
    assert_eq!(snap.codebase_version, "v1");
}

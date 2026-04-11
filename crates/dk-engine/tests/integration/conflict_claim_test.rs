use chrono::Utc;
use dk_core::SymbolKind;
use dk_engine::conflict::{SymbolClaim, SymbolClaimTracker};
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

#[test]
fn test_no_conflict_different_symbols_same_file() {
    let tracker = SymbolClaimTracker::new();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    tracker.record_claim(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    );

    let conflicts =
        tracker.check_conflicts(repo, "src/lib.rs", session_b, &["fn_b".to_string()]);
    assert!(conflicts.is_empty(), "different symbols should not conflict");
}

#[test]
fn test_conflict_same_symbol() {
    let tracker = SymbolClaimTracker::new();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    tracker.record_claim(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    );

    let conflicts =
        tracker.check_conflicts(repo, "src/lib.rs", session_b, &["fn_a".to_string()]);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].qualified_name, "fn_a");
    assert_eq!(conflicts[0].conflicting_session, session_a);
    assert_eq!(conflicts[0].conflicting_agent, "agent-1");
}

#[test]
fn test_claims_cleared_on_session_destroy() {
    let tracker = SymbolClaimTracker::new();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    tracker.record_claim(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    );

    tracker.clear_session(session_a);

    let conflicts =
        tracker.check_conflicts(repo, "src/lib.rs", session_b, &["fn_a".to_string()]);
    assert!(
        conflicts.is_empty(),
        "cleared session should not cause conflicts"
    );
}

#[test]
fn test_same_session_no_self_conflict() {
    let tracker = SymbolClaimTracker::new();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();

    tracker.record_claim(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    );
    // Re-write same symbol from same session
    tracker.record_claim(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    );

    let conflicts =
        tracker.check_conflicts(repo, "src/lib.rs", session_a, &["fn_a".to_string()]);
    assert!(
        conflicts.is_empty(),
        "same session should not conflict with itself"
    );
}

#[test]
fn test_multiple_conflicts() {
    let tracker = SymbolClaimTracker::new();
    let repo = Uuid::new_v4();
    let session_a = Uuid::new_v4();
    let session_b = Uuid::new_v4();

    tracker.record_claim(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
    );
    tracker.record_claim(
        repo,
        "src/lib.rs",
        make_claim(session_a, "agent-1", "fn_b", SymbolKind::Function),
    );

    let conflicts = tracker.check_conflicts(
        repo,
        "src/lib.rs",
        session_b,
        &["fn_a".to_string(), "fn_b".to_string()],
    );
    assert_eq!(conflicts.len(), 2);

    let names: Vec<&str> = conflicts.iter().map(|c| c.qualified_name.as_str()).collect();
    assert!(names.contains(&"fn_a"));
    assert!(names.contains(&"fn_b"));
}

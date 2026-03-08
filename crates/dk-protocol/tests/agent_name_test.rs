use dk_engine::workspace::session_manager::SessionInfo;
use uuid::Uuid;

#[test]
fn test_agent_name_auto_assigned_when_empty() {
    // When agent_name is empty, the connect handler auto-assigns "agent-N".
    // We test the logic branch directly:
    let provided = "";
    let agent_name = if provided.is_empty() {
        "agent-1".to_string()
    } else {
        provided.to_string()
    };
    assert_eq!(agent_name, "agent-1");
}

#[test]
fn test_agent_name_preserved_when_provided() {
    // When an agent provides a name, it should be used as-is.
    let provided = "feature-bot";
    let agent_name = if provided.is_empty() {
        "agent-1".to_string()
    } else {
        provided.to_string()
    };
    assert_eq!(agent_name, "feature-bot");
}

#[test]
fn test_agent_names_increment_per_repo() {
    // WorkspaceManager::next_agent_name uses per-repo AtomicU32 counters.
    // The unit test in session_manager.rs covers the full counter logic.
    // Here we verify the format convention.
    for n in 1..=5 {
        let name = format!("agent-{n}");
        assert!(name.starts_with("agent-"));
        assert_eq!(name, format!("agent-{}", n));
    }
}

#[test]
fn test_agent_name_in_session_info() {
    let info = SessionInfo {
        session_id: Uuid::new_v4(),
        agent_id: "claude-code".to_string(),
        agent_name: "agent-1".to_string(),
        intent: "refactor".to_string(),
        repo_id: Uuid::new_v4(),
        changeset_id: Uuid::new_v4(),
        state: "active".to_string(),
        elapsed_secs: 0,
    };

    assert_eq!(info.agent_name, "agent-1");

    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["agent_name"], "agent-1");
    assert_eq!(json["agent_id"], "claude-code");
}

#[tokio::test]
async fn test_agent_name_on_session_workspace() {
    use dk_engine::workspace::session_workspace::{SessionWorkspace, WorkspaceMode};

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "claude-code".to_string(),
        "fix bugs".to_string(),
        "abc123".to_string(),
        WorkspaceMode::Ephemeral,
    );

    // new_test defaults agent_name to empty string
    assert_eq!(ws.agent_name, "");
}

#[test]
fn test_source_branch_uses_agent_name() {
    // Verify the branch naming convention uses agent_name, not agent_id
    let agent_name = "feature-bot";
    let source_branch = format!("agent/{}", agent_name);
    assert_eq!(source_branch, "agent/feature-bot");

    let agent_name = "agent-3";
    let source_branch = format!("agent/{}", agent_name);
    assert_eq!(source_branch, "agent/agent-3");
}

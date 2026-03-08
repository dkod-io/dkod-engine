use dk_engine::changeset::{Changeset, ChangesetState};

/// Helper to build a Changeset in a given state.
fn changeset_in_state(state: &str) -> Changeset {
    use chrono::Utc;
    use uuid::Uuid;

    Changeset {
        id: Uuid::new_v4(),
        repo_id: Uuid::new_v4(),
        number: 1,
        title: "test".to_string(),
        intent_summary: None,
        source_branch: "agent/test".to_string(),
        target_branch: "main".to_string(),
        state: state.to_string(),
        reason: String::new(),
        session_id: None,
        agent_id: None,
        agent_name: None,
        author_id: None,
        base_version: None,
        merged_version: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        merged_at: None,
    }
}

// ── ChangesetState enum tests ───────────────────────────────────

#[test]
fn state_from_str_round_trips() {
    let states = [
        "draft",
        "submitted",
        "verifying",
        "approved",
        "rejected",
        "merged",
        "closed",
    ];
    for s in &states {
        let parsed = ChangesetState::parse(s).unwrap_or_else(|| panic!("failed to parse '{s}'"));
        assert_eq!(parsed.as_str(), *s);
    }
}

#[test]
fn state_from_str_rejects_unknown() {
    assert!(ChangesetState::parse("open").is_none());
    assert!(ChangesetState::parse("").is_none());
    assert!(ChangesetState::parse("DRAFT").is_none());
}

#[test]
fn state_display_matches_as_str() {
    let state = ChangesetState::Draft;
    assert_eq!(format!("{state}"), state.as_str());
}

// ── Valid transitions ───────────────────────────────────────────

#[test]
fn valid_transition_draft_to_submitted() {
    assert!(ChangesetState::Draft.can_transition_to(ChangesetState::Submitted));
}

#[test]
fn valid_transition_submitted_to_verifying() {
    assert!(ChangesetState::Submitted.can_transition_to(ChangesetState::Verifying));
}

#[test]
fn valid_transition_verifying_to_approved() {
    assert!(ChangesetState::Verifying.can_transition_to(ChangesetState::Approved));
}

#[test]
fn valid_transition_verifying_to_rejected() {
    assert!(ChangesetState::Verifying.can_transition_to(ChangesetState::Rejected));
}

#[test]
fn valid_transition_approved_to_merged() {
    assert!(ChangesetState::Approved.can_transition_to(ChangesetState::Merged));
}

#[test]
fn valid_transition_any_state_to_closed() {
    let all = [
        ChangesetState::Draft,
        ChangesetState::Submitted,
        ChangesetState::Verifying,
        ChangesetState::Approved,
        ChangesetState::Rejected,
        ChangesetState::Merged,
        ChangesetState::Closed,
    ];
    for state in &all {
        assert!(
            state.can_transition_to(ChangesetState::Closed),
            "{state} -> closed should be valid"
        );
    }
}

// ── Invalid transitions ─────────────────────────────────────────

#[test]
fn invalid_transition_draft_to_approved() {
    assert!(!ChangesetState::Draft.can_transition_to(ChangesetState::Approved));
}

#[test]
fn invalid_transition_draft_to_merged() {
    assert!(!ChangesetState::Draft.can_transition_to(ChangesetState::Merged));
}

#[test]
fn invalid_transition_submitted_to_approved() {
    assert!(!ChangesetState::Submitted.can_transition_to(ChangesetState::Approved));
}

#[test]
fn invalid_transition_submitted_to_merged() {
    assert!(!ChangesetState::Submitted.can_transition_to(ChangesetState::Merged));
}

#[test]
fn invalid_transition_approved_to_submitted() {
    assert!(!ChangesetState::Approved.can_transition_to(ChangesetState::Submitted));
}

#[test]
fn invalid_transition_rejected_to_merged() {
    assert!(!ChangesetState::Rejected.can_transition_to(ChangesetState::Merged));
}

#[test]
fn invalid_transition_merged_to_draft() {
    assert!(!ChangesetState::Merged.can_transition_to(ChangesetState::Draft));
}

// ── Changeset.transition() method ───────────────────────────────

#[test]
fn transition_method_succeeds_for_valid_transition() {
    let mut cs = changeset_in_state("draft");
    let result = cs.transition(ChangesetState::Submitted, "agent submitted code");
    assert!(result.is_ok());
    assert_eq!(cs.state, "submitted");
    assert_eq!(cs.reason, "agent submitted code");
}

#[test]
fn transition_method_rejects_invalid_transition() {
    let mut cs = changeset_in_state("draft");
    let result = cs.transition(ChangesetState::Merged, "should not work");
    assert!(result.is_err());
    // State should remain unchanged
    assert_eq!(cs.state, "draft");
}

#[test]
fn transition_records_reason_on_each_step() {
    let mut cs = changeset_in_state("draft");

    cs.transition(ChangesetState::Submitted, "code ready for review")
        .unwrap();
    assert_eq!(cs.reason, "code ready for review");

    cs.transition(ChangesetState::Verifying, "starting verification pipeline")
        .unwrap();
    assert_eq!(cs.reason, "starting verification pipeline");

    cs.transition(ChangesetState::Approved, "all checks passed")
        .unwrap();
    assert_eq!(cs.reason, "all checks passed");

    cs.transition(ChangesetState::Merged, "fast-forward merge")
        .unwrap();
    assert_eq!(cs.state, "merged");
    assert_eq!(cs.reason, "fast-forward merge");
}

#[test]
fn transition_to_closed_from_any_state() {
    for start_state in &["draft", "submitted", "verifying", "approved", "rejected"] {
        let mut cs = changeset_in_state(start_state);
        let result = cs.transition(ChangesetState::Closed, "user cancelled");
        assert!(
            result.is_ok(),
            "transition {start_state} -> closed should succeed"
        );
        assert_eq!(cs.state, "closed");
        assert_eq!(cs.reason, "user cancelled");
    }
}

#[test]
fn transition_rejects_unknown_current_state() {
    let mut cs = changeset_in_state("bogus");
    let result = cs.transition(ChangesetState::Submitted, "should fail");
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("unknown current state"),
        "expected 'unknown current state' in error, got: {err_msg}"
    );
}

#[test]
fn transition_rejection_to_closed_works() {
    let mut cs = changeset_in_state("verifying");
    cs.transition(ChangesetState::Rejected, "cargo clippy found 3 warnings")
        .unwrap();
    assert_eq!(cs.state, "rejected");
    assert_eq!(cs.reason, "cargo clippy found 3 warnings");

    cs.transition(ChangesetState::Closed, "closing after rejection")
        .unwrap();
    assert_eq!(cs.state, "closed");
}

#[test]
fn parsed_state_returns_correct_enum() {
    let cs = changeset_in_state("approved");
    assert_eq!(cs.parsed_state(), Some(ChangesetState::Approved));
}

#[test]
fn parsed_state_returns_none_for_unknown() {
    let cs = changeset_in_state("invalid");
    assert_eq!(cs.parsed_state(), None);
}

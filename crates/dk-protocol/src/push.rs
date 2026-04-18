use tonic::Status;

use crate::server::ProtocolServer;
use crate::{PushMode, PushRequest, PushResponse};

/// Validate that a branch name conforms to `git check-ref-format` rules.
///
/// Returns `Ok(())` if valid, or an `Err(Status::invalid_argument)` describing
/// the problem.  This is intentionally a subset of the full git rules — the
/// platform layer runs the real `git check-ref-format`, but catching the most
/// common mistakes here lets us return a clear gRPC error instead of an opaque
/// git failure.
fn validate_branch_name(name: &str) -> Result<(), Status> {
    if name.is_empty() {
        return Err(Status::invalid_argument("branch_name is required"));
    }

    // Single-character checks
    for &ch in &[' ', '~', '^', ':', '?', '*', '[', '\\'] {
        if name.contains(ch) {
            return Err(Status::invalid_argument(format!(
                "branch_name contains invalid character '{ch}'"
            )));
        }
    }

    // Substring / pattern checks
    if name.contains("..") {
        return Err(Status::invalid_argument(
            "branch_name must not contain '..'",
        ));
    }
    if name.contains("@{") {
        return Err(Status::invalid_argument(
            "branch_name must not contain '@{{'",
        ));
    }

    // Position checks
    if name.starts_with('/') || name.ends_with('/') {
        return Err(Status::invalid_argument(
            "branch_name must not start or end with '/'",
        ));
    }
    if name.starts_with('.') || name.ends_with('.') {
        return Err(Status::invalid_argument(
            "branch_name must not start or end with '.'",
        ));
    }
    if name.starts_with('-') {
        return Err(Status::invalid_argument(
            "branch_name must not start with '-'",
        ));
    }

    // Control characters (0x00–0x1F) and DEL (0x7F)
    if name.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(Status::invalid_argument(
            "branch_name must not contain control characters",
        ));
    }

    // .lock suffix on any path component
    for component in name.split('/') {
        if component.ends_with(".lock") {
            return Err(Status::invalid_argument(
                "branch_name path component must not end with '.lock'",
            ));
        }
        if component.is_empty() {
            return Err(Status::invalid_argument(
                "branch_name must not contain consecutive slashes",
            ));
        }
        if component.starts_with('.') {
            return Err(Status::invalid_argument(
                "branch_name path component must not start with '.'",
            ));
        }
    }

    Ok(())
}

/// Handle a Push request.
///
/// The engine's role is lightweight: validate the session exists and return
/// the repo info. The actual GitHub push (git operations, token handling,
/// PR creation) happens in the platform layer's gRPC wrapper.
pub async fn handle_push(
    server: &ProtocolServer,
    req: PushRequest,
) -> Result<PushResponse, Status> {
    // Validate session
    let _session = server.validate_session(&req.session_id)?;
    crate::require_live_session::require_live_session(server, &req.session_id).await?;

    // Validate mode
    let mode = req.mode();
    if mode == PushMode::Unspecified {
        return Err(Status::invalid_argument(
            "mode must be PUSH_MODE_BRANCH or PUSH_MODE_PR",
        ));
    }

    // Validate branch_name
    validate_branch_name(&req.branch_name)?;

    // Validate pr fields when mode is PR
    if mode == PushMode::Pr && req.pr_title.is_empty() {
        return Err(Status::invalid_argument(
            "pr_title is required when mode is PUSH_MODE_PR",
        ));
    }

    // Return empty response — the platform wrapper fills in the actual
    // push results (branch_name, pr_url, commit_hash, changeset_ids).
    Ok(PushResponse {
        branch_name: req.branch_name,
        pr_url: String::new(),
        commit_hash: String::new(),
        changeset_ids: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_response_fields() {
        let resp = PushResponse {
            branch_name: "feat/xyz".to_string(),
            pr_url: "https://github.com/org/repo/pull/1".to_string(),
            commit_hash: "abc123".to_string(),
            changeset_ids: vec!["cs-1".to_string(), "cs-2".to_string()],
        };
        assert_eq!(resp.branch_name, "feat/xyz");
        assert_eq!(resp.changeset_ids.len(), 2);
    }

    // ── branch_name validation ──

    #[test]
    fn valid_branch_names() {
        for name in &[
            "main",
            "feat/xyz",
            "fix/issue-42",
            "release/v1.0.0",
            "user/alice/topic",
        ] {
            assert!(validate_branch_name(name).is_ok(), "expected ok for {name}");
        }
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_branch_name("").is_err());
    }

    #[test]
    fn rejects_spaces() {
        assert!(validate_branch_name("feat xyz").is_err());
    }

    #[test]
    fn rejects_double_dot() {
        assert!(validate_branch_name("feat..bar").is_err());
    }

    #[test]
    fn rejects_leading_slash() {
        assert!(validate_branch_name("/feat").is_err());
    }

    #[test]
    fn rejects_trailing_slash() {
        assert!(validate_branch_name("feat/").is_err());
    }

    #[test]
    fn rejects_trailing_dot() {
        assert!(validate_branch_name("feat.").is_err());
    }

    #[test]
    fn rejects_leading_dot() {
        assert!(validate_branch_name(".feat").is_err());
    }

    #[test]
    fn rejects_tilde() {
        assert!(validate_branch_name("feat~1").is_err());
    }

    #[test]
    fn rejects_caret() {
        assert!(validate_branch_name("feat^2").is_err());
    }

    #[test]
    fn rejects_colon() {
        assert!(validate_branch_name("HEAD:path").is_err());
    }

    #[test]
    fn rejects_question_mark() {
        assert!(validate_branch_name("feat?").is_err());
    }

    #[test]
    fn rejects_asterisk() {
        assert!(validate_branch_name("feat*").is_err());
    }

    #[test]
    fn rejects_open_bracket() {
        assert!(validate_branch_name("feat[0]").is_err());
    }

    #[test]
    fn rejects_backslash() {
        assert!(validate_branch_name("feat\\bar").is_err());
    }

    #[test]
    fn rejects_at_brace() {
        assert!(validate_branch_name("feat@{0}").is_err());
    }

    #[test]
    fn rejects_control_chars() {
        assert!(validate_branch_name("feat\x01bar").is_err());
        assert!(validate_branch_name("feat\x7fbar").is_err());
    }

    #[test]
    fn rejects_lock_suffix() {
        assert!(validate_branch_name("refs/heads/main.lock").is_err());
        assert!(validate_branch_name("feat.lock/bar").is_err());
    }

    #[test]
    fn rejects_consecutive_slashes() {
        assert!(validate_branch_name("feat//bar").is_err());
    }

    #[test]
    fn rejects_hidden_component() {
        assert!(validate_branch_name("refs/.hidden/branch").is_err());
    }

    #[test]
    fn rejects_leading_dash() {
        assert!(validate_branch_name("-feat").is_err());
    }
}

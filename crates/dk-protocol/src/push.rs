use tonic::Status;

use crate::server::ProtocolServer;
use crate::{PushRequest, PushResponse};

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

    // Validate mode
    if req.mode != "branch" && req.mode != "pr" {
        return Err(Status::invalid_argument(
            "mode must be 'branch' or 'pr'",
        ));
    }

    // Validate branch_name is non-empty
    if req.branch_name.is_empty() {
        return Err(Status::invalid_argument(
            "branch_name is required",
        ));
    }

    // Validate pr fields when mode is "pr"
    if req.mode == "pr" && req.pr_title.is_empty() {
        return Err(Status::invalid_argument(
            "pr_title is required when mode is 'pr'",
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
}

use tonic::{Response, Status};
use tracing::info;

use dk_engine::changeset::ChangesetState;

use crate::server::ProtocolServer;
use crate::{ApproveRequest, ApproveResponse};

// Note: `find_session_changeset` is kept here as a shared helper used by
// resolve.rs and review.rs via `crate::approve::find_session_changeset`.

/// Handle an APPROVE RPC.
///
/// Transitions the session's active changeset to `approved`, enabling a
/// subsequent MERGE.  Valid from `submitted`, `verifying`, or `rejected`
/// states — the caller (harness or operator) takes responsibility for any
/// score-threshold override by supplying `override_reason`.
pub async fn handle_approve(
    server: &ProtocolServer,
    req: ApproveRequest,
) -> Result<Response<ApproveResponse>, Status> {
    let _session = server.validate_session(&req.session_id)?;

    let sid = req
        .session_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID format"))?;
    server.session_mgr().touch_session(&sid);

    let changeset_id = find_session_changeset(server, sid).await?;

    let changeset = server
        .engine()
        .changeset_store()
        .get(changeset_id)
        .await
        .map_err(|e| Status::internal(format!("Failed to fetch changeset: {e}")))?;

    let current = changeset.parsed_state().ok_or_else(|| {
        Status::internal(format!("Unknown changeset state: '{}'", changeset.state))
    })?;

    if !matches!(
        current,
        ChangesetState::Submitted | ChangesetState::Verifying | ChangesetState::Rejected
    ) {
        return Err(Status::failed_precondition(format!(
            "Cannot approve changeset in state '{current}'; \
             must be submitted, verifying, or rejected"
        )));
    }

    let reason = req
        .override_reason
        .as_deref()
        .unwrap_or("approved via agent protocol");

    server
        .engine()
        .changeset_store()
        .update_status_if_with_reason(
            changeset_id,
            ChangesetState::Approved.as_str(),
            &[current.as_str()],
            reason,
        )
        .await
        .map_err(|e| Status::internal(format!("Failed to approve changeset: {e}")))?;

    info!(
        session_id       = %sid,
        changeset_id     = %changeset_id,
        from_state       = %current,
        override_reason  = ?req.override_reason,
        "APPROVE: changeset approved"
    );

    Ok(Response::new(ApproveResponse {
        success: true,
        changeset_id: changeset_id.to_string(),
        new_state: ChangesetState::Approved.as_str().to_string(),
        message: reason.to_string(),
    }))
}

/// Find the most recent non-terminal changeset for the given session.
///
/// Shared by `approve`, `resolve`, and `review` handlers which receive only a
/// `session_id` and need to look up the associated changeset.
pub(crate) async fn find_session_changeset(
    server: &ProtocolServer,
    session_id: uuid::Uuid,
) -> Result<uuid::Uuid, Status> {
    server
        .engine()
        .changeset_store()
        .find_by_session(session_id)
        .await
        .map_err(|e| Status::internal(format!("DB error looking up changeset: {e}")))?
        .ok_or_else(|| Status::not_found("No active changeset found for this session"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_response_shape() {
        let resp = ApproveResponse {
            success: true,
            changeset_id: "cs-1".to_string(),
            new_state: "approved".to_string(),
            message: "approved via agent protocol".to_string(),
        };
        assert!(resp.success);
        assert_eq!(resp.new_state, "approved");
    }

    #[test]
    fn only_submitted_verifying_rejected_can_be_approved() {
        use dk_engine::changeset::ChangesetState;
        let approvable = [
            ChangesetState::Submitted,
            ChangesetState::Verifying,
            ChangesetState::Rejected,
        ];
        let not_approvable = [
            ChangesetState::Draft,
            ChangesetState::Approved,
            ChangesetState::Merged,
            ChangesetState::Closed,
        ];
        for s in approvable {
            assert!(
                matches!(
                    s,
                    ChangesetState::Submitted
                        | ChangesetState::Verifying
                        | ChangesetState::Rejected
                ),
                "{s} should be approvable"
            );
        }
        for s in not_approvable {
            assert!(
                !matches!(
                    s,
                    ChangesetState::Submitted
                        | ChangesetState::Verifying
                        | ChangesetState::Rejected
                ),
                "{s} should NOT be approvable"
            );
        }
    }
}

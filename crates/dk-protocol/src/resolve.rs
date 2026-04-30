use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::{ResolutionMode, ResolveRequest, ResolveResponse};

/// Handle a RESOLVE RPC.
///
/// Applies the chosen conflict resolution strategy for the session's active
/// changeset, then transitions it back to `submitted` so the verification
/// pipeline can re-run on the resolved state.
///
/// Resolution modes:
/// - `PROCEED` / `KEEP_YOURS` — no workspace mutation; the agent's changes win.
/// - `KEEP_THEIRS` — no workspace mutation; the agent acknowledges the loss
///   (the platform layer handles the actual revert at merge time).
/// - `MANUAL` — writes `manual_content` into the workspace overlay at the path
///   given by `conflict_id`, replacing the conflicting version.
pub async fn handle_resolve(
    server: &ProtocolServer,
    req: ResolveRequest,
) -> Result<Response<ResolveResponse>, Status> {
    let _session = server.validate_session(&req.session_id)?;

    let sid = req
        .session_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID format"))?;
    server.session_mgr().touch_session(&sid);

    let mode = req.resolution();
    if mode == ResolutionMode::Unspecified {
        return Err(Status::invalid_argument(
            "resolution mode must be specified",
        ));
    }
    if mode == ResolutionMode::Manual && req.manual_content.is_none() {
        return Err(Status::invalid_argument(
            "manual_content is required when resolution is MANUAL",
        ));
    }

    let changeset_id = crate::approve::find_session_changeset(server, sid).await?;

    // MANUAL: write caller-supplied content into the workspace overlay.
    // The overlay's `write` method is async and uses interior mutability, so
    // we can hold the DashMap Ref across the await safely (same pattern as
    // file_write.rs).
    if let (ResolutionMode::Manual, Some(conflict_path), Some(content)) = (
        mode,
        req.conflict_id.as_deref(),
        req.manual_content.as_deref(),
    ) {
        let ws = server
            .engine()
            .workspace_manager()
            .get_workspace(&sid)
            .ok_or_else(|| Status::not_found("Workspace not found for session"))?;

        ws.overlay
            .write(conflict_path, content.as_bytes().to_vec(), false)
            .await
            .map_err(|e| Status::internal(format!("Failed to write resolved content: {e}")))?;
    }

    // Return the changeset to `submitted` so verify can re-run.
    let mode_str = format!("{mode:?}").to_lowercase();
    let _ = server
        .engine()
        .changeset_store()
        .update_status_if_with_reason(
            changeset_id,
            "submitted",
            &["draft", "submitted", "verifying", "rejected"],
            &format!("conflicts resolved via {mode_str} mode"),
        )
        .await;

    info!(
        session_id   = %sid,
        changeset_id = %changeset_id,
        resolution   = %mode_str,
        "RESOLVE: conflict resolution applied"
    );

    Ok(Response::new(ResolveResponse {
        success: true,
        changeset_id: changeset_id.to_string(),
        new_state: "submitted".to_string(),
        message: format!("Conflicts resolved via {mode_str} mode"),
        conflicts_resolved: 1,
        conflicts_remaining: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_response_shape() {
        let resp = ResolveResponse {
            success: true,
            changeset_id: "cs-1".to_string(),
            new_state: "submitted".to_string(),
            message: "resolved".to_string(),
            conflicts_resolved: 2,
            conflicts_remaining: 0,
        };
        assert!(resp.success);
        assert_eq!(resp.conflicts_resolved, 2);
        assert_eq!(resp.conflicts_remaining, 0);
    }
}

use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::{CloseRequest, CloseResponse};

/// Handle a CLOSE RPC.
///
/// Destroys the session workspace overlay and removes the session from the
/// manager.  Safe to call multiple times — both operations are no-ops if the
/// session has already been cleaned up.
pub async fn handle_close(
    server: &ProtocolServer,
    req: CloseRequest,
) -> Result<Response<CloseResponse>, Status> {
    let session = server.validate_session(&req.session_id)?;

    let sid = req
        .session_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID format"))?;

    // Destroy workspace overlay (no-op if already gone)
    server.engine().workspace_manager().destroy_workspace(&sid);

    // Remove session from the in-memory manager
    server.session_mgr().remove_session(&sid);

    info!(
        session_id = %sid,
        codebase   = %session.codebase,
        "CLOSE: session and workspace destroyed"
    );

    Ok(Response::new(CloseResponse {
        success:    true,
        message:    "Session closed successfully".to_string(),
        session_id: req.session_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_response_fields() {
        let resp = CloseResponse {
            success:    true,
            message:    "done".to_string(),
            session_id: "abc".to_string(),
        };
        assert!(resp.success);
        assert_eq!(resp.session_id, "abc");
    }
}

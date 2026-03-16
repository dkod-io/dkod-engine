use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::validation::validate_file_path;
use crate::{FileReadRequest, FileReadResponse};

/// Handle a FileRead RPC.
///
/// Reads a file through the session workspace overlay:
/// 1. Check the overlay for session-local modifications.
/// 2. Fall through to the Git tree at the workspace's base commit.
pub async fn handle_file_read(
    server: &ProtocolServer,
    req: FileReadRequest,
) -> Result<Response<FileReadResponse>, Status> {
    validate_file_path(&req.path)?;

    let session = server.validate_session(&req.session_id)?;

    let sid = req
        .session_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;
    server.session_mgr().touch_session(&sid);

    let engine = server.engine();

    // Get workspace for this session
    let ws = engine
        .workspace_manager()
        .get_workspace(&sid)
        .ok_or_else(|| Status::not_found("Workspace not found for session"))?;

    // Get git repo for base-tree fallback
    let (_repo_id, git_repo) = engine
        .get_repo(&session.codebase)
        .await
        .map_err(|e| Status::internal(format!("Repo error: {e}")))?;

    let result = ws
        .read_file(&req.path, &git_repo)
        .map_err(|e| Status::not_found(format!("File not found: {e}")))?;

    info!(
        session_id = %req.session_id,
        path = %req.path,
        modified = result.modified_in_session,
        "FILE_READ: served"
    );

    Ok(Response::new(FileReadResponse {
        content: result.content,
        hash: result.hash,
        modified_in_session: result.modified_in_session,
    }))
}

use tonic::Status;

use crate::server::ProtocolServer;
use crate::{SessionStatusRequest, SessionStatusResponse};

pub async fn handle_get_session_status(
    server: &ProtocolServer,
    req: SessionStatusRequest,
) -> Result<tonic::Response<SessionStatusResponse>, Status> {
    let session = server.validate_session(&req.session_id)?;
    server.session_mgr().touch_session(&session.id);

    let ws = server
        .engine()
        .workspace_manager()
        .get_workspace(&session.id)
        .ok_or_else(|| Status::not_found("workspace not found for session"))?;

    let files_modified = ws.overlay.list_paths();
    let overlay_size_bytes = ws.overlay.total_bytes() as u64;
    let repo_id = ws.repo_id;
    let base_commit = ws.base_commit.clone();
    let changeset_id = ws.changeset_id;
    drop(ws);

    let active_other = server
        .engine()
        .workspace_manager()
        .active_sessions_for_repo(repo_id, Some(session.id))
        .len() as u32;

    let symbols_modified = match server
        .engine()
        .changeset_store()
        .get_affected_symbols(changeset_id)
        .await
    {
        Ok(syms) => syms.into_iter().map(|(_, qn)| qn).collect(),
        Err(_) => vec![],
    };

    Ok(tonic::Response::new(SessionStatusResponse {
        session_id: session.id.to_string(),
        base_commit,
        files_modified,
        symbols_modified,
        overlay_size_bytes,
        active_other_sessions: active_other,
    }))
}

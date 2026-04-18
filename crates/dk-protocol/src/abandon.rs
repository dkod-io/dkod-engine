use tonic::{Response, Status};
use tracing::info;
use uuid::Uuid;

use crate::server::ProtocolServer;
use crate::{AbandonRequest, AbandonResponse};
use dk_engine::workspace::session_manager::AbandonReason;

pub async fn handle_abandon(
    server: &ProtocolServer,
    req: AbandonRequest,
) -> Result<Response<AbandonResponse>, Status> {
    // Validate session (exists + belongs to caller's agent identity).
    let caller_session = server.validate_session(&req.session_id)?;
    let caller_agent = caller_session.agent_id.clone();

    let sid = req
        .session_id
        .parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;

    type WorkspaceRow = (
        String,
        Option<Uuid>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    );

    // Look up the workspace row for this session.
    let row: Option<WorkspaceRow> = sqlx::query_as(
        r#"
        SELECT agent_id, changeset_id, stranded_at, abandoned_at
          FROM session_workspaces WHERE session_id = $1
        "#,
    )
    .bind(sid)
    .fetch_optional(&server.engine().db)
    .await
    .map_err(|e| Status::internal(format!("workspace lookup failed: {e}")))?;

    let Some((orig_agent, changeset_id_opt, stranded_at, abandoned_at)) = row else {
        return Err(Status::not_found("Workspace not found"));
    };
    if orig_agent != caller_agent {
        return Err(Status::unauthenticated(format!(
            "abandon requires original agent_id '{orig_agent}'"
        )));
    }

    let changeset_str = changeset_id_opt
        .map(|u| u.to_string())
        .unwrap_or_default();

    // Idempotent: if already abandoned, just return success.
    if abandoned_at.is_some() {
        return Ok(Response::new(AbandonResponse {
            success: true,
            changeset_id: changeset_str,
            abandoned_reason: "explicit".into(),
        }));
    }
    if stranded_at.is_none() {
        return Err(Status::failed_precondition("session is not stranded"));
    }

    server
        .engine()
        .workspace_manager()
        .abandon_stranded(&sid, AbandonReason::Explicit { caller: caller_agent.clone() })
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(session_id = %req.session_id, caller = %caller_agent, "ABANDON: done");

    Ok(Response::new(AbandonResponse {
        success: true,
        changeset_id: changeset_str,
        abandoned_reason: "explicit".into(),
    }))
}

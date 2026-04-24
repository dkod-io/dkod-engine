use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use crate::server::ProtocolServer;
use crate::{AbandonRequest, AbandonResponse};
use dk_engine::workspace::session_manager::AbandonReason;

pub async fn handle_abandon(
    server: &ProtocolServer,
    request: Request<AbandonRequest>,
) -> Result<Response<AbandonResponse>, Status> {
    // Check whether the caller is an admin (scope "admin" in JWT).
    let is_admin = server.has_admin_scope(request.metadata());

    // Extract operator name from the `dk-admin-operator` metadata header when
    // the caller is admin.
    let dk_admin_operator: Option<String> = request
        .metadata()
        .get("dk-admin-operator")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let req = request.into_inner();

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

    // Owner check: admins bypass it; regular callers must be the original agent.
    // Use permission_denied (authenticated but not authorized) rather than
    // unauthenticated (which implies identity is unknown).
    if !is_admin && orig_agent != caller_agent {
        return Err(Status::permission_denied(format!(
            "abandon requires original agent_id '{orig_agent}'"
        )));
    }

    let changeset_str = changeset_id_opt.map(|u| u.to_string()).unwrap_or_default();

    // Idempotent: if already abandoned, just return success.
    if abandoned_at.is_some() {
        let reason_str = if is_admin { "admin" } else { "explicit" };
        return Ok(Response::new(AbandonResponse {
            success: true,
            changeset_id: changeset_str,
            abandoned_reason: reason_str.into(),
        }));
    }
    if stranded_at.is_none() {
        return Err(Status::failed_precondition("session is not stranded"));
    }

    let operator_name = dk_admin_operator.unwrap_or_else(|| "unknown-admin".to_string());

    let reason = if is_admin {
        AbandonReason::Admin {
            operator: operator_name.clone(),
        }
    } else {
        AbandonReason::Explicit {
            caller: caller_agent.clone(),
        }
    };

    let reason_str = reason.as_str().to_string();

    server
        .engine()
        .workspace_manager()
        .abandon_stranded(&sid, reason)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    if is_admin {
        info!(
            session_id = %req.session_id,
            operator = %operator_name,
            "ABANDON (admin): done"
        );
    } else {
        info!(session_id = %req.session_id, caller = %caller_agent, "ABANDON: done");
    }

    Ok(Response::new(AbandonResponse {
        success: true,
        changeset_id: changeset_str,
        abandoned_reason: reason_str,
    }))
}

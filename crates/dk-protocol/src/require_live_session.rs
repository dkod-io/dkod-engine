//! Middleware — every dk_* RPC must call this before touching workspace state.
//! If the workspace is live in-memory, returns Ok. Otherwise looks up the
//! persisted session_workspaces row and translates missing / stranded /
//! abandoned into structured gRPC statuses (carried as metadata keys prefixed
//! with `dk-`).

use tonic::metadata::MetadataValue;
use tonic::Status;
use uuid::Uuid;

use crate::server::ProtocolServer;

pub async fn require_live_session(
    server: &ProtocolServer,
    session_id: &str,
) -> Result<(), Status> {
    let sid = session_id
        .parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;

    // Live in-memory → proceed.
    if server.engine().workspace_manager().get_workspace(&sid).is_some() {
        return Ok(());
    }

    // Look up persistent state.
    type Row = (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<Uuid>,
        Option<String>,
        Option<String>,
        String,
    );
    let row: Option<Row> = sqlx::query_as(
        r#"
        SELECT w.stranded_at, w.abandoned_at, w.changeset_id,
               w.stranded_reason, w.abandoned_reason, w.base_commit_hash
          FROM session_workspaces w
         WHERE w.session_id = $1
        "#,
    )
    .bind(sid)
    .fetch_optional(&server.engine().db)
    .await
    .map_err(|e| Status::internal(format!("workspace lookup failed: {e}")))?;

    let Some((stranded_at, abandoned_at, changeset_id_opt, stranded_reason, abandoned_reason, base_commit)) = row else {
        return Err(Status::not_found("Workspace not found for session"));
    };

    let changeset_str = changeset_id_opt
        .map(|u: Uuid| u.to_string())
        .unwrap_or_default();

    if let Some(at) = abandoned_at {
        let mut st = Status::failed_precondition("session abandoned");
        st.metadata_mut().insert(
            "dk-error",
            MetadataValue::try_from("SESSION_ABANDONED").unwrap(),
        );
        st.metadata_mut().insert(
            "dk-changeset-id",
            MetadataValue::try_from(changeset_str.as_str()).unwrap(),
        );
        if let Some(r) = abandoned_reason {
            st.metadata_mut().insert(
                "dk-abandoned-reason",
                MetadataValue::try_from(r.as_str()).unwrap(),
            );
        }
        st.metadata_mut().insert(
            "dk-abandoned-at",
            MetadataValue::try_from(at.to_rfc3339().as_str()).unwrap(),
        );
        return Err(st);
    }
    if let Some(at) = stranded_at {
        let mut st = Status::failed_precondition("session stranded");
        st.metadata_mut().insert(
            "dk-error",
            MetadataValue::try_from("SESSION_STRANDED").unwrap(),
        );
        st.metadata_mut().insert(
            "dk-changeset-id",
            MetadataValue::try_from(changeset_str.as_str()).unwrap(),
        );
        st.metadata_mut().insert(
            "dk-base-commit",
            MetadataValue::try_from(base_commit.as_str()).unwrap(),
        );
        if let Some(r) = stranded_reason {
            st.metadata_mut().insert(
                "dk-stranded-reason",
                MetadataValue::try_from(r.as_str()).unwrap(),
            );
        }
        st.metadata_mut().insert(
            "dk-stranded-at",
            MetadataValue::try_from(at.to_rfc3339().as_str()).unwrap(),
        );
        return Err(st);
    }

    // Row exists but neither stranded nor abandoned nor in-memory (edge case):
    // transient or stale — treat as not found.
    Err(Status::not_found("Workspace not found for session"))
}

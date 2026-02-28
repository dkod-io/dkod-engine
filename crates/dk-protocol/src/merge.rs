use tonic::Status;
use uuid::Uuid;

use dk_engine::workspace::merge::{merge_workspace, WorkspaceMergeResult};

use crate::server::ProtocolServer;
use crate::{ConflictInfo, MergeRequest, MergeResponse};

pub async fn handle_merge(
    server: &ProtocolServer,
    req: MergeRequest,
) -> Result<MergeResponse, Status> {
    let session = server.validate_session(&req.session_id)?;
    let engine = server.engine();

    let sid = req
        .session_id
        .parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;

    let changeset_id = req.changeset_id.parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("invalid changeset_id"))?;

    // Get changeset and verify it's approved
    let changeset = engine.changeset_store().get(changeset_id).await
        .map_err(|e| Status::not_found(e.to_string()))?;

    if changeset.state != "approved" {
        return Err(Status::failed_precondition(format!(
            "changeset is '{}', must be 'approved' to merge",
            changeset.state
        )));
    }

    // Get workspace for this session
    let ws = engine
        .workspace_manager()
        .get_workspace(&sid)
        .ok_or_else(|| Status::not_found("Workspace not found for session"))?;

    // Get git repo
    let (_, git_repo) = engine.get_repo(&session.codebase).await
        .map_err(|e| Status::internal(e.to_string()))?;

    let agent = changeset.agent_id.as_deref().unwrap_or("agent");

    // Use the programmatic workspace merge instead of git add -A
    let merge_result = merge_workspace(
        &ws,
        &git_repo,
        engine.parser(),
        &req.commit_message,
        agent,
        &format!("{}@dekode.dev", agent),
    )
    .map_err(|e| Status::internal(format!("merge failed: {e}")))?;

    // Drop workspace guard before further async work
    drop(ws);

    match merge_result {
        WorkspaceMergeResult::FastMerge { commit_hash } => {
            // Update changeset status to merged
            engine.changeset_store().set_merged(changeset_id, &commit_hash).await
                .map_err(|e| Status::internal(e.to_string()))?;

            // Publish event
            server.event_bus().publish(crate::WatchEvent {
                event_type: "changeset.merged".to_string(),
                changeset_id: changeset_id.to_string(),
                agent_id: changeset.agent_id.clone().unwrap_or_default(),
                affected_symbols: vec![],
                details: format!("fast-merged as {}", commit_hash),
            });

            Ok(MergeResponse {
                commit_hash: commit_hash.clone(),
                merged_version: commit_hash,
                conflicts: Vec::new(),
                auto_rebased: false,
                auto_rebased_files: Vec::new(),
            })
        }

        WorkspaceMergeResult::RebaseMerge {
            commit_hash,
            auto_rebased_files,
        } => {
            // Update changeset status to merged
            engine.changeset_store().set_merged(changeset_id, &commit_hash).await
                .map_err(|e| Status::internal(e.to_string()))?;

            // Publish event
            server.event_bus().publish(crate::WatchEvent {
                event_type: "changeset.merged".to_string(),
                changeset_id: changeset_id.to_string(),
                agent_id: changeset.agent_id.clone().unwrap_or_default(),
                affected_symbols: vec![],
                details: format!(
                    "rebase-merged as {} (auto-rebased {} files)",
                    commit_hash,
                    auto_rebased_files.len()
                ),
            });

            Ok(MergeResponse {
                commit_hash: commit_hash.clone(),
                merged_version: commit_hash,
                conflicts: Vec::new(),
                auto_rebased: true,
                auto_rebased_files,
            })
        }

        WorkspaceMergeResult::Conflicts { conflicts } => {
            let conflict_infos: Vec<ConflictInfo> = conflicts
                .iter()
                .map(|c| ConflictInfo {
                    file_path: c.file_path.clone(),
                    symbol_name: c.symbol_name.clone(),
                    conflict_type: "semantic".to_string(),
                    other_agent_id: String::new(),
                    other_changeset_id: String::new(),
                    description: format!(
                        "Symbol '{}' — our change: {:?}, their change: {:?}",
                        c.symbol_name, c.our_change, c.their_change
                    ),
                })
                .collect();

            Ok(MergeResponse {
                commit_hash: String::new(),
                merged_version: String::new(),
                conflicts: conflict_infos,
                auto_rebased: false,
                auto_rebased_files: Vec::new(),
            })
        }
    }
}

// ── Event type constant ─────────────────────────────────────────────

/// Event published when a changeset is successfully merged.
pub const EVENT_MERGED: &str = "changeset.merged";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_event_type() {
        assert_eq!(EVENT_MERGED, "changeset.merged");
    }

    #[test]
    fn merged_event_type_uses_dot_separator() {
        assert!(
            EVENT_MERGED.contains('.'),
            "event type should use dot separator"
        );
        assert!(
            EVENT_MERGED.starts_with("changeset."),
            "event type should start with 'changeset.'"
        );
    }

    #[test]
    fn merged_event_type_is_not_underscore_format() {
        // Verify the event was renamed from "changeset_merged" to "changeset.merged"
        assert_ne!(EVENT_MERGED, "changeset_merged");
        assert_eq!(EVENT_MERGED, "changeset.merged");
    }
}

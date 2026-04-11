use tonic::Status;
use uuid::Uuid;

use dk_engine::workspace::merge::{merge_workspace, WorkspaceMergeResult};

use crate::server::ProtocolServer;
use crate::{merge_response, ConflictDetail, MergeConflict, MergeRequest, MergeResponse, MergeSuccess};

/// Conflict type for true write-write semantic conflicts.
const CONFLICT_TYPE_TRUE: &str = "true_conflict";

/// Sanitize a string for protobuf `string` fields.
///
/// Rust `String` is guaranteed valid UTF-8, but content originating from
/// tree-sitter AST parsing may contain null bytes or replacement characters
/// from lossy conversions.  Strip null bytes so the value round-trips cleanly
/// through protobuf serialization/deserialization.
fn sanitize_for_proto(s: &str) -> String {
    s.replace('\0', "")
}

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

    // Resolve repo_id_str for enriched events (non-fatal — empty string on failure)
    let repo_id_str = match engine.get_repo(&session.codebase).await {
        Ok((rid, _)) => rid.to_string(),
        Err(_) => String::new(),
    };

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

    // Get git repo — also use this repo_id for lock release (the first get_repo
    // call is non-fatal and may return empty string, but this one propagates errors)
    let (repo_id, git_repo) = engine.get_repo(&session.codebase).await
        .map_err(|e| Status::internal(e.to_string()))?;

    let agent = changeset.agent_id.as_deref().unwrap_or("agent");

    let (effective_name, effective_email) =
        dk_core::resolve_author(&req.author_name, &req.author_email, agent);

    // Capture affected files from workspace overlay before merge/drop
    let affected_files: Vec<crate::FileChange> = ws.overlay.list_changes()
        .iter()
        .map(|(path, entry)| {
            let operation = match entry {
                dk_engine::workspace::overlay::OverlayEntry::Added { .. } => "add",
                dk_engine::workspace::overlay::OverlayEntry::Modified { .. } => "modify",
                dk_engine::workspace::overlay::OverlayEntry::Deleted => "delete",
            };
            crate::FileChange {
                path: path.clone(),
                operation: operation.to_string(),
            }
        })
        .collect();

    // Use the programmatic workspace merge instead of git add -A
    let merge_result = merge_workspace(
        &ws,
        &git_repo,
        engine.parser(),
        &req.commit_message,
        &effective_name,
        &effective_email,
    )
    .map_err(|e| Status::internal(format!("merge failed: {e}")))?;

    // Drop workspace guard before further async work
    drop(ws);

    match merge_result {
        WorkspaceMergeResult::FastMerge { commit_hash } => {
            // Update changeset status to merged
            engine.changeset_store().set_merged(changeset_id, &commit_hash).await
                .map_err(|e| Status::internal(e.to_string()))?;

            // Release symbol locks and emit lock release events
            release_locks_and_emit(server, repo_id, sid, &req.session_id);

            // Publish merge event
            server.event_bus().publish(crate::WatchEvent {
                event_type: "changeset.merged".to_string(),
                changeset_id: changeset_id.to_string(),
                agent_id: changeset.agent_id.clone().unwrap_or_default(),
                affected_symbols: vec![],
                details: format!("fast-merged as {}", commit_hash),
                session_id: req.session_id.clone(),
                affected_files: affected_files.clone(),
                symbol_changes: vec![],
                repo_id: repo_id_str.clone(),
                event_id: Uuid::new_v4().to_string(),
            });

            Ok(MergeResponse {
                result: Some(merge_response::Result::Success(MergeSuccess {
                    commit_hash: commit_hash.clone(),
                    merged_version: commit_hash,
                    auto_rebased: false,
                    auto_rebased_files: Vec::new(),
                })),
            })
        }

        WorkspaceMergeResult::RebaseMerge {
            commit_hash,
            auto_rebased_files,
        } => {
            // Update changeset status to merged
            engine.changeset_store().set_merged(changeset_id, &commit_hash).await
                .map_err(|e| Status::internal(e.to_string()))?;

            // Release symbol locks and emit lock release events
            release_locks_and_emit(server, repo_id, sid, &req.session_id);

            // Publish merge event
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
                session_id: req.session_id.clone(),
                affected_files,
                symbol_changes: vec![],
                repo_id: repo_id_str.clone(),
                event_id: Uuid::new_v4().to_string(),
            });

            Ok(MergeResponse {
                result: Some(merge_response::Result::Success(MergeSuccess {
                    commit_hash: commit_hash.clone(),
                    merged_version: commit_hash,
                    auto_rebased: true,
                    auto_rebased_files,
                })),
            })
        }

        WorkspaceMergeResult::Conflicts { conflicts } => {
            // Intentionally NOT releasing locks here. The agent retains its locks
            // while resolving conflicts (dk_resolve → retry dk_merge). Locks are
            // released when the session is closed (dk_close) or times out (30 min GC).
            let conflict_details: Vec<ConflictDetail> = conflicts
                .iter()
                .map(|c| {
                    let file = sanitize_for_proto(&c.file_path);
                    let symbol = sanitize_for_proto(&c.symbol_name);
                    ConflictDetail {
                        file_path: file,
                        symbols: vec![symbol.clone()],
                        your_agent: agent.to_string(),
                        // TODO: resolve their_agent from the session/changeset store
                        // once SemanticConflict carries agent attribution.
                        their_agent: String::new(),
                        conflict_type: CONFLICT_TYPE_TRUE.to_string(),
                        description: format!(
                            "Symbol '{}' — our change: {:?}, their change: {:?}",
                            symbol, c.our_change, c.their_change
                        ),
                    }
                })
                .collect();

            let suggested_action = "adapt".to_string();
            let available_actions = vec!["adapt".to_string(), "keep_mine".to_string(), "keep_theirs".to_string()];

            debug_assert!(
                available_actions.iter().any(|a| a == &suggested_action),
                "suggested_action '{}' is not in available_actions {:?}",
                suggested_action, available_actions
            );

            Ok(MergeResponse {
                result: Some(merge_response::Result::Conflict(MergeConflict {
                    changeset_id: changeset_id.to_string(),
                    conflicts: conflict_details,
                    suggested_action,
                    available_actions,
                })),
            })
        }
    }
}

/// Release all symbol locks for a session and emit `symbol.lock.released` events
/// so blocked agents can wake up and retry their writes.
fn release_locks_and_emit(
    server: &ProtocolServer,
    repo_id: Uuid,
    session_id: Uuid,
    session_id_str: &str,
) {
    let released = server.claim_tracker().release_locks(repo_id, session_id);

    if released.is_empty() {
        return;
    }

    // Group released locks by file_path for efficient event emission
    let mut by_file: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for lock in &released {
        by_file
            .entry(lock.file_path.clone())
            .or_default()
            .push(lock.qualified_name.clone());
    }

    for (file_path, symbols) in by_file {
        server.event_bus().publish(crate::WatchEvent {
            event_type: EVENT_LOCK_RELEASED.to_string(),
            changeset_id: String::new(),
            agent_id: String::new(),
            affected_symbols: symbols,
            details: format!("Symbol locks released on {}", file_path),
            session_id: session_id_str.to_string(),
            affected_files: vec![crate::FileChange {
                path: file_path,
                operation: "unlock".to_string(),
            }],
            symbol_changes: vec![],
            repo_id: repo_id.to_string(),
            event_id: Uuid::new_v4().to_string(),
        });
    }
}

// ── Event type constants ────────────────────────────────────────────

/// Event published when a changeset is successfully merged.
pub const EVENT_MERGED: &str = "changeset.merged";

/// Event published when symbol locks are released (after merge, close, or timeout).
/// Blocked agents watch for this to retry their `dk_file_write`.
pub const EVENT_LOCK_RELEASED: &str = "symbol.lock.released";

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

    #[test]
    fn merge_success_construction() {
        let resp = MergeResponse {
            result: Some(merge_response::Result::Success(MergeSuccess {
                commit_hash: "abc123".to_string(),
                merged_version: "abc123".to_string(),
                auto_rebased: false,
                auto_rebased_files: Vec::new(),
            })),
        };
        match resp.result {
            Some(merge_response::Result::Success(s)) => {
                assert_eq!(s.commit_hash, "abc123");
                assert!(!s.auto_rebased);
                assert!(s.auto_rebased_files.is_empty());
            }
            _ => panic!("expected MergeSuccess"),
        }
    }

    #[test]
    fn merge_success_with_rebase() {
        let resp = MergeResponse {
            result: Some(merge_response::Result::Success(MergeSuccess {
                commit_hash: "def456".to_string(),
                merged_version: "def456".to_string(),
                auto_rebased: true,
                auto_rebased_files: vec!["src/main.rs".to_string()],
            })),
        };
        match resp.result {
            Some(merge_response::Result::Success(s)) => {
                assert!(s.auto_rebased);
                assert_eq!(s.auto_rebased_files, vec!["src/main.rs"]);
            }
            _ => panic!("expected MergeSuccess"),
        }
    }

    #[test]
    fn merge_conflict_construction() {
        // their_agent is currently not populated by the server (SemanticConflict
        // does not carry agent attribution yet), so the test mirrors real
        // behavior by using an empty string.
        let detail = ConflictDetail {
            file_path: "src/lib.rs".to_string(),
            symbols: vec!["process_data".to_string()],
            your_agent: "agent-1".to_string(),
            their_agent: String::new(),
            conflict_type: CONFLICT_TYPE_TRUE.to_string(),
            description: "both agents modified process_data".to_string(),
        };
        let resp = MergeResponse {
            result: Some(merge_response::Result::Conflict(MergeConflict {
                changeset_id: "cs-001".to_string(),
                conflicts: vec![detail],
                suggested_action: "adapt".to_string(),
                available_actions: vec![
                    "adapt".to_string(),
                    "keep_mine".to_string(),
                    "keep_theirs".to_string(),
                ],
            })),
        };
        match resp.result {
            Some(merge_response::Result::Conflict(c)) => {
                assert_eq!(c.changeset_id, "cs-001");
                assert_eq!(c.conflicts.len(), 1);
                assert_eq!(c.conflicts[0].file_path, "src/lib.rs");
                assert_eq!(c.conflicts[0].symbols, vec!["process_data"]);
                assert_eq!(c.conflicts[0].your_agent, "agent-1");
                assert!(c.conflicts[0].their_agent.is_empty());
                assert_eq!(c.suggested_action, "adapt");
                assert_eq!(c.available_actions.len(), 3);
            }
            _ => panic!("expected MergeConflict"),
        }
    }

    #[test]
    fn conflict_detail_fields() {
        let detail = ConflictDetail {
            file_path: "src/handler.rs".to_string(),
            symbols: vec!["handle_request".to_string(), "parse_input".to_string()],
            your_agent: "agent-a".to_string(),
            their_agent: "agent-b".to_string(),
            conflict_type: CONFLICT_TYPE_TRUE.to_string(),
            description: "multiple symbols in conflict".to_string(),
        };
        assert_eq!(detail.symbols.len(), 2);
        assert_eq!(detail.conflict_type, CONFLICT_TYPE_TRUE);
    }

    #[test]
    fn sanitize_for_proto_strips_null_bytes() {
        assert_eq!(sanitize_for_proto("hello\0world"), "helloworld");
        assert_eq!(sanitize_for_proto("\0\0"), "");
        assert_eq!(sanitize_for_proto("clean"), "clean");
    }

    #[test]
    fn sanitize_for_proto_preserves_valid_utf8() {
        // Multi-byte UTF-8 characters must survive sanitization
        assert_eq!(sanitize_for_proto("fn résumé()"), "fn résumé()");
        assert_eq!(sanitize_for_proto("日本語"), "日本語");
        // Replacement character from String::from_utf8_lossy is valid UTF-8
        assert_eq!(sanitize_for_proto("bad\u{FFFD}char"), "bad\u{FFFD}char");
    }
}

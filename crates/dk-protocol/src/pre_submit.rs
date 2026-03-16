use std::collections::HashMap;

use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::{PreSubmitCheckRequest, PreSubmitCheckResponse, SemanticConflict};

/// Handle a PreSubmitCheck RPC.
///
/// Performs a dry-run conflict detection:
/// 1. Retrieves the session workspace.
/// 2. Compares the workspace overlay against current HEAD.
/// 3. Uses the semantic conflict detector to find overlapping changes.
/// 4. Reports conflicts, file count, and symbol change count.
pub async fn handle_pre_submit_check(
    server: &ProtocolServer,
    req: PreSubmitCheckRequest,
) -> Result<Response<PreSubmitCheckResponse>, Status> {
    let session = server.validate_session(&req.session_id)?;

    let sid = req
        .session_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;
    server.session_mgr().touch_session(&sid);

    let engine = server.engine();

    // Get workspace
    let ws = engine
        .workspace_manager()
        .get_workspace(&sid)
        .ok_or_else(|| Status::not_found("Workspace not found for session"))?;

    // Get git repo to read HEAD
    let (_repo_id, git_repo) = engine
        .get_repo(&session.codebase)
        .await
        .map_err(|e| Status::internal(format!("Repo error: {e}")))?;

    let head_hash = git_repo
        .head_hash()
        .map_err(|e| Status::internal(format!("Failed to read HEAD: {e}")))?
        .unwrap_or_else(|| "initial".to_string());

    let overlay = ws.overlay_for_tree();
    let files_modified = overlay.len() as u32;
    let symbols_changed = ws.graph.change_count() as u32;

    // If HEAD == base_commit, no conflicts are possible (fast-forward path)
    if head_hash == ws.base_commit || overlay.is_empty() {
        info!(
            session_id = %req.session_id,
            files_modified,
            symbols_changed,
            "PRE_SUBMIT_CHECK: clean (fast-forward possible)"
        );

        return Ok(Response::new(PreSubmitCheckResponse {
            has_conflicts: false,
            potential_conflicts: Vec::new(),
            files_modified,
            symbols_changed,
        }));
    }

    // HEAD has advanced since workspace was created — check for conflicts.
    //
    // Batch-read all tree entries upfront so consecutive lookups against
    // the same commit let gitoxide reuse the resolved tree object from
    // its internal cache.
    let parser = engine.parser();
    let paths: Vec<&String> = overlay.iter().map(|(p, _)| p).collect();

    let mut base_entries: HashMap<&str, Option<Vec<u8>>> = HashMap::with_capacity(paths.len());
    for path in &paths {
        base_entries.insert(path.as_str(), git_repo.read_tree_entry(&ws.base_commit, path).ok());
    }

    let mut head_entries: HashMap<&str, Option<Vec<u8>>> = HashMap::with_capacity(paths.len());
    for path in &paths {
        head_entries.insert(path.as_str(), git_repo.read_tree_entry(&head_hash, path).ok());
    }

    let mut conflicts = Vec::new();

    for (path, maybe_content) in &overlay {
        let base_content = base_entries.get(path.as_str()).and_then(|v| v.as_ref());
        let head_content = head_entries.get(path.as_str()).and_then(|v| v.as_ref());

        match maybe_content {
            None => {
                // Deletion — check if HEAD also changed this file
                if let (Some(base), Some(head)) = (base_content, head_content) {
                    if base != head {
                        conflicts.push(SemanticConflict {
                            file_path: path.clone(),
                            symbol_name: "<entire file>".to_string(),
                            our_change: "deleted".to_string(),
                            their_change: "modified".to_string(),
                        });
                    }
                }
            }
            Some(overlay_content) => {
                match (base_content, head_content) {
                    (Some(base), Some(head)) => {
                        if base != head {
                            let analysis =
                                dk_engine::workspace::conflict::analyze_file_conflict(
                                    path,
                                    base,
                                    head,
                                    overlay_content,
                                    parser,
                                );

                            if let dk_engine::workspace::conflict::MergeAnalysis::Conflict {
                                conflicts: file_conflicts,
                            } = analysis
                            {
                                for c in file_conflicts {
                                    conflicts.push(SemanticConflict {
                                        file_path: c.file_path,
                                        symbol_name: c.symbol_name,
                                        our_change: format!("{:?}", c.our_change),
                                        their_change: format!("{:?}", c.their_change),
                                    });
                                }
                            }
                        }
                    }
                    (None, Some(head_blob)) => {
                        if *overlay_content != *head_blob {
                            conflicts.push(SemanticConflict {
                                file_path: path.clone(),
                                symbol_name: "<entire file>".to_string(),
                                our_change: "added".to_string(),
                                their_change: "added".to_string(),
                            });
                        }
                    }
                    (Some(_), None) => {
                        conflicts.push(SemanticConflict {
                            file_path: path.clone(),
                            symbol_name: "<entire file>".to_string(),
                            our_change: "modified".to_string(),
                            their_change: "deleted".to_string(),
                        });
                    }
                    (None, None) => {
                        // Pure addition, no conflict
                    }
                }
            }
        }
    }

    let has_conflicts = !conflicts.is_empty();

    info!(
        session_id = %req.session_id,
        has_conflicts,
        conflict_count = conflicts.len(),
        files_modified,
        symbols_changed,
        "PRE_SUBMIT_CHECK: completed"
    );

    Ok(Response::new(PreSubmitCheckResponse {
        has_conflicts,
        potential_conflicts: conflicts,
        files_modified,
        symbols_changed,
    }))
}

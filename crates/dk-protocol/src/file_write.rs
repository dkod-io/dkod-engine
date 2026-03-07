use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::validation::{validate_file_path, MAX_FILE_SIZE};
use crate::{FileWriteRequest, FileWriteResponse, SymbolChange};

/// Handle a FileWrite RPC.
///
/// Writes a file through the session workspace overlay and optionally
/// detects symbol changes by parsing the new content.
pub async fn handle_file_write(
    server: &ProtocolServer,
    req: FileWriteRequest,
) -> Result<Response<FileWriteResponse>, Status> {
    validate_file_path(&req.path)?;

    if req.content.len() > MAX_FILE_SIZE {
        return Err(Status::invalid_argument("file content exceeds 50MB limit"));
    }

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

    // Determine if the file is new (not in base tree) synchronously,
    // then drop the git_repo before async work to keep future Send.
    // Capture repo_id here to avoid a redundant second get_repo call later.
    let (repo_id, is_new) = {
        let (rid, git_repo) = engine
            .get_repo(&session.codebase)
            .await
            .map_err(|e| Status::internal(format!("Repo error: {e}")))?;
        let new = git_repo
            .read_tree_entry(&ws.base_commit, &req.path)
            .is_err();
        (rid, new)
    };
    let repo_id_str = repo_id.to_string();

    // Capture pre-write symbols for accurate change detection
    let pre_write_symbols: std::collections::HashSet<String> = engine
        .symbol_store()
        .find_by_file(repo_id, &req.path)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.qualified_name)
        .collect();

    // Write through the overlay (async DB persist)
    let new_hash = ws
        .overlay
        .write(&req.path, req.content.clone(), is_new)
        .await
        .map_err(|e| Status::internal(format!("Write failed: {e}")))?;

    let changeset_id = ws.changeset_id;

    // Drop workspace guard before further work
    drop(ws);

    // Also record in changeset_files so the verify pipeline can materialize them.
    let op = if is_new { "add" } else { "modify" };
    let content_str = std::str::from_utf8(&req.content).ok();
    let _ = engine
        .changeset_store()
        .upsert_file(changeset_id, &req.path, op, content_str)
        .await;

    // Attempt to detect symbol changes by parsing the new content
    let detected_changes = detect_symbol_changes(engine, &req.path, &req.content);

    // Build symbol change details with accurate change types by comparing
    // against pre-write symbols.
    let symbol_changes: Vec<crate::SymbolChangeDetail> = detected_changes
        .iter()
        .map(|sc| {
            let change_type = if is_new || !pre_write_symbols.contains(&sc.symbol_name) {
                "added"
            } else {
                "modified"
            };
            crate::SymbolChangeDetail {
                symbol_name: sc.symbol_name.clone(),
                file_path: req.path.clone(),
                change_type: change_type.to_string(),
                kind: sc.change_type.clone(),
            }
        })
        .collect();

    // Detect deleted symbols (existed before but no longer present)
    let detected_names: std::collections::HashSet<&str> = detected_changes
        .iter()
        .map(|sc| sc.symbol_name.as_str())
        .collect();
    let mut all_symbol_changes = symbol_changes;
    for name in &pre_write_symbols {
        if !detected_names.contains(name.as_str()) {
            all_symbol_changes.push(crate::SymbolChangeDetail {
                symbol_name: name.clone(),
                file_path: req.path.clone(),
                change_type: "deleted".to_string(),
                kind: String::new(),
            });
        }
    }

    // Emit a file.modified (or file.added) event
    let event_type = if is_new { "file.added" } else { "file.modified" };
    server.event_bus().publish(crate::WatchEvent {
        event_type: event_type.to_string(),
        changeset_id: changeset_id.to_string(),
        agent_id: session.agent_id.clone(),
        affected_symbols: vec![],
        details: format!("file {}: {}", op, req.path),
        session_id: req.session_id.clone(),
        affected_files: vec![crate::FileChange {
            path: req.path.clone(),
            operation: op.to_string(),
        }],
        symbol_changes: all_symbol_changes,
        repo_id: repo_id_str,
        event_id: uuid::Uuid::new_v4().to_string(),
    });

    info!(
        session_id = %req.session_id,
        path = %req.path,
        hash = %new_hash,
        changes = detected_changes.len(),
        "FILE_WRITE: completed"
    );

    Ok(Response::new(FileWriteResponse {
        new_hash,
        detected_changes,
    }))
}

/// Parse the file content and detect symbol-level changes.
///
/// This is best-effort: if the parser doesn't support the file type
/// or parsing fails, we return an empty list.
fn detect_symbol_changes(
    engine: &dk_engine::repo::Engine,
    path: &str,
    content: &[u8],
) -> Vec<SymbolChange> {
    let file_path = std::path::Path::new(path);
    let parser = engine.parser();

    if !parser.supports_file(file_path) {
        return Vec::new();
    }

    match parser.parse_file(file_path, content) {
        Ok(analysis) => analysis
            .symbols
            .iter()
            .map(|sym| SymbolChange {
                symbol_name: sym.qualified_name.clone(),
                change_type: sym.kind.to_string(),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

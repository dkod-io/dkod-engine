use std::time::Instant;

use tonic::{Response, Status};
use tracing::{info, warn};

use dk_engine::conflict::SymbolClaim;
use crate::server::ProtocolServer;
use crate::validation::{validate_file_path, MAX_FILE_SIZE};
use crate::{ConflictWarning, FileWriteRequest, FileWriteResponse, SymbolChange};

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

    // Determine if the file is new (not in base tree) and read old content
    // in a single get_repo call. Drop git_repo before async work to keep
    // future Send.
    let (repo_id, is_new, old_content) = {
        let (rid, git_repo, _) = engine
            .get_repo(&session.codebase, None)
            .await
            .map_err(|e| Status::internal(format!("Repo error: {e}")))?;
        match git_repo.read_tree_entry(&ws.base_commit, &req.path) {
            Ok(bytes) => (rid, false, bytes),
            Err(e) => {
                // File not in base tree — treat as new. Log the error in case
                // it's a transient git failure rather than a genuine "not found".
                warn!(
                    path = %req.path,
                    base_commit = %ws.base_commit,
                    error = %e,
                    "read_tree_entry failed — treating file as new"
                );
                (rid, true, Vec::new())
            }
        }
    };
    let repo_id_str = repo_id.to_string();

    // Write through the overlay (async DB persist)
    let new_hash = ws
        .overlay
        .write(&req.path, req.content.clone(), is_new)
        .await
        .map_err(|e| Status::internal(format!("Write failed: {e}")))?;

    let changeset_id = ws.changeset_id;
    let agent_name = ws.agent_name.clone();

    // Drop workspace guard before further work
    drop(ws);

    // Also record in changeset_files so the verify pipeline can materialize them.
    let op = if is_new { "add" } else { "modify" };
    let content_str = std::str::from_utf8(&req.content).ok();
    let _ = engine
        .changeset_store()
        .upsert_file(changeset_id, &req.path, op, content_str)
        .await;

    // Detect symbol changes by diffing old vs new file content.
    // Only symbols whose source text actually changed are reported.
    let (detected_changes, all_symbol_changes) =
        detect_symbol_changes_diffed(engine, &req.path, &old_content, &req.content, is_new);

    // ── Symbol claim tracking ──
    // Build claims from "added" and "modified" symbol changes and check for
    // cross-session conflicts. Two sessions modifying DIFFERENT symbols in the
    // same file is NOT a conflict — only same-symbol is a true conflict.
    let conflict_warnings = {
        let claimable: Vec<&crate::SymbolChangeDetail> = all_symbol_changes
            .iter()
            .filter(|sc| sc.change_type == "added" || sc.change_type == "modified")
            .collect();

        // Check for conflicts before recording our claims
        let qualified_names: Vec<String> = claimable.iter().map(|sc| sc.symbol_name.clone()).collect();
        let conflicts = server.claim_tracker().check_conflicts(
            repo_id,
            &req.path,
            sid,
            &qualified_names,
        );

        // Record claims (even if conflicts exist — warning only at write time)
        for sc in &claimable {
            let kind = sc.kind.parse::<dk_core::SymbolKind>().unwrap_or(dk_core::SymbolKind::Function);
            server.claim_tracker().record_claim(
                repo_id,
                &req.path,
                SymbolClaim {
                    session_id: sid,
                    agent_name: agent_name.clone(),
                    qualified_name: sc.symbol_name.clone(),
                    kind,
                    first_touched_at: Instant::now(),
                },
            );
        }

        // Build ConflictWarning proto messages
        let warnings: Vec<ConflictWarning> = conflicts
            .into_iter()
            .map(|c| {
                let msg = format!(
                    "Symbol '{}' was already modified by agent '{}' (session {})",
                    c.qualified_name, c.conflicting_agent, c.conflicting_session,
                );
                warn!(
                    session_id = %sid,
                    path = %req.path,
                    symbol = %c.qualified_name,
                    conflicting_agent = %c.conflicting_agent,
                    "CONFLICT_WARNING: {msg}"
                );
                ConflictWarning {
                    file_path: req.path.clone(),
                    symbol_name: c.qualified_name,
                    conflicting_agent: c.conflicting_agent,
                    conflicting_session_id: c.conflicting_session.to_string(),
                    message: msg,
                }
            })
            .collect();
        warnings
    };

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
        conflicts = conflict_warnings.len(),
        "FILE_WRITE: completed"
    );

    Ok(Response::new(FileWriteResponse {
        new_hash,
        detected_changes,
        conflict_warnings,
    }))
}

/// Parse both old and new file content, diff per-symbol source text,
/// and return only symbols that actually changed.
///
/// Returns `(detected_changes, all_symbol_change_details)`:
/// - `detected_changes`: `SymbolChange` for the gRPC response (only truly changed symbols)
/// - `all_symbol_change_details`: `SymbolChangeDetail` for claims + events (added/modified/deleted)
fn detect_symbol_changes_diffed(
    engine: &dk_engine::repo::Engine,
    path: &str,
    old_content: &[u8],
    new_content: &[u8],
    is_new_file: bool,
) -> (Vec<SymbolChange>, Vec<crate::SymbolChangeDetail>) {
    let file_path = std::path::Path::new(path);
    let parser = engine.parser();

    if !parser.supports_file(file_path) {
        return (Vec::new(), Vec::new());
    }

    // Parse new file
    let new_symbols = match parser.parse_file(file_path, new_content) {
        Ok(analysis) => analysis.symbols,
        Err(_) => return (Vec::new(), Vec::new()),
    };

    // If file is new, all symbols are "added"
    if is_new_file || old_content.is_empty() {
        let changes: Vec<SymbolChange> = new_symbols
            .iter()
            .map(|sym| SymbolChange {
                symbol_name: sym.qualified_name.clone(),
                change_type: sym.kind.to_string(),
            })
            .collect();
        let details: Vec<crate::SymbolChangeDetail> = new_symbols
            .iter()
            .map(|sym| crate::SymbolChangeDetail {
                symbol_name: sym.qualified_name.clone(),
                file_path: path.to_string(),
                change_type: "added".to_string(),
                kind: sym.kind.to_string(),
            })
            .collect();
        return (changes, details);
    }

    // Parse old file to get baseline symbols
    let old_symbols = match parser.parse_file(file_path, old_content) {
        Ok(analysis) => analysis.symbols,
        Err(_) => {
            // Can't parse old file — fall back to treating all new symbols as modified
            let changes: Vec<SymbolChange> = new_symbols
                .iter()
                .map(|sym| SymbolChange {
                    symbol_name: sym.qualified_name.clone(),
                    change_type: sym.kind.to_string(),
                })
                .collect();
            let details: Vec<crate::SymbolChangeDetail> = new_symbols
                .iter()
                .map(|sym| crate::SymbolChangeDetail {
                    symbol_name: sym.qualified_name.clone(),
                    file_path: path.to_string(),
                    change_type: "modified".to_string(),
                    kind: sym.kind.to_string(),
                })
                .collect();
            return (changes, details);
        }
    };

    // Build a map of old symbol qualified_name → source text.
    // Use entry().or_insert() to keep the first occurrence when duplicate
    // qualified names exist (e.g., overloaded methods in Java/Kotlin/C#).
    let mut old_symbol_text: std::collections::HashMap<&str, &[u8]> = std::collections::HashMap::new();
    for sym in &old_symbols {
        let start = sym.span.start_byte as usize;
        let end = sym.span.end_byte as usize;
        if start <= end && end <= old_content.len() {
            old_symbol_text.entry(sym.qualified_name.as_str()).or_insert(&old_content[start..end]);
        }
    }

    let mut detected_changes = Vec::new();
    let mut all_details = Vec::new();

    // Deduplicate new symbols while preserving original parse order.
    let mut seen_new: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Compare each deduplicated new symbol against its old version
    for sym in &new_symbols {
        if !seen_new.insert(sym.qualified_name.as_str()) {
            continue; // duplicate qualified name — already handled
        }
        let start = sym.span.start_byte as usize;
        let end = sym.span.end_byte as usize;
        let new_text = if start <= end && end <= new_content.len() {
            &new_content[start..end]
        } else {
            continue; // invalid or inverted span, skip
        };

        match old_symbol_text.get(sym.qualified_name.as_str()) {
            None => {
                // Symbol not in old file — added
                detected_changes.push(SymbolChange {
                    symbol_name: sym.qualified_name.clone(),
                    change_type: sym.kind.to_string(),
                });
                all_details.push(crate::SymbolChangeDetail {
                    symbol_name: sym.qualified_name.clone(),
                    file_path: path.to_string(),
                    change_type: "added".to_string(),
                    kind: sym.kind.to_string(),
                });
            }
            Some(old_text) => {
                if *old_text != new_text {
                    // Symbol text changed — modified
                    detected_changes.push(SymbolChange {
                        symbol_name: sym.qualified_name.clone(),
                        change_type: sym.kind.to_string(),
                    });
                    all_details.push(crate::SymbolChangeDetail {
                        symbol_name: sym.qualified_name.clone(),
                        file_path: path.to_string(),
                        change_type: "modified".to_string(),
                        kind: sym.kind.to_string(),
                    });
                }
                // else: symbol text identical — skip (no claim needed)
            }
        }
    }

    // Detect deleted symbols (deduplicated to avoid double-reporting overloads)
    let new_names: std::collections::HashSet<&str> = new_symbols
        .iter()
        .map(|s| s.qualified_name.as_str())
        .collect();
    let old_names: std::collections::HashSet<&str> = old_symbols
        .iter()
        .map(|s| s.qualified_name.as_str())
        .collect();
    for old_name in &old_names {
        if !new_names.contains(old_name) {
            if let Some(old_sym) = old_symbols.iter().find(|s| s.qualified_name.as_str() == *old_name) {
                all_details.push(crate::SymbolChangeDetail {
                    symbol_name: old_sym.qualified_name.clone(),
                    file_path: path.to_string(),
                    change_type: "deleted".to_string(),
                    kind: old_sym.kind.to_string(),
                });
            }
        }
    }

    (detected_changes, all_details)
}

use std::path::PathBuf;

use tonic::{Response, Status};
use tracing::{info, warn};

use dk_engine::workspace::overlay::OverlayEntry;

use crate::file_write::release_on_submit_enabled;
use crate::merge::release_locks_and_emit;
use crate::server::ProtocolServer;
use crate::validation::validate_file_path;
use crate::{ChangeType, SubmitError, SubmitRequest, SubmitResponse, SubmitStatus};

/// Handle a SUBMIT RPC.
///
/// 1. Validates the session.
/// 2. Resolves the repo to obtain the working directory path.
/// 3. Applies file-level writes through the session workspace overlay.
/// 4. Re-opens the repo and re-indexes changed files through the engine.
/// 5. Returns ACCEPTED with a new changeset ID, or REJECTED with errors.
pub async fn handle_submit(
    server: &ProtocolServer,
    req: SubmitRequest,
) -> Result<Response<SubmitResponse>, Status> {
    // 1. Validate session
    let session = server.validate_session(&req.session_id)?;

    // Validate all file paths before any processing
    for change in &req.changes {
        validate_file_path(&change.file_path)?;
    }

    let sid = req
        .session_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;
    server.session_mgr().touch_session(&sid);

    // 2. Resolve repo — extract work_dir, repo_id, and file-existence checks
    //    in a single get_repo call. The `GitRepository` (gix::Repository is
    //    !Sync) is dropped before any subsequent .await.
    let engine = server.engine();

    // Parse changeset_id from request
    let changeset_id = req.changeset_id.parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("invalid changeset_id"))?;

    // Get workspace for this session
    let ws = engine
        .workspace_manager()
        .get_workspace(&sid)
        .ok_or_else(|| Status::not_found("Workspace not found for session"))?;

    let base_commit = ws.base_commit.clone();

    // Single get_repo call: extract work_dir and pre-compute is_new for each file
    let (repo_id, work_dir, file_checks) = {
        let (repo_id, git_repo) = engine
            .get_repo(&session.codebase)
            .await
            .map_err(|e| Status::internal(format!("Repo error: {e}")))?;

        let work_dir = git_repo.path().to_path_buf();

        let checks: Vec<(&crate::Change, bool)> = req
            .changes
            .iter()
            .map(|change| {
                let exists_in_base = git_repo
                    .read_tree_entry(&base_commit, &change.file_path)
                    .is_ok();
                (change, exists_in_base)
            })
            .collect();

        (repo_id, work_dir, checks)
        // git_repo is dropped here
    };

    // Snapshot the existing symbols per file (file_path -> (qualified_name -> id))
    // for files that will be changed.  After re-indexing we compare by ID so that
    // modifications (same name, new UUID) are still detected.
    let mut pre_submit_symbols: std::collections::HashMap<String, std::collections::HashMap<String, uuid::Uuid>> = {
        let mut file_syms = std::collections::HashMap::new();
        for change in &req.changes {
            let entry: &mut std::collections::HashMap<String, uuid::Uuid> = file_syms.entry(change.file_path.clone()).or_default();
            if let Ok(symbols) = engine.symbol_store().find_by_file(repo_id, &change.file_path).await {
                for sym in symbols {
                    entry.insert(sym.qualified_name, sym.id);
                }
            }
        }
        file_syms
    };

    // Snapshot overlay early so we can reuse it for both pre_submit_symbols
    // (MCP path) and later for changeset_files, avoiding a redundant call.
    let early_overlay_snapshot = if req.changes.is_empty() {
        let snap = ws.overlay.list_changes();
        // Populate pre_submit_symbols from overlay paths so symbol diffs are accurate.
        for (path, _) in &snap {
            let entry = pre_submit_symbols.entry(path.clone()).or_default();
            if let Ok(symbols) = engine.symbol_store().find_by_file(repo_id, path).await {
                for sym in symbols {
                    entry.insert(sym.qualified_name, sym.id);
                }
            }
        }
        Some(snap)
    } else {
        None
    };

    let mut errors = Vec::new();
    let mut changed_files = Vec::new();

    // 3. Apply each change through the session workspace overlay.
    for (change, exists_in_base) in &file_checks {
        match change.r#type() {
            ChangeType::ModifyFunction | ChangeType::ModifyType => {
                // Target file must already exist in base or overlay
                let in_overlay = ws.overlay.contains(&change.file_path);
                if !exists_in_base && !in_overlay {
                    errors.push(SubmitError {
                        message: format!("File not found: {}", change.file_path),
                        symbol_id: change.old_symbol_id.clone(),
                        file_path: Some(change.file_path.clone()),
                    });
                    continue;
                }
                let is_new = !exists_in_base;
                if let Err(e) = ws
                    .overlay
                    .write(&change.file_path, change.new_source.as_bytes().to_vec(), is_new)
                    .await
                {
                    errors.push(SubmitError {
                        message: format!("Write failed: {e}"),
                        symbol_id: None,
                        file_path: Some(change.file_path.clone()),
                    });
                    continue;
                }
                changed_files.push(PathBuf::from(&change.file_path));
            }

            ChangeType::AddFunction | ChangeType::AddType | ChangeType::AddDependency => {
                let is_new = !exists_in_base;
                if let Err(e) = ws
                    .overlay
                    .write(&change.file_path, change.new_source.as_bytes().to_vec(), is_new)
                    .await
                {
                    errors.push(SubmitError {
                        message: format!("Write failed: {e}"),
                        symbol_id: None,
                        file_path: Some(change.file_path.clone()),
                    });
                    continue;
                }
                changed_files.push(PathBuf::from(&change.file_path));
            }

            ChangeType::DeleteFunction => {
                // For deletes we track the file as changed so the engine
                // can re-index it (the function body will have been removed
                // from the source by the agent).
                changed_files.push(PathBuf::from(&change.file_path));
            }
        }
    }

    // Reuse early snapshot if available (MCP path), otherwise take it now.
    let overlay_snapshot = early_overlay_snapshot.unwrap_or_else(|| ws.overlay.list_changes());

    // Reject empty changesets — there must be at least one file modification.
    if overlay_snapshot.is_empty() && changed_files.is_empty() && errors.is_empty() {
        warn!(
            session_id = %req.session_id,
            "SUBMIT: rejected — no file changes in overlay"
        );
        return Ok(Response::new(SubmitResponse {
            status: SubmitStatus::Rejected.into(),
            changeset_id: String::new(),
            new_version: None,
            errors: vec![SubmitError {
                message: "No changes to submit".to_string(),
                symbol_id: None,
                file_path: None,
            }],
            conflict_block: None,
            review_summary: None,
        }));
    }

    // Drop the workspace guard before further async work
    drop(ws);

    // Record file changes in changeset — always use the overlay as the
    // single source of truth.  This unifies the "standard" path (changes
    // sent inline via req.changes) and the "MCP" path (files written
    // earlier via dk_file_write).  The overlay is session-scoped, so we
    // only capture files belonging to this session.
    for (path, entry) in &overlay_snapshot {
        let (op, content) = match entry {
            OverlayEntry::Added { content, .. } => {
                ("add", Some(String::from_utf8_lossy(content).into_owned()))
            }
            OverlayEntry::Modified { content, .. } => {
                ("modify", Some(String::from_utf8_lossy(content).into_owned()))
            }
            OverlayEntry::Deleted => ("delete", None),
        };
        engine.changeset_store()
            .upsert_file(changeset_id, path, op, content.as_deref())
            .await
            .map_err(|e| Status::internal(format!("changeset file record failed: {e}")))?;
        // Ensure MCP-path files end up in changed_files for re-indexing
        if !changed_files.iter().any(|p| p.to_string_lossy() == *path) {
            changed_files.push(PathBuf::from(path));
        }
    }

    // If any change failed, reject the whole submission.
    if !errors.is_empty() {
        warn!(
            session_id = %req.session_id,
            error_count = errors.len(),
            "SUBMIT: rejected with errors"
        );
        return Ok(Response::new(SubmitResponse {
            status: SubmitStatus::Rejected.into(),
            changeset_id: String::new(),
            new_version: None,
            errors,
            conflict_block: None,
            review_summary: None,
        }));
    }

    // 4. Re-index changed files through the semantic graph.
    //    Use `update_files_by_root` which takes a `&Path` instead of
    //    `&GitRepository` (the latter is !Sync and cannot cross .await).
    if let Err(e) = engine
        .update_files_by_root(repo_id, &work_dir, &changed_files)
        .await
    {
        return Ok(Response::new(SubmitResponse {
            status: SubmitStatus::Rejected.into(),
            changeset_id: String::new(),
            new_version: None,
            errors: vec![SubmitError {
                message: format!("Re-indexing failed: {e}"),
                symbol_id: None,
                file_path: None,
            }],
            conflict_block: None,
            review_summary: None,
        }));
    }

    // Record only NEW or CHANGED symbols in the changeset.
    // A symbol is "affected" if:
    //   (a) its qualified_name did not exist before (new symbol), OR
    //   (b) its qualified_name existed but its UUID changed after re-index
    //       (the symbol was modified -- see symbols.rs ON CONFLICT ... SET id).
    for file_path in &changed_files {
        let rel_str = file_path.to_string_lossy().to_string();
        let file_pre_syms = pre_submit_symbols.get(&rel_str);
        if let Ok(new_symbols) = engine.symbol_store().find_by_file(repo_id, &rel_str).await {
            for sym in &new_symbols {
                // Record if the symbol is new OR its ID changed (modified).
                let unchanged = file_pre_syms
                    .and_then(|m| m.get(&sym.qualified_name))
                    .is_some_and(|old_id| *old_id == sym.id);
                if !unchanged {
                    let _ = engine.changeset_store()
                        .record_affected_symbol(changeset_id, sym.id, &sym.qualified_name)
                        .await;
                }
            }
        }
    }

    // Update changeset status to "submitted"
    engine.changeset_store().update_status(changeset_id, "submitted").await
        .map_err(|e| Status::internal(format!("changeset status update failed: {e}")))?;

    // Build affected_files list from overlay (unified source of truth)
    let affected_files: Vec<crate::FileChange> = overlay_snapshot.iter().map(|(path, entry)| {
        let operation = match entry {
            OverlayEntry::Added { .. } => "add",
            OverlayEntry::Modified { .. } => "modify",
            OverlayEntry::Deleted => "delete",
        };
        crate::FileChange {
            path: path.clone(),
            operation: operation.to_string(),
        }
    }).collect();

    // Build symbol_changes from pre/post symbol comparison
    let mut symbol_changes: Vec<crate::SymbolChangeDetail> = Vec::new();
    for file_path in &changed_files {
        let rel_str = file_path.to_string_lossy().to_string();
        let file_pre_syms = pre_submit_symbols.get(&rel_str);
        if let Ok(new_symbols) = engine.symbol_store().find_by_file(repo_id, &rel_str).await {
            // Detect added/modified symbols
            for sym in &new_symbols {
                let change_type = match file_pre_syms.and_then(|m| m.get(&sym.qualified_name)) {
                    Some(old_id) if *old_id == sym.id => continue, // unchanged
                    Some(_) => "modified",
                    None => "added",
                };
                symbol_changes.push(crate::SymbolChangeDetail {
                    symbol_name: sym.qualified_name.clone(),
                    file_path: rel_str.clone(),
                    change_type: change_type.to_string(),
                    kind: sym.kind.to_string(),
                });
            }
            // Detect deleted symbols: only check symbols that belonged to THIS file
            if let Some(old_syms) = file_pre_syms {
                for name in old_syms.keys() {
                    let still_exists = new_symbols.iter().any(|s| s.qualified_name == *name);
                    if !still_exists {
                        symbol_changes.push(crate::SymbolChangeDetail {
                            symbol_name: name.clone(),
                            file_path: rel_str.clone(),
                            change_type: "deleted".to_string(),
                            kind: String::new(),
                        });
                    }
                }
            }
        }
    }

    // Publish event
    server.event_bus().publish(crate::WatchEvent {
        event_type: "changeset.submitted".to_string(),
        changeset_id: changeset_id.to_string(),
        agent_id: session.agent_id.clone(),
        affected_symbols: vec![],
        details: req.intent.clone(),
        session_id: req.session_id.clone(),
        affected_files,
        symbol_changes,
        repo_id: repo_id.to_string(),
        event_id: uuid::Uuid::new_v4().to_string(),
    });

    // ── Release-on-submit (DKOD_RELEASE_ON_SUBMIT) ──
    // Locks historically lived until `dk_merge`, which is 1–5 minutes after
    // this point when DKOD_CODE_REVIEW + the LAND fix-loop are enabled.
    // Releasing here collapses the hold window from minutes to the few
    // milliseconds between submit and the blocked waiter's next
    // `dk_file_write`. The idempotent call in `handle_merge` still runs,
    // so crashed-before-merge sessions don't leave stranded locks.
    //
    // Gated so PR1 ships flag-off and the testbed can flip it only after
    // the STALE_OVERLAY backstop counter stays at zero.
    if release_on_submit_enabled() {
        let n = release_locks_and_emit(
            server,
            repo_id,
            sid,
            &req.session_id,
            &changeset_id.to_string(),
        )
        .await;
        if n > 0 {
            info!(
                session_id = %req.session_id,
                changeset_id = %changeset_id,
                symbols = n,
                "lock released on submit"
            );
            crate::metrics::incr_locks_released_on_submit(n as u64);
        }
    }

    // Read HEAD version without holding the GitRepository across awaits.
    let new_version = {
        let (_repo_id, git_repo) = engine
            .get_repo(&session.codebase)
            .await
            .map_err(|e| Status::internal(format!("Repo error (head read): {e}")))?;
        git_repo
            .head_hash()
            .ok()
            .flatten()
            .unwrap_or_else(|| "pending".to_string())
    };

    // 5. Return ACCEPTED with the changeset ID from the request.
    info!(
        session_id = %req.session_id,
        changeset_id = %changeset_id,
        files_changed = changed_files.len(),
        "SUBMIT: accepted"
    );

    Ok(Response::new(SubmitResponse {
        status: SubmitStatus::Accepted.into(),
        changeset_id: changeset_id.to_string(),
        new_version: Some(new_version),
        errors: vec![],
        conflict_block: None,
        review_summary: None,
    }))
}

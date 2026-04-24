use tonic::{Response, Status};
use tracing::{info, warn};

use dk_engine::workspace::session_manager::ResumeResult;
use dk_engine::workspace::session_workspace::WorkspaceMode;

use crate::server::ProtocolServer;
use crate::{
    ActiveSessionSummary, CodebaseSummary, ConnectRequest, ConnectResponse,
    WorkspaceConcurrencyInfo,
};

/// Handle a CONNECT RPC.
///
/// 1. Validates the bearer token.
/// 2. Looks up the repository by name.
/// 3. Retrieves a high-level codebase summary (languages, symbol count, file count).
/// 4. Reads the HEAD commit hash as the current codebase version.
/// 5. Creates a stateful session and returns the session ID.
/// 6. Creates a session workspace (isolated overlay for file changes).
/// 7. Returns workspace ID and concurrency info.
pub async fn handle_connect(
    server: &ProtocolServer,
    req: ConnectRequest,
) -> Result<Response<ConnectResponse>, Status> {
    // 1. Auth
    let authed_agent_id = server.validate_auth(&req.auth_token)?;

    // Check for session resume.
    //
    // `take_snapshot` consumes the snapshot so it cannot be reused by a stale
    // reconnect.  When a valid snapshot is found, its `codebase_version` is
    // used as the default base_commit so the resumed session starts from the
    // same commit the old session was on.
    let mut resumed_snapshot: Option<crate::session::SessionSnapshot> = None;
    if let Some(ref ws_config) = req.workspace_config {
        if let Some(ref resume_id_str) = ws_config.resume_session_id {
            match resume_id_str.parse::<uuid::Uuid>() {
                Ok(resume_id) => {
                    match server.session_mgr().take_snapshot(&resume_id) {
                        Some(snapshot) => {
                            if snapshot.codebase != req.codebase {
                                return Err(Status::invalid_argument(format!(
                                    "Cannot resume session from codebase '{}' into '{}'",
                                    snapshot.codebase, req.codebase
                                )));
                            }
                            info!(
                                resume_from = %resume_id,
                                agent_id = %snapshot.agent_id,
                                base_version = %snapshot.codebase_version,
                                "CONNECT: resuming from previous session snapshot"
                            );
                            resumed_snapshot = Some(snapshot);
                        }
                        None => {
                            warn!(
                                resume_session_id = %resume_id,
                                "CONNECT: resume snapshot not found; trying stranded workspace rehydrate"
                            );
                            // Fall through to rehydrate path below (keep resumed_snapshot as None).
                        }
                    }
                }
                Err(_) => {
                    return Err(Status::invalid_argument(format!(
                        "resume_session_id '{}' is not a valid UUID",
                        resume_id_str
                    )));
                }
            }
        }
    }

    // Epic B: stranded-workspace rehydrate fallback.
    // Runs when snapshot-resume yielded nothing, but resume_session_id points to
    // a stranded workspace row. Uses WorkspaceManager::resume.
    if resumed_snapshot.is_none() {
        if let Some(ref ws_config) = req.workspace_config {
            if let Some(ref resume_id_str) = ws_config.resume_session_id {
                if let Ok(dead) = resume_id_str.parse::<uuid::Uuid>() {
                    let new_sid = uuid::Uuid::new_v4();
                    // Use the JWT-validated identity rather than the client-supplied
                    // agent_id to prevent a client from claiming another agent's session.
                    let agent_id = authed_agent_id.clone();
                    let mgr = server.engine().workspace_manager();
                    match mgr.resume(&dead, new_sid, &agent_id).await {
                        Ok(ResumeResult::Ok(_)) => {
                            let ws = mgr.get_workspace(&new_sid).ok_or_else(|| {
                                Status::internal("rehydrated workspace not found")
                            })?;
                            // Epic B: reject cross-codebase resume. Resolve req.codebase and
                            // compare to the rehydrated workspace's repo_id so an agent cannot
                            // resume a stranded workspace that belongs to a different repository.
                            let (expected_repo_id, _git) =
                                server.engine().get_repo(&req.codebase).await.map_err(|e| {
                                    Status::invalid_argument(format!("codebase lookup failed: {e}"))
                                })?;
                            if ws.repo_id != expected_repo_id {
                                return Err(Status::invalid_argument(
                                    "Cannot resume stranded workspace from a different codebase"
                                        .to_string(),
                                ));
                            }
                            let base_commit = ws.base_commit.clone();
                            let changeset_id = ws.changeset_id;
                            let workspace_id = ws.id;
                            info!(
                                resume_from = %dead,
                                new_session_id = %new_sid,
                                workspace_id = %workspace_id,
                                "CONNECT: rehydrated stranded workspace"
                            );
                            return Ok(Response::new(ConnectResponse {
                                session_id: new_sid.to_string(),
                                codebase_version: base_commit,
                                changeset_id: changeset_id.to_string(),
                                workspace_id: workspace_id.to_string(),
                                ..Default::default()
                            }));
                        }
                        Ok(ResumeResult::Contended(syms)) => {
                            let mut st = Status::failed_precondition("resume contended");
                            st.metadata_mut().insert(
                                "dk-error",
                                tonic::metadata::MetadataValue::try_from("RESUME_CONTENDED")
                                    .expect("static ascii"),
                            );
                            let json_syms: Vec<serde_json::Value> = syms
                                .iter()
                                .map(|s| {
                                    serde_json::json!({
                                        "qualified_name": s.qualified_name,
                                        "file_path": s.file_path,
                                        "claimant_session": s.claimant_session.to_string(),
                                        "claimant_agent": s.claimant_agent,
                                    })
                                })
                                .collect();
                            if let Ok(json) = serde_json::to_string(&json_syms) {
                                if let Ok(mv) =
                                    tonic::metadata::MetadataValue::try_from(json.as_str())
                                {
                                    st.metadata_mut().insert("dk-conflicting-symbols", mv);
                                }
                            }
                            return Err(st);
                        }
                        Ok(ResumeResult::AlreadyResumed(resumed_sid)) => {
                            let mut st = Status::already_exists("already resumed");
                            st.metadata_mut().insert(
                                "dk-error",
                                tonic::metadata::MetadataValue::try_from("ALREADY_RESUMED")
                                    .expect("static"),
                            );
                            st.metadata_mut().insert(
                                "dk-new-session-id",
                                tonic::metadata::MetadataValue::try_from(
                                    resumed_sid.to_string().as_str(),
                                )
                                .expect("uuid is ascii"),
                            );
                            return Err(st);
                        }
                        Ok(ResumeResult::Abandoned) => {
                            let mut st = Status::failed_precondition("session abandoned");
                            st.metadata_mut().insert(
                                "dk-error",
                                tonic::metadata::MetadataValue::try_from("SESSION_ABANDONED")
                                    .expect("static"),
                            );
                            return Err(st);
                        }
                        Ok(ResumeResult::NotStranded) => {
                            warn!(
                                resume_session_id = %dead,
                                "CONNECT: resume requested but workspace not stranded \
                                 — falling through to new session"
                            );
                        }
                        Err(e) => return Err(Status::internal(e.to_string())),
                    }
                }
            }
        }
    }

    // Extract the requested base_commit early so it can be validated during
    // the initial repo lookup (avoids a redundant `get_repo` call later).
    // If resuming, default to the snapshot's codebase_version so the
    // workspace starts from the same commit the old session was on.
    let requested_base_commit = req
        .workspace_config
        .as_ref()
        .and_then(|c| c.base_commit.clone())
        .or_else(|| {
            resumed_snapshot
                .as_ref()
                .map(|s| s.codebase_version.clone())
        });

    // 2-4. Resolve repo, get summary, read HEAD version, and validate
    //      base_commit if one was provided.  Everything involving
    //      `GitRepository` (which is !Sync) is scoped inside a block so
    //      the future remains Send.
    let engine = server.engine();

    let (repo_id, version, summary) = {
        let (repo_id, git_repo) = engine.get_repo(&req.codebase).await.map_err(|e| match e {
            dk_core::Error::AmbiguousRepoName(_) => Status::invalid_argument(format!(
                "Ambiguous repository name: use the full 'owner/repo' form ({e})"
            )),
            _ => Status::not_found(format!("Repository not found: {e}")),
        })?;

        // HEAD commit hash (or "initial" for empty repos).
        let version = git_repo
            .head_hash()
            .map_err(|e| Status::internal(format!("Failed to read HEAD: {e}")))?
            .unwrap_or_else(|| "initial".to_string());

        // Validate custom base_commit while we still hold git_repo, avoiding
        // a second `get_repo` call.
        if let Some(ref base) = requested_base_commit {
            if base != &version && base != "initial" {
                git_repo.list_tree_files(base).map_err(|_| {
                    Status::invalid_argument(format!(
                        "base_commit '{base}' does not resolve to a valid commit"
                    ))
                })?;
            }
        }

        // Drop git_repo before the next .await to keep the future Send.
        drop(git_repo);

        let summary = engine
            .codebase_summary(repo_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get summary: {e}")))?;

        (repo_id, version, summary)
    };

    // 5. Create session (session_mgr is lock-free / DashMap-based).
    let session_id = server.session_mgr().create_session(
        req.agent_id.clone(),
        req.codebase.clone(),
        req.intent.clone(),
        version.clone(),
    );

    // 5a. Resolve agent name: use provided name or auto-assign.
    let agent_name = if req.agent_name.is_empty() {
        engine.workspace_manager().next_agent_name(&repo_id)
    } else {
        req.agent_name.clone()
    };

    // 5b. Create a changeset (staging area for file changes).
    let changeset = engine
        .changeset_store()
        .create(
            repo_id,
            Some(session_id),
            &req.agent_id,
            &req.intent,
            Some(&version),
            &agent_name,
        )
        .await
        .map_err(|e| Status::internal(format!("failed to create changeset: {e}")))?;

    // 6. Determine workspace mode from request config
    let ws_mode = match req.workspace_config.as_ref().map(|c| c.mode()) {
        Some(crate::WorkspaceMode::Persistent) => WorkspaceMode::Persistent { expires_at: None },
        _ => WorkspaceMode::Ephemeral,
    };

    // Use the provided base_commit or default to current HEAD version
    let base_commit = requested_base_commit.unwrap_or_else(|| version.clone());

    // Create the session workspace
    let workspace_id = engine
        .workspace_manager()
        .create_workspace(
            session_id,
            repo_id,
            req.agent_id.clone(),
            changeset.id,
            req.intent.clone(),
            base_commit,
            ws_mode,
            agent_name.clone(),
        )
        .await
        .map_err(|e| Status::internal(format!("failed to create workspace: {e}")))?;

    // 7. Build concurrency info
    let other_session_ids = engine
        .workspace_manager()
        .active_sessions_for_repo(repo_id, Some(session_id));

    let mut other_sessions = Vec::new();
    for other_sid in &other_session_ids {
        if let Some(other_ws) = engine.workspace_manager().get_workspace(other_sid) {
            // Gather just the paths (avoids cloning file content)
            let active_files: Vec<String> = other_ws.overlay.list_paths();

            other_sessions.push(ActiveSessionSummary {
                agent_id: other_ws.agent_id.clone(),
                intent: other_ws.intent.clone(),
                active_files,
            });
        }
    }

    let concurrency = WorkspaceConcurrencyInfo {
        active_sessions: (other_session_ids.len() + 1) as u32, // include this session
        other_sessions,
    };

    info!(
        session_id = %session_id,
        changeset_id = %changeset.id,
        workspace_id = %workspace_id,
        agent_id = %req.agent_id,
        agent_name = %agent_name,
        codebase = %req.codebase,
        active_sessions = concurrency.active_sessions,
        "CONNECT: session, changeset, and workspace created"
    );

    Ok(Response::new(ConnectResponse {
        session_id: session_id.to_string(),
        codebase_version: version,
        summary: Some(CodebaseSummary {
            languages: summary.languages,
            total_symbols: summary.total_symbols,
            total_files: summary.total_files,
        }),
        changeset_id: changeset.id.to_string(),
        workspace_id: workspace_id.to_string(),
        concurrency: Some(concurrency),
    }))
}

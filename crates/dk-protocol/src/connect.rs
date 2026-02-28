use tonic::{Response, Status};
use tracing::info;

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
    let _authed_agent_id = server.validate_auth(&req.auth_token)?;

    // Check for session resume.
    //
    // NOTE: `take_snapshot` intentionally *consumes* the snapshot so it cannot
    // be reused by a stale reconnect.  However, the snapshot data is not yet
    // used to restore workspace state — the workspace is always created fresh
    // from the request parameters below.  Full state restoration (overlay
    // files, cursor position, pending changes) requires persistent storage
    // (Redis / S3) and is tracked as future work.
    if let Some(ref ws_config) = req.workspace_config {
        if let Some(ref resume_id_str) = ws_config.resume_session_id {
            if let Ok(resume_id) = resume_id_str.parse::<uuid::Uuid>() {
                if let Some(snapshot) = server.session_mgr().take_snapshot(&resume_id) {
                    info!(
                        resume_from = %resume_id,
                        agent_id = %snapshot.agent_id,
                        "CONNECT: previous session snapshot acknowledged (workspace created fresh; full restore not yet implemented)"
                    );
                }
            }
        }
    }

    // 2-4. Resolve repo, get summary, and read HEAD version.
    //      Everything involving `GitRepository` (which is !Sync) is scoped
    //      inside a block so the future remains Send.
    let engine = server.engine();

    let (repo_id, version, summary) = {
        let (repo_id, git_repo) = engine
            .get_repo(&req.codebase)
            .await
            .map_err(|e| Status::not_found(format!("Repository not found: {e}")))?;

        // HEAD commit hash (or "initial" for empty repos).
        let version = git_repo
            .head_hash()
            .map_err(|e| Status::internal(format!("Failed to read HEAD: {e}")))?
            .unwrap_or_else(|| "initial".to_string());

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

    // 5b. Create a changeset (staging area for file changes).
    let changeset = engine
        .changeset_store()
        .create(repo_id, Some(session_id), &req.agent_id, &req.intent, Some(&version))
        .await
        .map_err(|e| Status::internal(format!("failed to create changeset: {e}")))?;

    // 6. Determine workspace mode from request config
    let ws_mode = match req.workspace_config.as_ref().map(|c| c.mode()) {
        Some(crate::WorkspaceMode::Persistent) => WorkspaceMode::Persistent { expires_at: None },
        _ => WorkspaceMode::Ephemeral,
    };

    // Use the provided base_commit or default to current HEAD version
    let base_commit = req
        .workspace_config
        .as_ref()
        .and_then(|c| c.base_commit.clone())
        .unwrap_or_else(|| version.clone());

    // Validate custom base_commit resolves to a real commit in the repo
    if base_commit != version && base_commit != "initial" {
        let (_rid, git_repo) = engine
            .get_repo(&req.codebase)
            .await
            .map_err(|e| Status::internal(format!("Repo error: {e}")))?;
        // list_tree_files calls find_commit internally; failure means bad commit
        git_repo
            .list_tree_files(&base_commit)
            .map_err(|_| {
                Status::invalid_argument(format!(
                    "base_commit '{base_commit}' does not resolve to a valid commit"
                ))
            })?;
        // git_repo dropped here before next .await
    }

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

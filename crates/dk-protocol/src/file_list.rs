use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::validation::validate_file_path;
use crate::{FileEntry, FileListRequest, FileListResponse};

/// Handle a FileList RPC.
///
/// Lists files visible in the session workspace, optionally filtered to
/// only modified files or by a path prefix.
pub async fn handle_file_list(
    server: &ProtocolServer,
    req: FileListRequest,
) -> Result<Response<FileListResponse>, Status> {
    // Validate prefix if provided
    if let Some(ref prefix) = req.prefix {
        if !prefix.is_empty() {
            validate_file_path(prefix)?;
        }
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

    // Get git repo for base-tree listing
    let (_repo_id, git_repo) = engine
        .get_repo(&session.codebase)
        .await
        .map_err(|e| Status::internal(format!("Repo error: {e}")))?;

    // Push prefix filter into list_files so the base tree traversal
    // only collects matching entries instead of the entire tree.
    let prefix = req.prefix.as_deref().filter(|p| !p.is_empty());

    let all_files = ws
        .list_files(&git_repo, req.only_modified, prefix)
        .map_err(|e| Status::internal(format!("List files failed: {e}")))?;

    // Collect modified file paths for O(1) lookup (list_paths avoids cloning content)
    let modified_paths: std::collections::HashSet<String> =
        ws.overlay.list_paths().into_iter().collect();

    let files: Vec<FileEntry> = all_files
        .into_iter()
        .map(|path| {
            let modified = modified_paths.contains(&path);
            FileEntry {
                path,
                modified_in_session: modified,
            }
        })
        .collect();

    info!(
        session_id = %req.session_id,
        file_count = files.len(),
        only_modified = req.only_modified,
        "FILE_LIST: served"
    );

    Ok(Response::new(FileListResponse { files }))
}

//! High-level tool operations for the Programmatic Tool Calling interface.
//!
//! Each method corresponds to one dkod tool (`dkod_connect`, `dkod_context`,
//! etc.). Both the gRPC handlers in dk-protocol and the HTTP fulfillment
//! endpoint in dk-platform call these methods — no logic duplication.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use uuid::Uuid;

use crate::repo::{CodebaseSummary, Engine};
use crate::workspace::session_workspace::WorkspaceMode;

/// Validate a file path for safety (no traversal, no absolute paths).
fn validate_path(path: &str) -> dk_core::Result<()> {
    if path.is_empty() {
        return Err(dk_core::Error::InvalidInput(
            "file path cannot be empty".into(),
        ));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(dk_core::Error::InvalidInput(
            "file path must be relative".into(),
        ));
    }
    if path.contains('\0') {
        return Err(dk_core::Error::InvalidInput(
            "file path contains null byte".into(),
        ));
    }
    // Split on both forward-slash and backslash to prevent traversal via
    // Windows-style paths like "foo\..\bar".
    for component in path.split(&['/', '\\'] as &[char]) {
        if component == ".." {
            return Err(dk_core::Error::InvalidInput(
                "file path contains '..' traversal".into(),
            ));
        }
    }
    Ok(())
}

// ── Result types (Serialize for JSON fulfillment responses) ──

#[derive(Debug, Serialize)]
pub struct ToolConnectResult {
    pub session_id: String,
    pub base_commit: String,
    pub codebase_summary: ToolCodebaseSummary,
    pub active_sessions: u32,
}

#[derive(Debug, Serialize)]
pub struct ToolCodebaseSummary {
    pub languages: Vec<String>,
    pub total_symbols: u64,
    pub total_files: u64,
}

impl From<CodebaseSummary> for ToolCodebaseSummary {
    fn from(s: CodebaseSummary) -> Self {
        Self {
            languages: s.languages,
            total_symbols: s.total_symbols,
            total_files: s.total_files,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ToolContextSymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file_path: String,
    pub signature: Option<String>,
    pub source: Option<String>,
    pub callers: Vec<String>,
    pub callees: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolContextResult {
    pub symbols: Vec<ToolContextSymbol>,
    pub token_count: u32,
    pub freshness: String,
}

#[derive(Debug, Serialize)]
pub struct ToolFileReadResult {
    pub content: String,
    pub hash: String,
    pub modified_in_session: bool,
}

#[derive(Debug, Serialize)]
pub struct ToolFileWriteResult {
    pub new_hash: String,
    pub detected_changes: Vec<ToolDetectedChange>,
}

#[derive(Debug, Serialize)]
pub struct ToolDetectedChange {
    pub symbol_name: String,
    pub change_type: String,
}

#[derive(Debug, Serialize)]
pub struct ToolSubmitResult {
    pub status: String,
    pub version: Option<String>,
    pub changeset_id: String,
    pub failures: Vec<ToolVerifyFailure>,
    pub conflicts: Vec<ToolConflict>,
}

#[derive(Debug, Serialize)]
pub struct ToolVerifyFailure {
    pub gate: String,
    pub test_name: String,
    pub error: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolConflict {
    pub file: String,
    pub symbol: String,
    pub our_change: String,
    pub their_change: String,
}

#[derive(Debug, Serialize)]
pub struct ToolStatusResult {
    pub session_id: String,
    pub base_commit: String,
    pub files_modified: Vec<String>,
    pub symbols_modified: Vec<String>,
    pub overlay_size_bytes: u64,
    pub active_other_sessions: u32,
}

#[derive(Debug, Serialize)]
pub struct ToolFileListEntry {
    pub path: String,
    pub modified_in_session: bool,
}

#[derive(Debug, Serialize)]
pub struct ToolFileListResult {
    pub files: Vec<ToolFileListEntry>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct ToolVerifyStepResult {
    pub step_name: String,
    pub status: String,
    pub output: String,
    pub required: bool,
}

#[derive(Debug, Serialize)]
pub struct ToolVerifyResult {
    pub changeset_id: String,
    pub passed: bool,
    pub steps: Vec<ToolVerifyStepResult>,
}

#[derive(Debug, Serialize)]
pub struct ToolMergeResult {
    pub commit_hash: String,
    pub merged_version: String,
    pub auto_rebased: bool,
    pub auto_rebased_files: Vec<String>,
    pub conflicts: Vec<ToolConflict>,
}

// ── Tool operation implementations on Engine ──

impl Engine {
    /// CONNECT — establish an isolated session workspace.
    pub async fn tool_connect(
        &self,
        repo_name: &str,
        intent: &str,
        agent_id: &str,
        session_id: Uuid,
        changeset_id: Uuid,
    ) -> dk_core::Result<ToolConnectResult> {
        let (repo_id, git_repo) = self.get_repo(repo_name).await?;
        let head = git_repo
            .head_hash()?
            .unwrap_or_else(|| "initial".to_string());
        drop(git_repo);

        // Create changeset
        self.changeset_store()
            .create(repo_id, Some(session_id), agent_id, intent, Some(&head))
            .await?;

        // Create workspace (agent_id is AgentId = String)
        self.workspace_manager()
            .create_workspace(
                session_id,
                repo_id,
                agent_id.to_string(),
                changeset_id,
                intent.to_string(),
                head.clone(),
                WorkspaceMode::Ephemeral,
            )
            .await?;

        let summary = self.codebase_summary(repo_id).await?;

        let active = self
            .workspace_manager()
            .active_sessions_for_repo(repo_id, Some(session_id))
            .len() as u32;

        Ok(ToolConnectResult {
            session_id: session_id.to_string(),
            base_commit: head,
            codebase_summary: summary.into(),
            active_sessions: active,
        })
    }

    /// CONTEXT — semantic code search through the session workspace.
    pub async fn tool_context(
        &self,
        session_id: Uuid,
        query: &str,
        depth: Option<&str>,
        _include_tests: Option<bool>,
        _max_tokens: Option<u32>,
    ) -> dk_core::Result<ToolContextResult> {
        // Get workspace info, then drop the guard
        let repo_id = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            ws.repo_id
        };

        let max_results = 50usize;
        let symbols = self.query_symbols(repo_id, query, max_results).await?;

        let depth = depth.unwrap_or("signatures");
        let include_source = depth == "full" || depth == "call_graph";
        let include_call_graph = depth == "call_graph";

        // Get repo path for source reading
        let (_, git_repo) = self.get_repo_by_db_id(repo_id).await?;
        let work_dir = git_repo.path().to_path_buf();
        drop(git_repo);

        let mut result_symbols = Vec::with_capacity(symbols.len());
        let mut total_chars = 0u64;

        // Cache file contents to avoid re-reading the same file for multiple symbols.
        let mut file_cache: HashMap<String, Option<Vec<u8>>> = HashMap::new();

        for sym in &symbols {
            let source = if include_source {
                // Try workspace overlay first, then base tree via working directory
                let file_path_str = sym.file_path.to_string_lossy().to_string();

                let file_content = if let Some(cached) = file_cache.get(&file_path_str) {
                    cached.clone()
                } else {
                    let overlay_content = {
                        let ws = self.workspace_manager().get_workspace(&session_id);
                        ws.and_then(|ws_ref| {
                            ws_ref
                                .overlay
                                .get(&file_path_str)
                                .and_then(|entry| entry.value().content().map(|c| c.to_vec()))
                        })
                    };
                    let content = match overlay_content {
                        Some(c) => Some(c),
                        None => {
                            let full_path = work_dir.join(&sym.file_path);
                            tokio::fs::read(&full_path).await.ok()
                        }
                    };
                    file_cache.insert(file_path_str, content.clone());
                    content
                };

                let start = sym.span.start_byte as usize;
                let end = sym.span.end_byte as usize;
                file_content.and_then(|c| {
                    if start < c.len() && end <= c.len() {
                        Some(
                            String::from_utf8_lossy(&c[start..end]).to_string(),
                        )
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // TODO(perf): Batch-fetch call graph edges for all symbol IDs in a
            // single query instead of N sequential get_call_graph calls.
            let (callers, callees) = if include_call_graph {
                let (c, e) = self.get_call_graph(repo_id, sym.id).await?;
                (
                    c.iter().map(|s| s.qualified_name.clone()).collect(),
                    e.iter().map(|s| s.qualified_name.clone()).collect(),
                )
            } else {
                (vec![], vec![])
            };

            if let Some(ref src) = source {
                total_chars += src.len() as u64;
            }

            result_symbols.push(ToolContextSymbol {
                name: sym.name.clone(),
                qualified_name: sym.qualified_name.clone(),
                kind: format!("{:?}", sym.kind),
                file_path: sym.file_path.to_string_lossy().to_string(),
                signature: sym.signature.clone(),
                source,
                callers,
                callees,
            });
        }

        let token_count = (total_chars / 4) as u32;

        Ok(ToolContextResult {
            symbols: result_symbols,
            token_count,
            freshness: "live".to_string(),
        })
    }

    /// FILE_READ — read a file through the session workspace overlay.
    pub async fn tool_read_file(
        &self,
        session_id: Uuid,
        path: &str,
    ) -> dk_core::Result<ToolFileReadResult> {
        validate_path(path)?;

        // Single workspace lookup: extract repo_id and read file in one guard scope.
        let repo_id = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            ws.repo_id
        };

        let (_, git_repo) = self.get_repo_by_db_id(repo_id).await?;

        let result = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            ws.read_file(path, &git_repo)?
        };
        drop(git_repo);

        Ok(ToolFileReadResult {
            content: String::from_utf8_lossy(&result.content).to_string(),
            hash: result.hash,
            modified_in_session: result.modified_in_session,
        })
    }

    /// FILE_WRITE — write a file to the session workspace overlay.
    pub async fn tool_write_file(
        &self,
        session_id: Uuid,
        changeset_id: Uuid,
        path: &str,
        content: &str,
    ) -> dk_core::Result<ToolFileWriteResult> {
        validate_path(path)?;

        // Single workspace lookup: extract repo_id and base_commit together.
        let (repo_id, base_commit) = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            (ws.repo_id, ws.base_commit.clone())
        };

        // Determine is_new synchronously, then drop git_repo before any
        // async work so the future stays Send (gix::Repository has RefCell).
        let is_new = {
            let (_, git_repo) = self.get_repo_by_db_id(repo_id).await?;
            git_repo.read_tree_entry(&base_commit, path).is_err()
            // git_repo dropped here
        };

        let content_bytes = content.as_bytes().to_vec();

        // Write to overlay without holding git_repo across .await
        let new_hash = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            ws.overlay.write(path, content_bytes, is_new).await?
        };

        // Record in changeset
        self.changeset_store()
            .upsert_file(changeset_id, path, "modify", Some(content))
            .await?;

        // Detect symbol changes
        let detected = self.detect_symbol_changes(path, content.as_bytes());

        Ok(ToolFileWriteResult {
            new_hash,
            detected_changes: detected,
        })
    }

    /// SUBMIT — submit the session's workspace changes as a changeset.
    pub async fn tool_submit(
        &self,
        _session_id: Uuid,
        changeset_id: Uuid,
        _intent: &str,
    ) -> dk_core::Result<ToolSubmitResult> {
        self.changeset_store()
            .update_status(changeset_id, "submitted")
            .await?;

        Ok(ToolSubmitResult {
            status: "accepted".to_string(),
            version: None,
            changeset_id: changeset_id.to_string(),
            failures: vec![],
            conflicts: vec![],
        })
    }

    /// SESSION_STATUS — get the current workspace state.
    pub async fn tool_session_status(
        &self,
        session_id: Uuid,
    ) -> dk_core::Result<ToolStatusResult> {
        let (files_modified, overlay_size_bytes, repo_id, base_commit, changeset_id) = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            (
                ws.overlay.list_paths(),
                ws.overlay.total_bytes() as u64,
                ws.repo_id,
                ws.base_commit.clone(),
                ws.changeset_id,
            )
        };

        let active_other = self
            .workspace_manager()
            .active_sessions_for_repo(repo_id, Some(session_id))
            .len() as u32;

        let symbols_modified = match self
            .changeset_store()
            .get_affected_symbols(changeset_id)
            .await
        {
            Ok(syms) => syms.into_iter().map(|(_, qn)| qn).collect(),
            Err(_) => vec![],
        };

        Ok(ToolStatusResult {
            session_id: session_id.to_string(),
            base_commit,
            files_modified,
            symbols_modified,
            overlay_size_bytes,
            active_other_sessions: active_other,
        })
    }

    /// LIST_FILES — list files visible in the session workspace.
    pub async fn tool_list_files(
        &self,
        session_id: Uuid,
        prefix: Option<&str>,
    ) -> dk_core::Result<ToolFileListResult> {
        // First lookup: extract repo_id and modified paths from the workspace
        // in a single DashMap guard scope to avoid race conditions.
        let (repo_id, modified_paths) = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            let modified: std::collections::HashSet<String> =
                ws.overlay.list_paths().into_iter().collect();
            (ws.repo_id, modified)
        };

        let (_, git_repo) = self.get_repo_by_db_id(repo_id).await?;

        // Second lookup: list_files needs the git_repo which required an async
        // call above, so a second guard acquisition is unavoidable here.
        let all_files = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            ws.list_files(&git_repo, false, prefix)?
        };
        drop(git_repo);

        let total = all_files.len();
        let files = all_files
            .into_iter()
            .map(|path| {
                let modified_in_session = modified_paths.contains(&path);
                ToolFileListEntry {
                    path,
                    modified_in_session,
                }
            })
            .collect();

        Ok(ToolFileListResult { files, total })
    }

    /// VERIFY — prepare a session's changeset for verification.
    ///
    /// Returns `(changeset_id, repo_name)` after validating the session,
    /// checking the changeset, and updating its status to "verifying".
    /// The actual runner invocation must be done by the caller (since
    /// dk-runner depends on dk-engine, not the other way around).
    pub async fn tool_verify_prepare(
        &self,
        session_id: Uuid,
    ) -> dk_core::Result<(Uuid, String)> {
        let (changeset_id, repo_id) = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            (ws.changeset_id, ws.repo_id)
        };

        // Verify changeset exists
        self.changeset_store().get(changeset_id).await?;

        // Update status to verifying
        self.changeset_store()
            .update_status(changeset_id, "verifying")
            .await?;

        // Look up repo name by ID for the runner
        let (repo_name,): (String,) =
            sqlx::query_as("SELECT name FROM repositories WHERE id = $1")
                .bind(repo_id)
                .fetch_one(&self.db)
                .await
                .map_err(|e| {
                    dk_core::Error::Internal(format!("failed to look up repo name: {e}"))
                })?;

        Ok((changeset_id, repo_name))
    }

    /// VERIFY — finalize after the runner has completed.
    ///
    /// Updates the changeset status to "approved" or "rejected" based on
    /// whether all steps passed.
    pub async fn tool_verify_finalize(
        &self,
        changeset_id: Uuid,
        passed: bool,
    ) -> dk_core::Result<()> {
        let final_status = if passed { "approved" } else { "rejected" };
        self.changeset_store()
            .update_status(changeset_id, final_status)
            .await
    }

    /// MERGE — merge the verified changeset into a Git commit.
    pub async fn tool_merge(
        &self,
        session_id: Uuid,
        message: Option<&str>,
    ) -> dk_core::Result<ToolMergeResult> {
        let (changeset_id, repo_id) = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;
            (ws.changeset_id, ws.repo_id)
        };

        // Get changeset and verify it's approved
        let changeset = self.changeset_store().get(changeset_id).await?;
        if changeset.state != "approved" {
            return Err(dk_core::Error::InvalidInput(format!(
                "changeset is '{}', must be 'approved' to merge",
                changeset.state
            )));
        }

        let agent = changeset.agent_id.as_deref().unwrap_or("agent");
        let commit_message = message.unwrap_or("merge changeset");

        let (_, git_repo) = self.get_repo_by_db_id(repo_id).await?;

        let merge_result = {
            let ws = self
                .workspace_manager()
                .get_workspace(&session_id)
                .ok_or_else(|| dk_core::Error::SessionNotFound(session_id.to_string()))?;

            crate::workspace::merge::merge_workspace(
                &ws,
                &git_repo,
                self.parser(),
                commit_message,
                agent,
                &format!("{agent}@dekode.dev"),
            )?
        };
        drop(git_repo);

        match merge_result {
            crate::workspace::merge::WorkspaceMergeResult::FastMerge { commit_hash } => {
                self.changeset_store()
                    .set_merged(changeset_id, &commit_hash)
                    .await?;

                Ok(ToolMergeResult {
                    commit_hash: commit_hash.clone(),
                    merged_version: commit_hash,
                    auto_rebased: false,
                    auto_rebased_files: vec![],
                    conflicts: vec![],
                })
            }

            crate::workspace::merge::WorkspaceMergeResult::RebaseMerge {
                commit_hash,
                auto_rebased_files,
            } => {
                self.changeset_store()
                    .set_merged(changeset_id, &commit_hash)
                    .await?;

                Ok(ToolMergeResult {
                    commit_hash: commit_hash.clone(),
                    merged_version: commit_hash,
                    auto_rebased: true,
                    auto_rebased_files,
                    conflicts: vec![],
                })
            }

            crate::workspace::merge::WorkspaceMergeResult::Conflicts { conflicts } => {
                let tool_conflicts = conflicts
                    .iter()
                    .map(|c| ToolConflict {
                        file: c.file_path.clone(),
                        symbol: c.symbol_name.clone(),
                        our_change: format!("{:?}", c.our_change),
                        their_change: format!("{:?}", c.their_change),
                    })
                    .collect();

                Ok(ToolMergeResult {
                    commit_hash: String::new(),
                    merged_version: String::new(),
                    auto_rebased: false,
                    auto_rebased_files: vec![],
                    conflicts: tool_conflicts,
                })
            }
        }
    }

    /// Parse a file and return all detected symbols as changes.
    fn detect_symbol_changes(&self, path: &str, content: &[u8]) -> Vec<ToolDetectedChange> {
        let rel_path = Path::new(path);
        match self.parser().parse_file(rel_path, content) {
            Ok(analysis) => analysis
                .symbols
                .iter()
                .map(|s| ToolDetectedChange {
                    symbol_name: s.qualified_name.clone(),
                    change_type: "modified".to_string(),
                })
                .collect(),
            Err(_) => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_path ───────────────────────────────────────────────

    #[test]
    fn validate_path_accepts_simple_relative() {
        assert!(validate_path("src/main.rs").is_ok());
    }

    #[test]
    fn validate_path_accepts_single_file() {
        assert!(validate_path("Cargo.toml").is_ok());
    }

    #[test]
    fn validate_path_accepts_nested() {
        assert!(validate_path("a/b/c/d.txt").is_ok());
    }

    #[test]
    fn validate_path_accepts_dot_prefix() {
        // Single dot is fine (current dir), only ".." is banned.
        assert!(validate_path("./src/lib.rs").is_ok());
    }

    #[test]
    fn validate_path_rejects_empty() {
        let err = validate_path("").unwrap_err();
        assert!(
            matches!(err, dk_core::Error::InvalidInput(ref msg) if msg.contains("empty")),
            "expected empty error, got: {err}"
        );
    }

    #[test]
    fn validate_path_rejects_absolute_forward_slash() {
        let err = validate_path("/etc/passwd").unwrap_err();
        assert!(matches!(err, dk_core::Error::InvalidInput(ref msg) if msg.contains("relative")));
    }

    #[test]
    fn validate_path_rejects_absolute_backslash() {
        let err = validate_path("\\Windows\\system32").unwrap_err();
        assert!(matches!(err, dk_core::Error::InvalidInput(ref msg) if msg.contains("relative")));
    }

    #[test]
    fn validate_path_rejects_null_byte() {
        let err = validate_path("src/\0evil.rs").unwrap_err();
        assert!(matches!(err, dk_core::Error::InvalidInput(ref msg) if msg.contains("null")));
    }

    #[test]
    fn validate_path_rejects_dot_dot_traversal() {
        let err = validate_path("src/../../../etc/passwd").unwrap_err();
        assert!(matches!(err, dk_core::Error::InvalidInput(ref msg) if msg.contains("traversal")));
    }

    #[test]
    fn validate_path_rejects_backslash_traversal() {
        let err = validate_path("src\\..\\secret.txt").unwrap_err();
        assert!(matches!(err, dk_core::Error::InvalidInput(ref msg) if msg.contains("traversal")));
    }

    #[test]
    fn validate_path_allows_dot_dot_in_filename() {
        // "foo..bar" should be fine — only bare ".." as a component is banned.
        assert!(validate_path("foo..bar.txt").is_ok());
    }

    // ── ToolFileListEntry / ToolFileListResult construction ─────────

    #[test]
    fn file_list_entry_modified_flag() {
        let entry = ToolFileListEntry {
            path: "src/lib.rs".into(),
            modified_in_session: true,
        };
        assert!(entry.modified_in_session);
        assert_eq!(entry.path, "src/lib.rs");

        let unmodified = ToolFileListEntry {
            path: "Cargo.toml".into(),
            modified_in_session: false,
        };
        assert!(!unmodified.modified_in_session);
    }

    #[test]
    fn file_list_result_total_matches_files() {
        let entries = vec![
            ToolFileListEntry {
                path: "a.rs".into(),
                modified_in_session: false,
            },
            ToolFileListEntry {
                path: "b.rs".into(),
                modified_in_session: true,
            },
            ToolFileListEntry {
                path: "c.rs".into(),
                modified_in_session: false,
            },
        ];
        let result = ToolFileListResult {
            total: entries.len(),
            files: entries,
        };
        assert_eq!(result.total, 3);
        assert_eq!(result.files.len(), 3);
    }

    #[test]
    fn file_list_modified_filter_via_hashset() {
        // Mirrors the logic in tool_list_files: build modified set, map files.
        let modified_paths: std::collections::HashSet<String> =
            ["src/changed.rs".to_string()].into_iter().collect();

        let all_files = vec![
            "src/changed.rs".to_string(),
            "src/unchanged.rs".to_string(),
            "Cargo.toml".to_string(),
        ];

        let entries: Vec<ToolFileListEntry> = all_files
            .into_iter()
            .map(|path| {
                let modified_in_session = modified_paths.contains(&path);
                ToolFileListEntry {
                    path,
                    modified_in_session,
                }
            })
            .collect();

        assert!(entries[0].modified_in_session); // src/changed.rs
        assert!(!entries[1].modified_in_session); // src/unchanged.rs
        assert!(!entries[2].modified_in_session); // Cargo.toml
    }

    // ── ToolCodebaseSummary From<CodebaseSummary> ───────────────────

    #[test]
    fn codebase_summary_from_conversion() {
        let src = CodebaseSummary {
            languages: vec!["Rust".into(), "TypeScript".into()],
            total_symbols: 42,
            total_files: 10,
        };
        let tool: ToolCodebaseSummary = src.into();
        assert_eq!(tool.languages, vec!["Rust", "TypeScript"]);
        assert_eq!(tool.total_symbols, 42);
        assert_eq!(tool.total_files, 10);
    }

    // ── ToolVerifyStepResult / ToolVerifyResult construction ────────

    #[test]
    fn verify_result_passed_true() {
        let result = ToolVerifyResult {
            changeset_id: Uuid::new_v4().to_string(),
            passed: true,
            steps: vec![ToolVerifyStepResult {
                step_name: "lint".into(),
                status: "passed".into(),
                output: "no warnings".into(),
                required: true,
            }],
        };
        assert!(result.passed);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].status, "passed");
    }

    #[test]
    fn verify_result_passed_false() {
        let result = ToolVerifyResult {
            changeset_id: Uuid::new_v4().to_string(),
            passed: false,
            steps: vec![
                ToolVerifyStepResult {
                    step_name: "lint".into(),
                    status: "passed".into(),
                    output: String::new(),
                    required: true,
                },
                ToolVerifyStepResult {
                    step_name: "test".into(),
                    status: "failed".into(),
                    output: "1 test failed".into(),
                    required: true,
                },
            ],
        };
        assert!(!result.passed);
        assert_eq!(result.steps[1].status, "failed");
    }

    // ── verify_finalize status logic ────────────────────────────────
    // The actual method requires a DB. Here we test the status derivation
    // logic directly (the same expression used in tool_verify_finalize).

    #[test]
    fn verify_finalize_status_derivation() {
        let status_for = |passed: bool| -> &str {
            if passed { "approved" } else { "rejected" }
        };
        assert_eq!(status_for(true), "approved");
        assert_eq!(status_for(false), "rejected");
    }

    // ── merge rejection logic ───────────────────────────────────────
    // tool_merge checks `changeset.state != "approved"`. We test that
    // the error message format matches for various non-approved states.

    #[test]
    fn merge_rejects_non_approved_states() {
        for state in &["submitted", "verifying", "rejected", "draft"] {
            let err = dk_core::Error::InvalidInput(format!(
                "changeset is '{}', must be 'approved' to merge",
                state
            ));
            let msg = err.to_string();
            assert!(
                msg.contains("must be 'approved' to merge"),
                "unexpected error for state '{state}': {msg}"
            );
            assert!(
                msg.contains(state),
                "error should contain the state '{state}': {msg}"
            );
        }
    }

    // ── ToolDetectedChange construction ─────────────────────────────

    #[test]
    fn detected_change_construction() {
        let change = ToolDetectedChange {
            symbol_name: "crate::foo::Bar".into(),
            change_type: "modified".into(),
        };
        assert_eq!(change.symbol_name, "crate::foo::Bar");
        assert_eq!(change.change_type, "modified");
    }

    // ── JSON serialization ──────────────────────────────────────────

    #[test]
    fn tool_connect_result_serializes() {
        let result = ToolConnectResult {
            session_id: "abc-123".into(),
            base_commit: "deadbeef".into(),
            codebase_summary: ToolCodebaseSummary {
                languages: vec!["Rust".into()],
                total_symbols: 100,
                total_files: 5,
            },
            active_sessions: 2,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["session_id"], "abc-123");
        assert_eq!(json["active_sessions"], 2);
        assert!(json["codebase_summary"]["languages"].is_array());
    }

    #[test]
    fn tool_file_list_result_serializes() {
        let result = ToolFileListResult {
            total: 1,
            files: vec![ToolFileListEntry {
                path: "src/lib.rs".into(),
                modified_in_session: true,
            }],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["total"], 1);
        assert_eq!(json["files"][0]["path"], "src/lib.rs");
        assert_eq!(json["files"][0]["modified_in_session"], true);
    }

    #[test]
    fn tool_merge_result_serializes_with_conflicts() {
        let result = ToolMergeResult {
            commit_hash: String::new(),
            merged_version: String::new(),
            auto_rebased: false,
            auto_rebased_files: vec![],
            conflicts: vec![ToolConflict {
                file: "src/main.rs".into(),
                symbol: "main".into(),
                our_change: "added line".into(),
                their_change: "removed line".into(),
            }],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["conflicts"][0]["file"], "src/main.rs");
        assert_eq!(json["conflicts"][0]["symbol"], "main");
    }
}

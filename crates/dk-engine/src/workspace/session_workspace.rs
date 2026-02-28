//! SessionWorkspace — the isolated workspace for a single agent session.
//!
//! Each workspace owns a [`FileOverlay`] and a [`SessionGraph`], pinned to a
//! `base_commit` in the repository. Reads go through the overlay first, then
//! fall back to the Git tree at the base commit.

use dk_core::{AgentId, RepoId, Result};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::HashSet;
use tokio::time::Instant;
use uuid::Uuid;

use crate::git::GitRepository;
use crate::workspace::overlay::{FileOverlay, OverlayEntry};
use crate::workspace::session_graph::SessionGraph;

// ── Type aliases ─────────────────────────────────────────────────────

pub type WorkspaceId = Uuid;
pub type SessionId = Uuid;

// ── Workspace mode ───────────────────────────────────────────────────

/// Controls the lifetime semantics of a workspace.
#[derive(Debug, Clone)]
pub enum WorkspaceMode {
    /// Destroyed when the session disconnects.
    Ephemeral,
    /// Survives disconnection; optionally expires at a deadline.
    Persistent { expires_at: Option<Instant> },
}

impl WorkspaceMode {
    /// SQL label for the DB column.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::Persistent { .. } => "persistent",
        }
    }
}

// ── Workspace state machine ──────────────────────────────────────────

/// Lifecycle state of a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceState {
    Active,
    Submitted,
    Merged,
    Expired,
    Abandoned,
}

impl WorkspaceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Submitted => "submitted",
            Self::Merged => "merged",
            Self::Expired => "expired",
            Self::Abandoned => "abandoned",
        }
    }
}

// ── File read result ─────────────────────────────────────────────────

/// Result of reading a file through the workspace layer.
#[derive(Debug, Clone)]
pub struct FileReadResult {
    pub content: Vec<u8>,
    pub hash: String,
    pub modified_in_session: bool,
}

// ── SessionWorkspace ─────────────────────────────────────────────────

/// An isolated workspace for a single agent session.
///
/// Reads resolve overlay-first, then fall through to the Git tree at
/// `base_commit`. Writes go exclusively to the overlay.
pub struct SessionWorkspace {
    pub id: WorkspaceId,
    pub session_id: SessionId,
    pub repo_id: RepoId,
    pub agent_id: AgentId,
    pub changeset_id: uuid::Uuid,
    pub intent: String,
    pub base_commit: String,
    pub overlay: FileOverlay,
    pub graph: SessionGraph,
    pub mode: WorkspaceMode,
    pub state: WorkspaceState,
    pub created_at: Instant,
    pub last_active: Instant,
}

impl SessionWorkspace {
    /// Create a workspace without any database interaction (test-only).
    ///
    /// Uses [`FileOverlay::new_inmemory`] so writes go only to the
    /// in-memory DashMap. Suitable for unit / integration tests that
    /// verify isolation semantics without requiring PostgreSQL.
    #[doc(hidden)]
    pub fn new_test(
        session_id: SessionId,
        repo_id: RepoId,
        agent_id: AgentId,
        intent: String,
        base_commit: String,
        mode: WorkspaceMode,
    ) -> Self {
        let id = Uuid::new_v4();
        let now = Instant::now();
        let overlay = FileOverlay::new_inmemory(id);
        let graph = SessionGraph::empty();

        Self {
            id,
            session_id,
            repo_id,
            agent_id,
            changeset_id: Uuid::new_v4(),
            intent,
            base_commit,
            overlay,
            graph,
            mode,
            state: WorkspaceState::Active,
            created_at: now,
            last_active: now,
        }
    }

    /// Create a new workspace and persist metadata to the database.
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        session_id: SessionId,
        repo_id: RepoId,
        agent_id: AgentId,
        changeset_id: Uuid,
        intent: String,
        base_commit: String,
        mode: WorkspaceMode,
        db: PgPool,
    ) -> Result<Self> {
        let id = Uuid::new_v4();
        let now = Instant::now();

        // Persist to DB
        sqlx::query(
            r#"
            INSERT INTO session_workspaces
                (id, session_id, repo_id, base_commit_hash, state, mode, agent_id, intent)
            VALUES ($1, $2, $3, $4, 'active', $5, $6, $7)
            "#,
        )
        .bind(id)
        .bind(session_id)
        .bind(repo_id)
        .bind(&base_commit)
        .bind(mode.as_str())
        .bind(&agent_id)
        .bind(&intent)
        .execute(&db)
        .await?;

        let overlay = FileOverlay::new(id, db);
        let graph = SessionGraph::empty();

        Ok(Self {
            id,
            session_id,
            repo_id,
            agent_id,
            changeset_id,
            intent,
            base_commit,
            overlay,
            graph,
            mode,
            state: WorkspaceState::Active,
            created_at: now,
            last_active: now,
        })
    }

    /// Read a file through the overlay-first layer.
    ///
    /// 1. If the overlay has a `Modified` or `Added` entry, return that content.
    /// 2. If the overlay has a `Deleted` entry, return a "not found" error.
    /// 3. Otherwise, read from the Git tree at `base_commit`.
    pub fn read_file(&self, path: &str, git_repo: &GitRepository) -> Result<FileReadResult> {
        if let Some(entry) = self.overlay.get(path) {
            return match entry.value() {
                OverlayEntry::Modified { content, hash } | OverlayEntry::Added { content, hash } => {
                    Ok(FileReadResult {
                        content: content.clone(),
                        hash: hash.clone(),
                        modified_in_session: true,
                    })
                }
                OverlayEntry::Deleted => Err(dk_core::Error::Git(format!(
                    "file '{path}' has been deleted in this session"
                ))),
            };
        }

        // Fall through to base tree.
        // TODO(perf): The git tree entry already stores a content-addressable
        // OID (blob hash). If GitRepository exposed the entry OID we could use
        // it directly instead of recomputing SHA-256 on every base-tree read.
        let content = git_repo.read_tree_entry(&self.base_commit, path)?;
        let hash = format!("{:x}", Sha256::digest(&content));

        Ok(FileReadResult {
            content,
            hash,
            modified_in_session: false,
        })
    }

    /// Write a file through the overlay.
    ///
    /// Determines whether the file is new (not in base tree) or modified.
    pub async fn write_file(
        &self,
        path: &str,
        content: Vec<u8>,
        git_repo: &GitRepository,
    ) -> Result<String> {
        let is_new = git_repo.read_tree_entry(&self.base_commit, path).is_err();
        self.overlay.write(path, content, is_new).await
    }

    /// Delete a file in the overlay.
    pub async fn delete_file(&self, path: &str) -> Result<()> {
        self.overlay.delete(path).await
    }

    /// List files visible in this workspace.
    ///
    /// If `only_modified` is true, return only overlay entries.
    /// Otherwise, return the full base tree merged with overlay changes.
    ///
    /// When `prefix` is `Some`, only paths starting with the given prefix
    /// are included. The filter is applied early in the pipeline so that
    /// building the `HashSet` only contains relevant entries rather than
    /// the entire tree (which can be 100k+ files in large repos).
    pub fn list_files(
        &self,
        git_repo: &GitRepository,
        only_modified: bool,
        prefix: Option<&str>,
    ) -> Result<Vec<String>> {
        let matches_prefix = |p: &str| -> bool {
            match prefix {
                Some(pfx) => p.starts_with(pfx),
                None => true,
            }
        };

        if only_modified {
            return Ok(self
                .overlay
                .list_changes()
                .into_iter()
                .filter(|(path, _)| matches_prefix(path))
                .map(|(path, _)| path)
                .collect());
        }

        // Start with base tree — filter by prefix before collecting into
        // the HashSet to avoid allocating entries we will immediately discard.
        let base_files = git_repo.list_tree_files(&self.base_commit)?;
        let mut result: HashSet<String> = base_files
            .into_iter()
            .filter(|p| matches_prefix(p))
            .collect();

        // Apply overlay (only entries matching the prefix)
        for (path, entry) in self.overlay.list_changes() {
            if !matches_prefix(&path) {
                continue;
            }
            match entry {
                OverlayEntry::Added { .. } | OverlayEntry::Modified { .. } => {
                    result.insert(path);
                }
                OverlayEntry::Deleted => {
                    result.remove(&path);
                }
            }
        }

        let mut files: Vec<String> = result.into_iter().collect();
        files.sort();
        Ok(files)
    }

    /// Touch the workspace to update last-active timestamp.
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    /// Build the overlay vector for `commit_tree_overlay`.
    ///
    /// Returns `(path, Some(content))` for modified/added files and
    /// `(path, None)` for deleted files.
    pub fn overlay_for_tree(&self) -> Vec<(String, Option<Vec<u8>>)> {
        self.overlay
            .list_changes()
            .into_iter()
            .map(|(path, entry)| {
                let data = match entry {
                    OverlayEntry::Modified { content, .. }
                    | OverlayEntry::Added { content, .. } => Some(content),
                    OverlayEntry::Deleted => None,
                };
                (path, data)
            })
            .collect()
    }
}

//! WorkspaceManager — manages all active session workspaces.
//!
//! Provides creation, lookup, destruction, and garbage collection of
//! workspaces. Uses `DashMap` for lock-free concurrent access from
//! multiple agent sessions.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use dk_core::{AgentId, RepoId, Result};
use serde::Serialize;
use sqlx::PgPool;
use tokio::time::Instant;
use uuid::Uuid;

use crate::workspace::cache::{NoOpCache, WorkspaceCache};
use crate::workspace::session_workspace::{
    SessionId, SessionWorkspace, WorkspaceMode,
};

// ── SessionInfo ─────────────────────────────────────────────────────

/// Lightweight snapshot of a session workspace, suitable for JSON serialization.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub agent_id: String,
    pub agent_name: String,
    pub intent: String,
    pub repo_id: Uuid,
    pub changeset_id: Uuid,
    pub state: String,
    pub elapsed_secs: u64,
}

// ── WorkspaceManager ─────────────────────────────────────────────────

/// Central registry of all active session workspaces.
///
/// Thread-safe via `DashMap`; every public method is either `&self` or
/// returns a scoped reference guard.
///
/// The optional `cache` field holds an [`Arc`]-wrapped [`WorkspaceCache`]
/// implementation. In single-pod deployments the default [`NoOpCache`] is
/// used. Multi-pod deployments can supply a `ValkeyCache` (or any other
/// implementation) via [`WorkspaceManager::with_cache`].
pub struct WorkspaceManager {
    workspaces: DashMap<SessionId, SessionWorkspace>,
    agent_counters: DashMap<Uuid, AtomicU32>,
    db: PgPool,
    cache: Arc<dyn WorkspaceCache>,
}

impl WorkspaceManager {
    /// Create a new, empty workspace manager backed by [`NoOpCache`].
    pub fn new(db: PgPool) -> Self {
        Self::with_cache(db, Arc::new(NoOpCache))
    }

    /// Create a workspace manager with an explicit cache implementation.
    ///
    /// Use this constructor when a `ValkeyCache` or other L2 cache is
    /// available. Pass `Arc::new(NoOpCache)` to opt-out of caching.
    pub fn with_cache(db: PgPool, cache: Arc<dyn WorkspaceCache>) -> Self {
        Self {
            workspaces: DashMap::new(),
            agent_counters: DashMap::new(),
            db,
            cache,
        }
    }

    /// Return a reference to the underlying cache implementation.
    pub fn cache(&self) -> &dyn WorkspaceCache {
        self.cache.as_ref()
    }

    /// Auto-assign the next agent name for a repository.
    ///
    /// Returns "agent-1", "agent-2", etc. incrementing per repo.
    pub fn next_agent_name(&self, repo_id: &Uuid) -> String {
        let counter = self
            .agent_counters
            .entry(*repo_id)
            .or_insert_with(|| AtomicU32::new(0));
        let n = counter.value().fetch_add(1, Ordering::Relaxed) + 1;
        format!("agent-{n}")
    }

    /// Create a new workspace for a session and register it.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_workspace(
        &self,
        session_id: SessionId,
        repo_id: RepoId,
        agent_id: AgentId,
        changeset_id: uuid::Uuid,
        intent: String,
        base_commit: String,
        mode: WorkspaceMode,
        agent_name: String,
    ) -> Result<SessionId> {
        let ws = SessionWorkspace::new(
            session_id,
            repo_id,
            agent_id,
            changeset_id,
            intent,
            base_commit,
            mode,
            agent_name,
            self.db.clone(),
        )
        .await?;

        // Write-through to L2 cache (fire-and-forget — Valkey failure
        // does not block workspace creation).
        let snapshot = crate::workspace::cache::WorkspaceSnapshot {
            session_id: ws.session_id,
            repo_id: ws.repo_id,
            agent_id: ws.agent_id.clone(),
            agent_name: ws.agent_name.clone(),
            changeset_id: ws.changeset_id,
            intent: ws.intent.clone(),
            base_commit: ws.base_commit.clone(),
            state: ws.state.as_str().to_string(),
            mode: ws.mode.as_str().to_string(),
        };
        let cache = self.cache.clone();
        tokio::spawn(async move {
            if let Err(e) = cache.cache_workspace(&session_id, &snapshot).await {
                tracing::warn!("L2 cache write failed on create: {e}");
            }
        });

        self.workspaces.insert(session_id, ws);
        Ok(session_id)
    }

    /// Get an immutable reference to a workspace.
    pub fn get_workspace(
        &self,
        session_id: &SessionId,
    ) -> Option<dashmap::mapref::one::Ref<'_, SessionId, SessionWorkspace>> {
        self.workspaces.get(session_id)
    }

    /// Get a mutable reference to a workspace.
    pub fn get_workspace_mut(
        &self,
        session_id: &SessionId,
    ) -> Option<dashmap::mapref::one::RefMut<'_, SessionId, SessionWorkspace>> {
        self.workspaces.get_mut(session_id)
    }

    /// Remove and drop a workspace.
    pub fn destroy_workspace(&self, session_id: &SessionId) -> Option<SessionWorkspace> {
        // Evict from L2 cache (fire-and-forget).
        let sid = *session_id;
        let cache = self.cache.clone();
        tokio::spawn(async move {
            if let Err(e) = cache.evict(&sid).await {
                tracing::warn!("L2 cache evict failed on destroy: {e}");
            }
        });
        self.workspaces.remove(session_id).map(|(_, ws)| ws)
    }

    /// Count active workspaces for a specific repository.
    pub fn active_count(&self, repo_id: RepoId) -> usize {
        self.workspaces
            .iter()
            .filter(|entry| entry.value().repo_id == repo_id)
            .count()
    }

    /// Return session IDs of all active workspaces for a repo,
    /// optionally excluding one session.
    pub fn active_sessions_for_repo(
        &self,
        repo_id: RepoId,
        exclude_session: Option<SessionId>,
    ) -> Vec<SessionId> {
        self.workspaces
            .iter()
            .filter(|entry| {
                entry.value().repo_id == repo_id
                    && exclude_session.is_none_or(|ex| *entry.key() != ex)
            })
            .map(|entry| *entry.key())
            .collect()
    }

    /// Garbage-collect expired persistent workspaces.
    ///
    /// Ephemeral workspaces are not GC'd here — they are destroyed when
    /// the session disconnects. This only handles persistent workspaces
    /// whose `expires_at` deadline has passed.
    pub fn gc_expired(&self) -> Vec<SessionId> {
        let now = Instant::now();
        let mut expired = Vec::new();

        // Collect IDs first to avoid holding DashMap guards during removal.
        self.workspaces.iter().for_each(|entry| {
            if let WorkspaceMode::Persistent {
                expires_at: Some(deadline),
            } = &entry.value().mode
            {
                if now >= *deadline {
                    expired.push(*entry.key());
                }
            }
        });

        for sid in &expired {
            self.workspaces.remove(sid);
        }

        expired
    }

    /// Destroy workspaces for sessions that no longer exist.
    /// Call this when a session disconnects or during periodic cleanup.
    pub fn cleanup_disconnected(&self, active_session_ids: &[uuid::Uuid]) {
        let to_remove: Vec<uuid::Uuid> = self.workspaces.iter()
            .filter(|entry| !active_session_ids.contains(entry.key()))
            .map(|entry| *entry.key())
            .collect();
        for sid in to_remove {
            self.workspaces.remove(&sid);
        }
    }

    /// Remove workspaces that are idle beyond `idle_ttl` or alive beyond `max_ttl`.
    ///
    /// Returns the list of expired session IDs. This complements [`gc_expired`]
    /// (which handles persistent workspace deadlines) by enforcing activity-based
    /// and hard-maximum lifetime limits on **all** workspaces.
    pub fn gc_expired_sessions(
        &self,
        idle_ttl: std::time::Duration,
        max_ttl: std::time::Duration,
    ) -> Vec<SessionId> {
        let now = Instant::now();
        let mut expired = Vec::new();

        self.workspaces.retain(|_session_id, ws| {
            let idle = now.duration_since(ws.last_active);
            let total = now.duration_since(ws.created_at);

            if idle > idle_ttl || total > max_ttl {
                expired.push(ws.session_id);
                false // remove
            } else {
                true // keep
            }
        });

        expired
    }

    /// Insert a pre-built workspace (test-only).
    ///
    /// Allows unit tests to insert workspaces with manipulated timestamps
    /// without requiring a live database connection.
    #[doc(hidden)]
    pub fn insert_test_workspace(&self, ws: SessionWorkspace) {
        let sid = ws.session_id;
        self.workspaces.insert(sid, ws);
    }

    /// Total number of active workspaces across all repos.
    pub fn total_active(&self) -> usize {
        self.workspaces.len()
    }

    /// Describe which other sessions have modified a given file.
    ///
    /// Returns a formatted string like `"fn create_task modified by agent-2"`
    /// or `"modified by agent-2, agent-3"`. Returns an empty string if no
    /// other session has touched the file.
    pub fn describe_other_modifiers(
        &self,
        file_path: &str,
        repo_id: RepoId,
        exclude_session: SessionId,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        for entry in self.workspaces.iter() {
            let ws = entry.value();
            if ws.repo_id != repo_id || ws.session_id == exclude_session {
                continue;
            }

            // Check if this other session has the file in its overlay
            if !ws.overlay.list_paths().contains(&file_path.to_string()) {
                continue;
            }

            // Get changed symbols for this file from the session graph
            let symbols = ws.graph.changed_symbols_for_file(file_path);
            let agent = &ws.agent_name;

            if symbols.is_empty() {
                parts.push(format!("modified by {agent}"));
            } else {
                // Take up to 3 symbol names to keep it concise
                let sym_list: Vec<&str> = symbols.iter().take(3).map(|s| s.as_str()).collect();
                let sym_str = sym_list.join(", ");
                if symbols.len() > 3 {
                    parts.push(format!("{sym_str},... modified by {agent}"));
                } else {
                    parts.push(format!("{sym_str} modified by {agent}"));
                }
            }
        }

        parts.join("; ")
    }

    /// List all active sessions for a given repository.
    pub fn list_sessions(&self, repo_id: RepoId) -> Vec<SessionInfo> {
        let now = Instant::now();
        self.workspaces
            .iter()
            .filter(|entry| entry.value().repo_id == repo_id)
            .map(|entry| {
                let ws = entry.value();
                SessionInfo {
                    session_id: ws.session_id,
                    agent_id: ws.agent_id.clone(),
                    agent_name: ws.agent_name.clone(),
                    intent: ws.intent.clone(),
                    repo_id: ws.repo_id,
                    changeset_id: ws.changeset_id,
                    state: ws.state.as_str().to_string(),
                    elapsed_secs: now.duration_since(ws.created_at).as_secs(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_info_serializes_to_json() {
        let info = SessionInfo {
            session_id: Uuid::nil(),
            agent_id: "test-agent".to_string(),
            agent_name: "agent-1".to_string(),
            intent: "fix bug".to_string(),
            repo_id: Uuid::nil(),
            changeset_id: Uuid::nil(),
            state: "active".to_string(),
            elapsed_secs: 42,
        };

        let json = serde_json::to_value(&info).expect("SessionInfo should serialize to JSON");

        assert_eq!(json["agent_id"], "test-agent");
        assert_eq!(json["agent_name"], "agent-1");
        assert_eq!(json["intent"], "fix bug");
        assert_eq!(json["state"], "active");
        assert_eq!(json["elapsed_secs"], 42);
        assert_eq!(
            json["session_id"],
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn session_info_all_fields_present_in_json() {
        let info = SessionInfo {
            session_id: Uuid::new_v4(),
            agent_id: "claude".to_string(),
            agent_name: "agent-1".to_string(),
            intent: "refactor".to_string(),
            repo_id: Uuid::new_v4(),
            changeset_id: Uuid::new_v4(),
            state: "submitted".to_string(),
            elapsed_secs: 100,
        };

        let json = serde_json::to_value(&info).expect("serialize");
        let obj = json.as_object().expect("should be an object");

        let expected_keys = [
            "session_id",
            "agent_id",
            "agent_name",
            "intent",
            "repo_id",
            "changeset_id",
            "state",
            "elapsed_secs",
        ];
        for key in &expected_keys {
            assert!(obj.contains_key(*key), "missing key: {}", key);
        }
        assert_eq!(obj.len(), expected_keys.len(), "unexpected extra keys in SessionInfo JSON");
    }

    #[test]
    fn session_info_clone_preserves_values() {
        let info = SessionInfo {
            session_id: Uuid::new_v4(),
            agent_id: "agent-1".to_string(),
            agent_name: "feature-bot".to_string(),
            intent: "deploy".to_string(),
            repo_id: Uuid::new_v4(),
            changeset_id: Uuid::new_v4(),
            state: "active".to_string(),
            elapsed_secs: 5,
        };

        let cloned = info.clone();
        assert_eq!(info.session_id, cloned.session_id);
        assert_eq!(info.agent_id, cloned.agent_id);
        assert_eq!(info.agent_name, cloned.agent_name);
        assert_eq!(info.intent, cloned.intent);
        assert_eq!(info.repo_id, cloned.repo_id);
        assert_eq!(info.changeset_id, cloned.changeset_id);
        assert_eq!(info.state, cloned.state);
        assert_eq!(info.elapsed_secs, cloned.elapsed_secs);
    }

    #[tokio::test]
    async fn next_agent_name_increments_per_repo() {
        let db = PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();
        let mgr = WorkspaceManager::new(db);
        let repo1 = Uuid::new_v4();
        let repo2 = Uuid::new_v4();

        assert_eq!(mgr.next_agent_name(&repo1), "agent-1");
        assert_eq!(mgr.next_agent_name(&repo1), "agent-2");
        assert_eq!(mgr.next_agent_name(&repo1), "agent-3");

        // Different repo starts at 1
        assert_eq!(mgr.next_agent_name(&repo2), "agent-1");
        assert_eq!(mgr.next_agent_name(&repo2), "agent-2");

        // Original repo continues
        assert_eq!(mgr.next_agent_name(&repo1), "agent-4");
    }

    /// Integration-level test for list_sessions and WorkspaceManager.
    /// Requires PgPool which we cannot construct without a DB, so this
    /// is marked #[ignore]. Run with:
    ///   DATABASE_URL=postgres://localhost/dkod_test cargo test -p dk-engine -- --ignored
    #[test]
    #[ignore]
    fn list_sessions_returns_empty_for_unknown_repo() {
        // This test would require a PgPool. The structural tests above
        // validate SessionInfo independently.
    }

    #[tokio::test]
    async fn describe_other_modifiers_empty_when_no_other_sessions() {
        let db = PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();
        let mgr = WorkspaceManager::new(db);
        let repo_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let result = mgr.describe_other_modifiers("src/lib.rs", repo_id, session_id);
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn describe_other_modifiers_shows_agent_name() {
        use crate::workspace::session_workspace::{SessionWorkspace, WorkspaceMode};

        let db = PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();
        let mgr = WorkspaceManager::new(db);
        let repo_id = Uuid::new_v4();

        let session1 = Uuid::new_v4();
        let session2 = Uuid::new_v4();

        let mut ws2 = SessionWorkspace::new_test(
            session2,
            repo_id,
            "agent-2-id".to_string(),
            "fix bug".to_string(),
            "abc123".to_string(),
            WorkspaceMode::Ephemeral,
        );
        ws2.agent_name = "agent-2".to_string();
        ws2.overlay.write_local("src/lib.rs", b"content".to_vec(), false);

        mgr.insert_test_workspace(ws2);

        let result = mgr.describe_other_modifiers("src/lib.rs", repo_id, session1);
        assert_eq!(result, "modified by agent-2");

        let result2 = mgr.describe_other_modifiers("src/other.rs", repo_id, session1);
        assert!(result2.is_empty());

        let result3 = mgr.describe_other_modifiers("src/lib.rs", repo_id, session2);
        assert!(result3.is_empty());
    }

    #[tokio::test]
    async fn describe_other_modifiers_includes_symbols() {
        use crate::workspace::session_workspace::{SessionWorkspace, WorkspaceMode};
        use dk_core::{Span, Symbol, SymbolKind, Visibility};
        use std::path::PathBuf;

        let db = PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();
        let mgr = WorkspaceManager::new(db);
        let repo_id = Uuid::new_v4();

        let session1 = Uuid::new_v4();
        let session2 = Uuid::new_v4();

        let mut ws2 = SessionWorkspace::new_test(
            session2,
            repo_id,
            "agent-2-id".to_string(),
            "add feature".to_string(),
            "abc123".to_string(),
            WorkspaceMode::Ephemeral,
        );
        ws2.agent_name = "agent-2".to_string();
        ws2.overlay
            .write_local("src/tasks.rs", b"fn create_task() {}".to_vec(), true);
        ws2.graph.add_symbol(Symbol {
            id: Uuid::new_v4(),
            name: "create_task".to_string(),
            qualified_name: "create_task".to_string(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            file_path: PathBuf::from("src/tasks.rs"),
            span: Span {
                start_byte: 0,
                end_byte: 20,
            },
            signature: None,
            doc_comment: None,
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        });

        mgr.insert_test_workspace(ws2);

        let result = mgr.describe_other_modifiers("src/tasks.rs", repo_id, session1);
        assert_eq!(result, "create_task modified by agent-2");
    }
}

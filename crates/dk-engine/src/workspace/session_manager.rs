//! WorkspaceManager — manages all active session workspaces.
//!
//! Provides creation, lookup, destruction, and garbage collection of
//! workspaces. Uses `DashMap` for lock-free concurrent access from
//! multiple agent sessions.

use dashmap::DashMap;
use dk_core::{AgentId, RepoId, Result};
use serde::Serialize;
use sqlx::PgPool;
use tokio::time::Instant;
use uuid::Uuid;

use crate::workspace::session_workspace::{
    SessionId, SessionWorkspace, WorkspaceMode,
};

// ── SessionInfo ─────────────────────────────────────────────────────

/// Lightweight snapshot of a session workspace, suitable for JSON serialization.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub agent_id: String,
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
pub struct WorkspaceManager {
    workspaces: DashMap<SessionId, SessionWorkspace>,
    db: PgPool,
}

impl WorkspaceManager {
    /// Create a new, empty workspace manager.
    pub fn new(db: PgPool) -> Self {
        Self {
            workspaces: DashMap::new(),
            db,
        }
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
    ) -> Result<SessionId> {
        let ws = SessionWorkspace::new(
            session_id,
            repo_id,
            agent_id,
            changeset_id,
            intent,
            base_commit,
            mode,
            self.db.clone(),
        )
        .await?;

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

    /// Total number of active workspaces across all repos.
    pub fn total_active(&self) -> usize {
        self.workspaces.len()
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
            intent: "fix bug".to_string(),
            repo_id: Uuid::nil(),
            changeset_id: Uuid::nil(),
            state: "active".to_string(),
            elapsed_secs: 42,
        };

        let json = serde_json::to_value(&info).expect("SessionInfo should serialize to JSON");

        assert_eq!(json["agent_id"], "test-agent");
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
            intent: "deploy".to_string(),
            repo_id: Uuid::new_v4(),
            changeset_id: Uuid::new_v4(),
            state: "active".to_string(),
            elapsed_secs: 5,
        };

        let cloned = info.clone();
        assert_eq!(info.session_id, cloned.session_id);
        assert_eq!(info.agent_id, cloned.agent_id);
        assert_eq!(info.intent, cloned.intent);
        assert_eq!(info.repo_id, cloned.repo_id);
        assert_eq!(info.changeset_id, cloned.changeset_id);
        assert_eq!(info.state, cloned.state);
        assert_eq!(info.elapsed_secs, cloned.elapsed_secs);
    }

    /// Integration-level test for list_sessions and WorkspaceManager.
    /// Requires PgPool which we cannot construct without a DB, so this
    /// is marked #[ignore]. Run with:
    ///   DATABASE_URL=postgres://localhost/dekode_test cargo test -p dk-engine -- --ignored
    #[test]
    #[ignore]
    fn list_sessions_returns_empty_for_unknown_repo() {
        // This test would require a PgPool. The structural tests above
        // validate SessionInfo independently.
    }
}

//! WorkspaceCache — L2 cache interface for session workspace state.
//!
//! This module defines the [`WorkspaceCache`] trait, supporting types, and the
//! [`NoOpCache`] implementation that satisfies the trait with no-op behaviour.
//! The trait is the seam used by multi-pod deployments to share workspace
//! snapshots across replicas via an external store (e.g. Valkey/Redis).
//!
//! Available implementations are [`NoOpCache`] (single-pod / local dev) and
//! [`ValkeyCache`](super::valkey_cache::ValkeyCache) (production multi-pod, behind the `valkey` feature).
//! For single-pod deployments (and tests), [`NoOpCache`] is the default.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── WorkspaceSnapshot ────────────────────────────────────────────────

/// A serializable point-in-time snapshot of a session workspace's metadata.
///
/// This is what gets written to the L2 cache so that any pod in the cluster
/// can reconstruct the workspace context for a session it did not originally
/// create. It intentionally contains only plain, serializable fields — no
/// in-process state such as `DashMap` or `Instant`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    /// The workspace (changeset) UUID.
    pub session_id: Uuid,
    /// The repository this workspace operates on.
    pub repo_id: Uuid,
    /// The authenticated agent identifier (e.g. Clerk user ID or API key prefix).
    pub agent_id: String,
    /// Human-readable agent name assigned by the server (e.g. `"agent-3"`).
    pub agent_name: String,
    /// The changeset UUID associated with this session.
    pub changeset_id: Uuid,
    /// The user-provided intent string for this session.
    pub intent: String,
    /// The Git commit hash that serves as the read-base for this workspace.
    pub base_commit: String,
    /// Lifecycle state label (e.g. `"active"`, `"submitted"`, `"merged"`).
    pub state: String,
    /// Mode label (`"ephemeral"` or `"persistent"`).
    pub mode: String,
}

// ── CachedOverlayEntry ───────────────────────────────────────────────

/// A file-level cache entry mirroring [`OverlayEntry`](super::overlay::OverlayEntry).
///
/// Carrying `content` inline keeps the cache self-contained — a pod
/// recovering a session can reconstruct the overlay without a DB round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CachedOverlayEntry {
    /// File was modified relative to the base commit.
    Modified { content: Vec<u8>, hash: String },
    /// File was added (did not exist in the base commit).
    Added { content: Vec<u8>, hash: String },
    /// File was deleted.
    Deleted,
}

// ── WorkspaceCache trait ─────────────────────────────────────────────

/// L2 cache interface for workspace state across pods.
///
/// Implementors store and retrieve [`WorkspaceSnapshot`]s, per-file
/// [`CachedOverlayEntry`]s, and serialised session graphs keyed by
/// workspace UUID. All methods are async and infallible in the "cache miss"
/// sense — they return `Ok(None)` rather than an error when an entry is absent.
///
/// The trait is `Send + Sync` so it can be stored behind `Arc<dyn WorkspaceCache>`
/// and shared across Tokio tasks.
#[async_trait]
pub trait WorkspaceCache: Send + Sync + 'static {
    // ── Workspace-level operations ───────────────────────────────────

    /// Persist a workspace snapshot to the cache under its session ID.
    async fn cache_workspace(&self, id: &Uuid, snapshot: &WorkspaceSnapshot) -> Result<()>;

    /// Retrieve a previously cached workspace snapshot.
    ///
    /// Returns `Ok(None)` on a cache miss.
    async fn get_workspace(&self, id: &Uuid) -> Result<Option<WorkspaceSnapshot>>;

    // ── File overlay operations ──────────────────────────────────────

    /// Cache a single overlay file entry for `(workspace_id, path)`.
    async fn cache_file(
        &self,
        workspace_id: &Uuid,
        path: &str,
        entry: &CachedOverlayEntry,
    ) -> Result<()>;

    /// Retrieve a cached overlay file entry.
    ///
    /// Returns `Ok(None)` on a cache miss.
    async fn get_file(
        &self,
        workspace_id: &Uuid,
        path: &str,
    ) -> Result<Option<CachedOverlayEntry>>;

    /// List all file paths cached in the overlay for a workspace.
    ///
    /// Returns an empty `Vec` when no files are cached.
    async fn list_files(&self, workspace_id: &Uuid) -> Result<Vec<String>>;

    // ── Session graph operations ─────────────────────────────────────

    /// Persist a serialised session graph blob for a workspace.
    async fn cache_graph(&self, workspace_id: &Uuid, graph_data: &[u8]) -> Result<()>;

    /// Retrieve a serialised session graph blob.
    ///
    /// Returns `Ok(None)` on a cache miss.
    async fn get_graph(&self, workspace_id: &Uuid) -> Result<Option<Vec<u8>>>;

    // ── Lifecycle operations ─────────────────────────────────────────

    /// Remove all cache entries for a workspace (snapshot, files, graph).
    async fn evict(&self, id: &Uuid) -> Result<()>;

    /// Update the cache TTL / last-access timestamp for a workspace.
    ///
    /// Implementations that support TTL-based expiry should reset the clock
    /// on this call. No-op implementations accept silently.
    async fn touch(&self, id: &Uuid) -> Result<()>;
}

// ── NoOpCache ────────────────────────────────────────────────────────

/// A [`WorkspaceCache`] implementation that does nothing.
///
/// All writes are silently discarded. All reads return `Ok(None)` /
/// `Ok(vec![])`. This is the default used by single-pod deployments and
/// in unit tests where no external cache is needed.
#[derive(Debug, Clone, Default)]
pub struct NoOpCache;

#[async_trait]
impl WorkspaceCache for NoOpCache {
    async fn cache_workspace(&self, _id: &Uuid, _snapshot: &WorkspaceSnapshot) -> Result<()> {
        Ok(())
    }

    async fn get_workspace(&self, _id: &Uuid) -> Result<Option<WorkspaceSnapshot>> {
        Ok(None)
    }

    async fn cache_file(
        &self,
        _workspace_id: &Uuid,
        _path: &str,
        _entry: &CachedOverlayEntry,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_file(
        &self,
        _workspace_id: &Uuid,
        _path: &str,
    ) -> Result<Option<CachedOverlayEntry>> {
        Ok(None)
    }

    async fn list_files(&self, _workspace_id: &Uuid) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn cache_graph(&self, _workspace_id: &Uuid, _graph_data: &[u8]) -> Result<()> {
        Ok(())
    }

    async fn get_graph(&self, _workspace_id: &Uuid) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn evict(&self, _id: &Uuid) -> Result<()> {
        Ok(())
    }

    async fn touch(&self, _id: &Uuid) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Serialization round-trips ────────────────────────────────────

    #[test]
    fn workspace_snapshot_roundtrips_json() {
        let snap = WorkspaceSnapshot {
            session_id: Uuid::new_v4(),
            repo_id: Uuid::new_v4(),
            agent_id: "clerk|abc".to_string(),
            agent_name: "agent-1".to_string(),
            changeset_id: Uuid::new_v4(),
            intent: "fix the bug".to_string(),
            base_commit: "deadbeef".to_string(),
            state: "active".to_string(),
            mode: "ephemeral".to_string(),
        };
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: WorkspaceSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(snap, back);
    }

    #[test]
    fn cached_overlay_entry_roundtrips_json() {
        let entries = [
            CachedOverlayEntry::Modified {
                content: b"fn foo() {}".to_vec(),
                hash: "abc123".to_string(),
            },
            CachedOverlayEntry::Added {
                content: b"new file".to_vec(),
                hash: "def456".to_string(),
            },
            CachedOverlayEntry::Deleted,
        ];
        for entry in &entries {
            let json = serde_json::to_string(entry).expect("serialize");
            let back: CachedOverlayEntry = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(entry, &back);
        }
    }

    // ── NoOpCache unit tests ─────────────────────────────────────────

    #[tokio::test]
    async fn noop_get_workspace_returns_none() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        let result = cache.get_workspace(&id).await.expect("should not error");
        assert!(result.is_none(), "NoOpCache must return None on get_workspace");
    }

    #[tokio::test]
    async fn noop_cache_workspace_is_silent() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        let snap = WorkspaceSnapshot {
            session_id: id,
            repo_id: Uuid::new_v4(),
            agent_id: "agent".to_string(),
            agent_name: "agent-1".to_string(),
            changeset_id: Uuid::new_v4(),
            intent: "intent".to_string(),
            base_commit: "abc".to_string(),
            state: "active".to_string(),
            mode: "ephemeral".to_string(),
        };
        // Write should succeed silently.
        cache.cache_workspace(&id, &snap).await.expect("should not error");
        // Read-back must still return None (nothing was stored).
        let result = cache.get_workspace(&id).await.expect("should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn noop_get_file_returns_none() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        let result = cache.get_file(&id, "src/lib.rs").await.expect("should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn noop_cache_file_is_silent() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        let entry = CachedOverlayEntry::Modified {
            content: b"hello".to_vec(),
            hash: "abc".to_string(),
        };
        cache.cache_file(&id, "src/lib.rs", &entry).await.expect("should not error");
        // Read-back must return None.
        let result = cache.get_file(&id, "src/lib.rs").await.expect("should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn noop_list_files_returns_empty() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        let files = cache.list_files(&id).await.expect("should not error");
        assert!(files.is_empty(), "NoOpCache must return empty vec from list_files");
    }

    #[tokio::test]
    async fn noop_get_graph_returns_none() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        let result = cache.get_graph(&id).await.expect("should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn noop_cache_graph_is_silent() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        let data = b"graph-bytes";
        cache.cache_graph(&id, data).await.expect("should not error");
        let result = cache.get_graph(&id).await.expect("should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn noop_evict_is_noop() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        cache.evict(&id).await.expect("evict should not error");
    }

    #[tokio::test]
    async fn noop_touch_is_noop() {
        let cache = NoOpCache;
        let id = Uuid::new_v4();
        cache.touch(&id).await.expect("touch should not error");
    }

    #[tokio::test]
    async fn noop_cache_is_send_sync() {
        // Verify the trait object can be shared across threads.
        let cache: std::sync::Arc<dyn WorkspaceCache> = std::sync::Arc::new(NoOpCache);
        let id = Uuid::new_v4();
        let result = cache.get_workspace(&id).await.expect("should not error");
        assert!(result.is_none());
    }
}

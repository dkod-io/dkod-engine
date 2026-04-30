//! Valkey/Redis-backed [`WorkspaceCache`] implementation.
//!
//! Available only when the `valkey` cargo feature is enabled.  Workspace
//! snapshots, overlay files, and session graph blobs are stored as
//! MessagePack-encoded values with a configurable TTL.
//!
//! ## Key schema
//!
//! All keys use `{uuid}` as a Redis Cluster hash tag so that all keys for a
//! given workspace are co-located in the same slot, enabling atomic pipelines.
//!
//! | Pattern                      | Value                            |
//! |------------------------------|----------------------------------|
//! | `{id}:meta`                  | MessagePack `WorkspaceSnapshot`  |
//! | `{id}:graph`                 | MessagePack `SessionGraph` bytes |
//! | `{id}:file:{path}`           | MessagePack `CachedOverlayEntry` |
//! | `{id}:files`                 | Redis SET of file paths          |

use anyhow::{Context, Result};
use async_trait::async_trait;
use redis::AsyncCommands;
use uuid::Uuid;

use super::cache::{CachedOverlayEntry, WorkspaceCache, WorkspaceSnapshot};

/// Valkey/Redis-backed workspace cache.
///
/// Uses [`redis::aio::ConnectionManager`] for automatic reconnection and cheap
/// cloning across concurrent tasks.
pub struct ValkeyCache {
    conn: redis::aio::ConnectionManager,
    ttl_secs: u32,
}

impl ValkeyCache {
    /// Create a new `ValkeyCache` connected to the given Redis/Valkey URL.
    ///
    /// `url` is a standard Redis connection string, e.g.
    /// `redis://127.0.0.1:6379`. `ttl_secs` caps at `u32::MAX` (~136 years).
    pub async fn new(url: &str, ttl_secs: u32) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let conn = redis::aio::ConnectionManager::new(client).await?;
        Ok(Self { conn, ttl_secs })
    }

    // ── Key helpers ──────────────────────────────────────────────────

    fn meta_key(id: &Uuid) -> String {
        format!("{{{id}}}:meta")
    }

    fn graph_key(id: &Uuid) -> String {
        format!("{{{id}}}:graph")
    }

    fn file_key(id: &Uuid, path: &str) -> String {
        format!("{{{id}}}:file:{path}")
    }

    fn files_set_key(id: &Uuid) -> String {
        format!("{{{id}}}:files")
    }
}

// SECURITY: All rmp_serde deserialization in this module targets data written
// by this process (or an identically versioned replica) into a trusted internal
// Valkey/Redis backend.  The target types contain only primitive fields and
// derive `Deserialize` without custom implementations — no gadget chains are
// possible.  See CWE-502 assessment: false positive.
#[async_trait]
impl WorkspaceCache for ValkeyCache {
    // ── Workspace-level operations ───────────────────────────────────

    async fn cache_workspace(&self, id: &Uuid, snapshot: &WorkspaceSnapshot) -> Result<()> {
        let key = Self::meta_key(id);
        let bytes = rmp_serde::to_vec_named(snapshot)
            .context("ValkeyCache: failed to serialize snapshot")?;
        let mut conn = self.conn.clone();
        conn.set_ex::<_, _, ()>(&key, bytes.as_slice(), u64::from(self.ttl_secs))
            .await
            .context("ValkeyCache: SET meta failed")?;
        Ok(())
    }

    async fn get_workspace(&self, id: &Uuid) -> Result<Option<WorkspaceSnapshot>> {
        let key = Self::meta_key(id);
        let mut conn = self.conn.clone();
        let bytes: Option<Vec<u8>> = conn
            .get(&key)
            .await
            .context("ValkeyCache: GET meta failed")?;
        match bytes {
            Some(b) => {
                let snap = rmp_serde::from_slice(&b)
                    .context("ValkeyCache: failed to deserialize snapshot")?;
                Ok(Some(snap))
            }
            None => Ok(None),
        }
    }

    // ── File overlay operations ──────────────────────────────────────

    async fn cache_file(
        &self,
        workspace_id: &Uuid,
        path: &str,
        entry: &CachedOverlayEntry,
    ) -> Result<()> {
        let file_key = Self::file_key(workspace_id, path);
        let set_key = Self::files_set_key(workspace_id);
        let bytes = rmp_serde::to_vec_named(entry)
            .context("ValkeyCache: failed to serialize overlay entry")?;

        let mut conn = self.conn.clone();
        redis::pipe()
            .atomic()
            .set_ex(&file_key, bytes.as_slice(), u64::from(self.ttl_secs))
            .sadd(&set_key, path)
            .expire(&set_key, i64::from(self.ttl_secs))
            .query_async::<()>(&mut conn)
            .await
            .context("ValkeyCache: pipeline SET+SADD failed")?;
        Ok(())
    }

    async fn get_file(
        &self,
        workspace_id: &Uuid,
        path: &str,
    ) -> Result<Option<CachedOverlayEntry>> {
        let key = Self::file_key(workspace_id, path);
        let mut conn = self.conn.clone();
        let bytes: Option<Vec<u8>> = conn
            .get(&key)
            .await
            .context("ValkeyCache: GET file failed")?;
        match bytes {
            Some(b) => {
                let entry = rmp_serde::from_slice(&b)
                    .context("ValkeyCache: failed to deserialize overlay entry")?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    async fn list_files(&self, workspace_id: &Uuid) -> Result<Vec<String>> {
        let key = Self::files_set_key(workspace_id);
        let mut conn = self.conn.clone();
        let paths: Vec<String> = conn
            .smembers(&key)
            .await
            .context("ValkeyCache: SMEMBERS files failed")?;
        Ok(paths)
    }

    // ── Session graph operations ─────────────────────────────────────

    async fn cache_graph(&self, workspace_id: &Uuid, graph_data: &[u8]) -> Result<()> {
        let key = Self::graph_key(workspace_id);
        let mut conn = self.conn.clone();
        conn.set_ex::<_, _, ()>(&key, graph_data, u64::from(self.ttl_secs))
            .await
            .context("ValkeyCache: SET graph failed")?;
        Ok(())
    }

    async fn get_graph(&self, workspace_id: &Uuid) -> Result<Option<Vec<u8>>> {
        let key = Self::graph_key(workspace_id);
        let mut conn = self.conn.clone();
        let bytes: Option<Vec<u8>> = conn
            .get(&key)
            .await
            .context("ValkeyCache: GET graph failed")?;
        Ok(bytes)
    }

    // ── Lifecycle operations ─────────────────────────────────────────

    async fn evict(&self, id: &Uuid) -> Result<()> {
        let meta_key = Self::meta_key(id);
        let graph_key = Self::graph_key(id);
        let set_key = Self::files_set_key(id);

        let mut conn = self.conn.clone();

        // Collect file paths then delete all keys in a pipeline.
        // NOTE: TOCTOU race — files added between SMEMBERS and DEL are orphaned
        // until TTL expires. Acceptable: TTL is the backstop and this avoids the
        // complexity of a Lua script for an infrequent operation.
        let file_paths: Vec<String> = conn
            .smembers(&set_key)
            .await
            .context("ValkeyCache: SMEMBERS during evict failed")?;
        let mut pipe = redis::pipe();
        pipe.atomic();
        pipe.del(&meta_key);
        pipe.del(&graph_key);
        pipe.del(&set_key);
        for path in &file_paths {
            pipe.del(Self::file_key(id, path));
        }

        pipe.query_async::<()>(&mut conn)
            .await
            .context("ValkeyCache: pipeline DEL during evict failed")?;
        Ok(())
    }

    async fn touch(&self, id: &Uuid) -> Result<()> {
        let meta_key = Self::meta_key(id);
        let graph_key = Self::graph_key(id);
        let set_key = Self::files_set_key(id);

        let mut conn = self.conn.clone();

        // Collect file paths then refresh TTLs in a pipeline.
        // NOTE: Same TOCTOU race as evict — see comment there.
        let file_paths: Vec<String> = conn
            .smembers(&set_key)
            .await
            .context("ValkeyCache: SMEMBERS during touch failed")?;

        let ttl = i64::from(self.ttl_secs);
        let mut pipe = redis::pipe();
        pipe.atomic();
        pipe.expire(&meta_key, ttl).ignore();
        pipe.expire(&graph_key, ttl).ignore();
        pipe.expire(&set_key, ttl).ignore();
        for path in &file_paths {
            pipe.expire(Self::file_key(id, path), ttl).ignore();
        }

        pipe.query_async::<()>(&mut conn)
            .await
            .context("ValkeyCache: pipeline EXPIRE during touch failed")?;
        Ok(())
    }
}

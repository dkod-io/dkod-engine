//! Redis-backed session store.
//!
//! Available only when the `redis` cargo feature is enabled.
//! Sessions are stored as JSON values with Redis TTL for automatic expiration.
//! Snapshots expire after 24 hours.

use async_trait::async_trait;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::warn;
use uuid::Uuid;

use crate::session::{AgentSession, SessionSnapshot};
use crate::session_store::{SessionId, SessionStore};

/// Key prefix for session data.
const SESSION_PREFIX: &str = "dk:session:";
/// Key prefix for snapshot data.
const SNAPSHOT_PREFIX: &str = "dk:snapshot:";
/// Snapshot TTL: 24 hours.
const SNAPSHOT_TTL_SECS: u64 = 86_400;

/// A serializable representation of an [`AgentSession`] for Redis storage.
///
/// `std::time::Instant` is not serializable, so we store `created_at` and
/// `last_active` as Unix-epoch milliseconds via `chrono`.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    id: Uuid,
    agent_id: String,
    codebase: String,
    intent: String,
    codebase_version: String,
    created_at_ms: i64,
    last_active_ms: i64,
}

impl StoredSession {
    fn from_parts(id: Uuid, agent_id: String, codebase: String, intent: String, codebase_version: String) -> Self {
        let now_ms = chrono::Utc::now().timestamp_millis();
        Self {
            id,
            agent_id,
            codebase,
            intent,
            codebase_version,
            created_at_ms: now_ms,
            last_active_ms: now_ms,
        }
    }

    fn into_agent_session(self) -> AgentSession {
        // Instant cannot be constructed from a timestamp, so we approximate:
        // - created_at: Instant::now() minus the elapsed time since creation
        // - last_active: Instant::now() minus the elapsed time since last touch
        let now_ms = chrono::Utc::now().timestamp_millis();
        let now_instant = Instant::now();

        let created_elapsed = Duration::from_millis((now_ms - self.created_at_ms).max(0) as u64);
        let active_elapsed = Duration::from_millis((now_ms - self.last_active_ms).max(0) as u64);

        AgentSession {
            id: self.id,
            agent_id: self.agent_id,
            codebase: self.codebase,
            intent: self.intent,
            codebase_version: self.codebase_version,
            created_at: now_instant - created_elapsed,
            last_active: now_instant - active_elapsed,
        }
    }
}

/// Redis-backed session store.
///
/// Uses `redis::aio::ConnectionManager` which automatically reconnects on
/// transient failures and is cheaply cloneable.
pub struct RedisSessionStore {
    conn: redis::aio::ConnectionManager,
    /// Session TTL — matches the DashMap timeout semantics.
    timeout: Duration,
}

impl RedisSessionStore {
    /// Create a new Redis session store.
    ///
    /// `redis_url` is a standard Redis connection string, e.g.
    /// `redis://127.0.0.1:6379`.
    pub async fn new(redis_url: &str, timeout: Duration) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = redis::aio::ConnectionManager::new(client).await?;
        Ok(Self { conn, timeout })
    }

    fn session_key(id: &Uuid) -> String {
        format!("{SESSION_PREFIX}{id}")
    }

    fn snapshot_key(id: &Uuid) -> String {
        format!("{SNAPSHOT_PREFIX}{id}")
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn create_session(
        &self,
        agent_id: String,
        codebase: String,
        intent: String,
        codebase_version: String,
    ) -> SessionId {
        let id = Uuid::new_v4();
        let stored = StoredSession::from_parts(id, agent_id, codebase, intent, codebase_version);
        let key = Self::session_key(&id);
        let ttl_secs = self.timeout.as_secs() as i64;

        let json = match serde_json::to_string(&stored) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize session: {e}");
                return id;
            }
        };

        let mut conn = self.conn.clone();
        if let Err(e) = conn.set_ex::<_, _, ()>(&key, &json, ttl_secs as u64).await {
            warn!("Redis SET failed for session {id}: {e}");
        }
        id
    }

    async fn get_session(&self, id: &SessionId) -> Option<AgentSession> {
        let key = Self::session_key(id);
        let mut conn = self.conn.clone();
        let json: Option<String> = conn.get(&key).await.ok()?;
        let json = json?;
        let stored: StoredSession = serde_json::from_str(&json).ok()?;
        Some(stored.into_agent_session())
    }

    async fn touch_session(&self, id: &SessionId) -> bool {
        let key = Self::session_key(id);
        let mut conn = self.conn.clone();

        // Read, update last_active_ms, write back with refreshed TTL.
        let json: Option<String> = match conn.get(&key).await {
            Ok(v) => v,
            Err(_) => return false,
        };
        let Some(json) = json else {
            return false;
        };

        let mut stored: StoredSession = match serde_json::from_str(&json) {
            Ok(s) => s,
            Err(_) => return false,
        };

        stored.last_active_ms = chrono::Utc::now().timestamp_millis();

        let updated = match serde_json::to_string(&stored) {
            Ok(j) => j,
            Err(_) => return false,
        };

        let ttl_secs = self.timeout.as_secs();
        conn.set_ex::<_, _, ()>(&key, &updated, ttl_secs)
            .await
            .is_ok()
    }

    async fn remove_session(&self, id: &SessionId) -> bool {
        let key = Self::session_key(id);
        let mut conn = self.conn.clone();
        let removed: i64 = conn.del(&key).await.unwrap_or(0);
        removed > 0
    }

    async fn cleanup_expired(&self) {
        // No-op: Redis TTL handles expiration automatically.
    }

    async fn save_snapshot(&self, id: &SessionId, snapshot: SessionSnapshot) {
        let key = Self::snapshot_key(id);
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize snapshot: {e}");
                return;
            }
        };
        let mut conn = self.conn.clone();
        if let Err(e) = conn
            .set_ex::<_, _, ()>(&key, &json, SNAPSHOT_TTL_SECS)
            .await
        {
            warn!("Redis SET failed for snapshot {id}: {e}");
        }
    }

    async fn take_snapshot(&self, id: &SessionId) -> Option<SessionSnapshot> {
        let key = Self::snapshot_key(id);
        let mut conn = self.conn.clone();
        let json: Option<String> = conn.get(&key).await.ok()?;
        let json = json?;

        // Delete after reading (take semantics).
        let _: Option<i64> = conn.del(&key).await.ok();

        serde_json::from_str(&json).ok()
    }
}

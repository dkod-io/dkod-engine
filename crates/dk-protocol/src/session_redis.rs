//! Redis-backed session store.
//!
//! Available only when the `redis` cargo feature is enabled.
//! Sessions are stored as JSON values with Redis TTL for automatic expiration.
//! Snapshots expire after 24 hours.
//!
//! ## Session expiry and snapshots
//!
//! When a session expires (via Redis TTL), there is no callback to save a
//! snapshot.  To support session resume, `remove_session` saves a snapshot
//! before deleting the key, and `cleanup_expired` scans for sessions whose
//! `last_active_ms` exceeds the configured timeout, migrates them to
//! snapshots, and removes the session key.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::Duration;
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
/// Timestamps are stored as Unix-epoch milliseconds so they survive
/// serialization round-trips and server restarts (unlike `std::time::Instant`
/// which is process-local and non-serializable).
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
        let created_at = millis_to_utc(self.created_at_ms);
        let last_active = millis_to_utc(self.last_active_ms);

        AgentSession {
            id: self.id,
            agent_id: self.agent_id,
            codebase: self.codebase,
            intent: self.intent,
            codebase_version: self.codebase_version,
            created_at,
            last_active,
        }
    }

    fn to_snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            agent_id: self.agent_id.clone(),
            codebase: self.codebase.clone(),
            intent: self.intent.clone(),
            codebase_version: self.codebase_version.clone(),
        }
    }

    /// Returns true if this session has exceeded the given timeout based on
    /// `last_active_ms`.
    fn is_expired(&self, timeout: &Duration) -> bool {
        let last_active = millis_to_utc(self.last_active_ms);
        let elapsed = Utc::now().signed_duration_since(last_active);
        let timeout_delta = chrono::TimeDelta::from_std(*timeout)
            .unwrap_or(chrono::TimeDelta::max_value());
        elapsed > timeout_delta
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

    /// Save a session's data as a snapshot before removing it, so the session
    /// can be resumed later via CONNECT.
    async fn save_snapshot_from_stored(&self, id: &Uuid, stored: &StoredSession) {
        let snapshot = stored.to_snapshot();
        let key = Self::snapshot_key(id);
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize snapshot for session {id}: {e}");
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
        let stored: StoredSession = match serde_json::from_str(&json) {
            Ok(s) => s,
            Err(_) => return None,
        };

        // Check application-level timeout in addition to Redis TTL.
        // After a server restart, the timeout config may differ from the
        // original TTL that was set on the key.
        if stored.is_expired(&self.timeout) {
            // Save a snapshot before removing so resume is possible.
            self.save_snapshot_from_stored(id, &stored).await;
            let _: Option<i64> = conn.del(&key).await.ok();
            return None;
        }

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

        // Read the session data before deleting so we can save a snapshot.
        let json: Option<String> = conn.get(&key).await.unwrap_or(None);
        if let Some(json) = json {
            if let Ok(stored) = serde_json::from_str::<StoredSession>(&json) {
                self.save_snapshot_from_stored(id, &stored).await;
            }
        }

        let removed: i64 = conn.del(&key).await.unwrap_or(0);
        removed > 0
    }

    async fn cleanup_expired(&self) {
        // Scan for sessions that have exceeded the application-level timeout.
        // Redis TTL is a safety net, but after a server restart the timeout
        // config may differ from the original TTL on the key.  This method
        // mirrors the in-memory SessionManager behavior: expired sessions are
        // converted to snapshots for resume support.
        let mut conn = self.conn.clone();
        let pattern = format!("{SESSION_PREFIX}*");
        let keys: Vec<String> = match redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
        {
            Ok(k) => k,
            Err(e) => {
                warn!("Redis KEYS scan failed during cleanup: {e}");
                return;
            }
        };

        for key in keys {
            let json: Option<String> = match conn.get(&key).await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(json) = json else { continue };
            let stored: StoredSession = match serde_json::from_str(&json) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if stored.is_expired(&self.timeout) {
                self.save_snapshot_from_stored(&stored.id, &stored).await;
                let _: Option<i64> = conn.del(&key).await.ok();
            }
        }
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

/// Convert Unix-epoch milliseconds to `DateTime<Utc>`.
fn millis_to_utc(ms: i64) -> DateTime<Utc> {
    let secs = ms / 1000;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nsecs)
        .single()
        .unwrap_or_else(Utc::now)
}

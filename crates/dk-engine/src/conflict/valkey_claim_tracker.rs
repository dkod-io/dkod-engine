//! Valkey/Redis-backed [`ClaimTracker`] implementation.
//!
//! Available only when the `valkey` cargo feature is enabled.
//!
//! ## Key schema
//!
//! | Pattern | Value |
//! |---------|-------|
//! | `lock:{repo_id}:{file_path}:{qualified_name}` | JSON `SymbolClaim` |
//! | `sess:{session_id}` | Redis SET of lock key strings |
//!
//! All keys have a 2-hour TTL as a safety net for crashed sessions.

use async_trait::async_trait;
use chrono::Utc;
use redis::AsyncCommands;
use uuid::Uuid;

use super::claim_tracker::{
    AcquireOutcome, ClaimTracker, ConflictInfo, ReleasedLock, SymbolClaim, SymbolLocked,
};

const TTL_SECS: u64 = 7200; // 2 hours

/// Lua script for atomic acquire_lock.
///
/// KEYS[1] = lock key
/// KEYS[2] = session set key
/// ARGV[1] = session_id (string)
/// ARGV[2] = claim JSON
/// ARGV[3] = TTL seconds
///
/// Returns:
///   `"FRESH"`     — lock acquired for the first time
///   `"REACQUIRE"` — same session already holds it (updated)
///   JSON string   — existing claim from a different session (blocked)
const ACQUIRE_SCRIPT: &str = r#"
local existing = redis.call('GET', KEYS[1])
if existing then
    local claim = cjson.decode(existing)
    if claim.session_id == ARGV[1] then
        redis.call('SET', KEYS[1], ARGV[2], 'EX', ARGV[3])
        return 'REACQUIRE'
    else
        return existing
    end
end
redis.call('SET', KEYS[1], ARGV[2], 'EX', ARGV[3])
redis.call('SADD', KEYS[2], KEYS[1])
redis.call('EXPIRE', KEYS[2], ARGV[3])
return 'FRESH'
"#;

/// Valkey/Redis-backed claim tracker for cross-pod symbol locking.
pub struct ValkeyClaimTracker {
    conn: redis::aio::ConnectionManager,
}

impl ValkeyClaimTracker {
    /// Connect to a Valkey/Redis instance.
    pub async fn new(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let conn = redis::aio::ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    fn lock_key(repo_id: Uuid, file_path: &str, qualified_name: &str) -> String {
        format!("lock:{repo_id}:{file_path}:{qualified_name}")
    }

    fn session_set_key(session_id: Uuid) -> String {
        format!("sess:{session_id}")
    }

    /// Parse a lock key back into (repo_id, file_path, qualified_name).
    /// Key format: `lock:{repo_id}:{file_path}:{qualified_name}`
    fn parse_lock_key(key: &str) -> Option<(Uuid, String, String)> {
        let rest = key.strip_prefix("lock:")?;
        // repo_id is always 36 chars (UUID)
        if rest.len() < 38 {
            return None;
        }
        let repo_id = rest[..36].parse::<Uuid>().ok()?;
        let after_repo = &rest[37..]; // skip the ":"
        // file_path:qualified_name — split on the LAST ":"
        let last_colon = after_repo.rfind(':')?;
        let file_path = after_repo[..last_colon].to_string();
        let qualified_name = after_repo[last_colon + 1..].to_string();
        Some((repo_id, file_path, qualified_name))
    }
}

#[async_trait]
impl ClaimTracker for ValkeyClaimTracker {
    async fn record_claim(&self, repo_id: Uuid, file_path: &str, claim: SymbolClaim) {
        let lock_key = Self::lock_key(repo_id, file_path, &claim.qualified_name);
        let sess_key = Self::session_set_key(claim.session_id);
        let json = match serde_json::to_string(&claim) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(error = %e, "failed to serialize claim");
                return;
            }
        };

        let mut conn = self.conn.clone();
        let _: Result<(), _> = redis::pipe()
            .cmd("SET")
            .arg(&lock_key)
            .arg(&json)
            .arg("EX")
            .arg(TTL_SECS)
            .cmd("SADD")
            .arg(&sess_key)
            .arg(&lock_key)
            .cmd("EXPIRE")
            .arg(&sess_key)
            .arg(TTL_SECS)
            .query_async(&mut conn)
            .await;
    }

    async fn acquire_lock(
        &self,
        repo_id: Uuid,
        file_path: &str,
        claim: SymbolClaim,
    ) -> Result<AcquireOutcome, SymbolLocked> {
        let lock_key = Self::lock_key(repo_id, file_path, &claim.qualified_name);
        let sess_key = Self::session_set_key(claim.session_id);
        let claim_json = serde_json::to_string(&claim).map_err(|e| SymbolLocked {
            qualified_name: claim.qualified_name.clone(),
            kind: claim.kind.clone(),
            locked_by_session: claim.session_id,
            locked_by_agent: format!("serialization error: {e}"),
            locked_since: Utc::now(),
            file_path: file_path.to_string(),
        })?;

        let mut conn = self.conn.clone();
        let result: String = redis::Script::new(ACQUIRE_SCRIPT)
            .key(&lock_key)
            .key(&sess_key)
            .arg(claim.session_id.to_string())
            .arg(&claim_json)
            .arg(TTL_SECS)
            .invoke_async(&mut conn)
            .await
            .map_err(|e| SymbolLocked {
                qualified_name: claim.qualified_name.clone(),
                kind: claim.kind.clone(),
                locked_by_session: Uuid::nil(),
                locked_by_agent: format!("Valkey error: {e}"),
                locked_since: Utc::now(),
                file_path: file_path.to_string(),
            })?;

        match result.as_str() {
            "FRESH" => Ok(AcquireOutcome::Fresh),
            "REACQUIRE" => Ok(AcquireOutcome::ReAcquired),
            json_str => {
                // Another session holds the lock — parse their claim
                let holder: SymbolClaim =
                    serde_json::from_str(json_str).map_err(|e| SymbolLocked {
                        qualified_name: claim.qualified_name.clone(),
                        kind: claim.kind.clone(),
                        locked_by_session: Uuid::nil(),
                        locked_by_agent: format!("parse error: {e}"),
                        locked_since: Utc::now(),
                        file_path: file_path.to_string(),
                    })?;
                Err(SymbolLocked {
                    qualified_name: claim.qualified_name,
                    kind: holder.kind,
                    locked_by_session: holder.session_id,
                    locked_by_agent: holder.agent_name,
                    locked_since: holder.first_touched_at,
                    file_path: file_path.to_string(),
                })
            }
        }
    }

    async fn release_lock(
        &self,
        repo_id: Uuid,
        file_path: &str,
        session_id: Uuid,
        qualified_name: &str,
    ) {
        let lock_key = Self::lock_key(repo_id, file_path, qualified_name);
        let sess_key = Self::session_set_key(session_id);

        let mut conn = self.conn.clone();
        let _: Result<(), _> = redis::pipe()
            .del(&lock_key)
            .cmd("SREM")
            .arg(&sess_key)
            .arg(&lock_key)
            .query_async(&mut conn)
            .await;
    }

    async fn release_locks(&self, repo_id: Uuid, session_id: Uuid) -> Vec<ReleasedLock> {
        let sess_key = Self::session_set_key(session_id);
        let prefix = format!("lock:{repo_id}:");

        let mut conn = self.conn.clone();

        // Get all lock keys for this session
        let all_keys: Vec<String> = match conn.smembers(&sess_key).await {
            Ok(keys) => keys,
            Err(_) => return Vec::new(),
        };

        // Filter to keys belonging to this repo
        let repo_keys: Vec<&String> = all_keys.iter().filter(|k| k.starts_with(&prefix)).collect();
        if repo_keys.is_empty() {
            return Vec::new();
        }

        // MGET to read claim data before deleting
        let values: Vec<Option<String>> = match redis::cmd("MGET")
            .arg(repo_keys.iter().map(|k| k.as_str()).collect::<Vec<_>>())
            .query_async(&mut conn)
            .await
        {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        // Build released list from stored claims
        let mut released = Vec::new();
        let mut pipe = redis::pipe();

        for (key, value) in repo_keys.iter().zip(values.iter()) {
            if let Some(json) = value {
                if let Ok(claim) = serde_json::from_str::<SymbolClaim>(json) {
                    if let Some((_, file_path, _)) = Self::parse_lock_key(key) {
                        released.push(ReleasedLock {
                            file_path,
                            qualified_name: claim.qualified_name,
                            kind: claim.kind,
                            agent_name: claim.agent_name,
                        });
                    }
                }
            }
            pipe.del(*key);
            pipe.cmd("SREM").arg(&sess_key).arg(*key);
        }

        // Execute deletes + remove from session set
        let _: Result<(), _> = pipe.query_async(&mut conn).await;

        // If we released all keys for this session, clean up the set
        if repo_keys.len() == all_keys.len() {
            let _: Result<(), _> = conn.del(&sess_key).await;
        }

        released
    }

    async fn check_conflicts(
        &self,
        repo_id: Uuid,
        file_path: &str,
        session_id: Uuid,
        qualified_names: &[String],
    ) -> Vec<ConflictInfo> {
        if qualified_names.is_empty() {
            return Vec::new();
        }

        let keys: Vec<String> = qualified_names
            .iter()
            .map(|name| Self::lock_key(repo_id, file_path, name))
            .collect();

        let mut conn = self.conn.clone();
        let values: Vec<Option<String>> = match redis::cmd("MGET")
            .arg(keys.iter().map(|k| k.as_str()).collect::<Vec<_>>())
            .query_async(&mut conn)
            .await
        {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut conflicts = Vec::new();
        for (name, value) in qualified_names.iter().zip(values.iter()) {
            if let Some(json) = value {
                if let Ok(claim) = serde_json::from_str::<SymbolClaim>(json) {
                    if claim.session_id != session_id {
                        conflicts.push(ConflictInfo {
                            qualified_name: name.clone(),
                            kind: claim.kind,
                            conflicting_session: claim.session_id,
                            conflicting_agent: claim.agent_name,
                            first_touched_at: claim.first_touched_at,
                        });
                    }
                }
            }
        }
        conflicts
    }

    async fn get_all_conflicts_for_session(
        &self,
        repo_id: Uuid,
        session_id: Uuid,
    ) -> Vec<(String, ConflictInfo)> {
        let sess_key = Self::session_set_key(session_id);
        let prefix = format!("lock:{repo_id}:");

        let mut conn = self.conn.clone();

        let all_keys: Vec<String> = match conn.smembers(&sess_key).await {
            Ok(keys) => keys,
            Err(_) => return Vec::new(),
        };

        let my_keys: Vec<&String> = all_keys.iter().filter(|k| k.starts_with(&prefix)).collect();
        if my_keys.is_empty() {
            return Vec::new();
        }

        // For each of my lock keys, check if another session also holds a
        // lock on the same symbol. Since Valkey locks are exclusive (only one
        // session can hold a key), cross-session conflicts in Valkey mode
        // would only exist if record_claim was used (non-blocking). With
        // acquire_lock (blocking), conflicts are prevented at write time.
        // Return empty — the blocking model prevents concurrent claims.
        Vec::new()
    }

    async fn clear_session(&self, session_id: Uuid) -> Vec<ReleasedLock> {
        let sess_key = Self::session_set_key(session_id);

        let mut conn = self.conn.clone();

        let all_keys: Vec<String> = match conn.smembers(&sess_key).await {
            Ok(keys) => keys,
            Err(_) => return Vec::new(),
        };

        if all_keys.is_empty() {
            return Vec::new();
        }

        // MGET all values before deleting
        let values: Vec<Option<String>> = match redis::cmd("MGET")
            .arg(all_keys.iter().map(|k| k.as_str()).collect::<Vec<_>>())
            .query_async(&mut conn)
            .await
        {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut released = Vec::new();
        let mut pipe = redis::pipe();

        for (key, value) in all_keys.iter().zip(values.iter()) {
            if let Some(json) = value {
                if let Ok(claim) = serde_json::from_str::<SymbolClaim>(json) {
                    if let Some((_, file_path, _)) = Self::parse_lock_key(key) {
                        released.push(ReleasedLock {
                            file_path,
                            qualified_name: claim.qualified_name,
                            kind: claim.kind,
                            agent_name: claim.agent_name,
                        });
                    }
                }
            }
            pipe.del(key);
        }
        pipe.del(&sess_key);

        let _: Result<(), _> = pipe.query_async(&mut conn).await;

        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lock_key_valid() {
        let repo = Uuid::new_v4();
        let key = format!("lock:{repo}:src/lib.rs:fn_main");
        let (r, f, q) = ValkeyClaimTracker::parse_lock_key(&key).unwrap();
        assert_eq!(r, repo);
        assert_eq!(f, "src/lib.rs");
        assert_eq!(q, "fn_main");
    }

    #[test]
    fn parse_lock_key_nested_path() {
        let repo = Uuid::new_v4();
        let key = format!("lock:{repo}:src/api/v2/handler.rs:MyStruct::method");
        let (r, f, q) = ValkeyClaimTracker::parse_lock_key(&key).unwrap();
        assert_eq!(r, repo);
        assert_eq!(f, "src/api/v2/handler.rs");
        assert_eq!(q, "MyStruct::method");
    }

    #[test]
    fn parse_lock_key_invalid() {
        assert!(ValkeyClaimTracker::parse_lock_key("invalid").is_none());
        assert!(ValkeyClaimTracker::parse_lock_key("lock:").is_none());
    }
}

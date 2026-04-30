use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dk_core::SymbolKind;

/// A claim that a particular session has touched a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolClaim {
    pub session_id: Uuid,
    pub agent_name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub first_touched_at: DateTime<Utc>,
}

/// Information about a detected conflict: another session already claims
/// ownership of a symbol that the current session wants to modify.
#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub conflicting_session: Uuid,
    pub conflicting_agent: String,
    pub first_touched_at: DateTime<Utc>,
}

/// Information about a symbol lock held by another session.
/// Returned when `acquire_lock` finds the symbol is already locked.
#[derive(Debug, Clone)]
pub struct SymbolLocked {
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub locked_by_session: Uuid,
    pub locked_by_agent: String,
    pub locked_since: DateTime<Utc>,
    pub file_path: String,
}

/// Outcome of a successful `acquire_lock` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireOutcome {
    /// Lock freshly acquired — include in rollback list.
    Fresh,
    /// Session already held this lock — exclude from rollback.
    ReAcquired,
}

/// Result of releasing locks for a session. Contains the symbols that
/// were released, so callers can emit `symbol.lock.released` events.
#[derive(Debug, Clone)]
pub struct ReleasedLock {
    pub file_path: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub agent_name: String,
}

// ---------------------------------------------------------------------------
// ClaimTracker trait
// ---------------------------------------------------------------------------

/// Async trait for symbol-level claim tracking across sessions.
///
/// Implementations:
/// - [`LocalClaimTracker`] — in-memory DashMap (single-pod / tests)
/// - `ValkeyClaimTracker` — Valkey/Redis-backed (multi-pod, behind `valkey` feature)
#[async_trait]
pub trait ClaimTracker: Send + Sync {
    /// Record a symbol claim (non-blocking — does not reject on conflict).
    async fn record_claim(&self, repo_id: Uuid, file_path: &str, claim: SymbolClaim);

    /// Attempt to acquire a symbol lock. Returns `Err(SymbolLocked)` if
    /// another session already holds the symbol.
    async fn acquire_lock(
        &self,
        repo_id: Uuid,
        file_path: &str,
        claim: SymbolClaim,
    ) -> Result<AcquireOutcome, SymbolLocked>;

    /// Release a single symbol lock for a session in a specific file.
    async fn release_lock(
        &self,
        repo_id: Uuid,
        file_path: &str,
        session_id: Uuid,
        qualified_name: &str,
    );

    /// Release all locks held by a session for a specific repo.
    async fn release_locks(&self, repo_id: Uuid, session_id: Uuid) -> Vec<ReleasedLock>;

    /// Check whether any of the given symbols conflict with another session.
    async fn check_conflicts(
        &self,
        repo_id: Uuid,
        file_path: &str,
        session_id: Uuid,
        qualified_names: &[String],
    ) -> Vec<ConflictInfo>;

    /// Return all conflicts for a session across all file paths.
    ///
    /// This method is only meaningful for the non-blocking `record_claim` path
    /// where multiple sessions can record overlapping claims without rejection.
    /// Backends that use exclusive locking (e.g. `ValkeyClaimTracker`) may
    /// return an empty list since cross-session conflicts are prevented at
    /// write time by `acquire_lock`.
    async fn get_all_conflicts_for_session(
        &self,
        repo_id: Uuid,
        session_id: Uuid,
    ) -> Vec<(String, ConflictInfo)>;

    /// Remove all claims belonging to a session across ALL repos.
    async fn clear_session(&self, session_id: Uuid) -> Vec<ReleasedLock>;
}

// ---------------------------------------------------------------------------
// LocalClaimTracker — in-memory DashMap implementation
// ---------------------------------------------------------------------------

/// Thread-safe, lock-free tracker for symbol-level claims across sessions.
///
/// Key insight: two sessions modifying DIFFERENT symbols in the same file is
/// NOT a conflict. Only same-symbol modifications across sessions are TRUE
/// conflicts. This is dkod's core differentiator over line-based VCS.
///
/// The tracker is keyed by `(repo_id, file_path)` and stores a `Vec<SymbolClaim>`
/// for each file. DashMap provides fine-grained per-shard locking so reads are
/// effectively lock-free when not contending on the same shard.
pub struct LocalClaimTracker {
    /// Map from (repo_id, file_path) to the list of claims on that file.
    claims: DashMap<(Uuid, String), Vec<SymbolClaim>>,
}

impl LocalClaimTracker {
    pub fn new() -> Self {
        Self {
            claims: DashMap::new(),
        }
    }
}

impl Default for LocalClaimTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ClaimTracker for LocalClaimTracker {
    async fn record_claim(&self, repo_id: Uuid, file_path: &str, claim: SymbolClaim) {
        let key = (repo_id, file_path.to_string());
        let mut entry = self.claims.entry(key).or_default();
        let claims = entry.value_mut();

        if let Some(existing) = claims
            .iter_mut()
            .find(|c| c.session_id == claim.session_id && c.qualified_name == claim.qualified_name)
        {
            existing.kind = claim.kind;
            existing.agent_name = claim.agent_name;
        } else {
            claims.push(claim);
        }
    }

    async fn acquire_lock(
        &self,
        repo_id: Uuid,
        file_path: &str,
        claim: SymbolClaim,
    ) -> Result<AcquireOutcome, SymbolLocked> {
        let key = (repo_id, file_path.to_string());
        let mut entry = self.claims.entry(key).or_default();
        let claims = entry.value_mut();

        if let Some(existing) = claims
            .iter()
            .find(|c| c.qualified_name == claim.qualified_name && c.session_id != claim.session_id)
        {
            return Err(SymbolLocked {
                qualified_name: claim.qualified_name,
                kind: existing.kind.clone(),
                locked_by_session: existing.session_id,
                locked_by_agent: existing.agent_name.clone(),
                locked_since: existing.first_touched_at,
                file_path: file_path.to_string(),
            });
        }

        if let Some(existing) = claims
            .iter_mut()
            .find(|c| c.session_id == claim.session_id && c.qualified_name == claim.qualified_name)
        {
            existing.kind = claim.kind;
            existing.agent_name = claim.agent_name;
            return Ok(AcquireOutcome::ReAcquired);
        }

        claims.push(claim);
        Ok(AcquireOutcome::Fresh)
    }

    async fn release_lock(
        &self,
        repo_id: Uuid,
        file_path: &str,
        session_id: Uuid,
        qualified_name: &str,
    ) {
        let key = (repo_id, file_path.to_string());
        if let Some(mut entry) = self.claims.get_mut(&key) {
            entry
                .value_mut()
                .retain(|c| !(c.session_id == session_id && c.qualified_name == qualified_name));
        }
        self.claims.remove_if(&key, |_, v| v.is_empty());
    }

    async fn release_locks(&self, repo_id: Uuid, session_id: Uuid) -> Vec<ReleasedLock> {
        let mut released = Vec::new();
        let mut empty_keys = Vec::new();

        for mut entry in self.claims.iter_mut() {
            let key = entry.key().clone();
            if key.0 != repo_id {
                continue;
            }
            let file_path = &key.1;
            let claims = entry.value_mut();

            for claim in claims.iter().filter(|c| c.session_id == session_id) {
                released.push(ReleasedLock {
                    file_path: file_path.clone(),
                    qualified_name: claim.qualified_name.clone(),
                    kind: claim.kind.clone(),
                    agent_name: claim.agent_name.clone(),
                });
            }

            claims.retain(|c| c.session_id != session_id);
            if claims.is_empty() {
                empty_keys.push(key);
            }
        }

        for key in empty_keys {
            self.claims.remove_if(&key, |_, v| v.is_empty());
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
        let key = (repo_id, file_path.to_string());
        let Some(entry) = self.claims.get(&key) else {
            return Vec::new();
        };

        let mut conflicts = Vec::new();
        for name in qualified_names {
            for claim in entry.value() {
                if claim.qualified_name == *name && claim.session_id != session_id {
                    conflicts.push(ConflictInfo {
                        qualified_name: name.clone(),
                        kind: claim.kind.clone(),
                        conflicting_session: claim.session_id,
                        conflicting_agent: claim.agent_name.clone(),
                        first_touched_at: claim.first_touched_at,
                    });
                    break;
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
        let mut results = Vec::new();
        for entry in self.claims.iter() {
            let (entry_repo_id, file_path) = entry.key();
            if *entry_repo_id != repo_id {
                continue;
            }
            let claims = entry.value();

            let my_symbols: Vec<&SymbolClaim> = claims
                .iter()
                .filter(|c| c.session_id == session_id)
                .collect();

            for my_claim in &my_symbols {
                for other_claim in claims {
                    if other_claim.session_id != session_id
                        && other_claim.qualified_name == my_claim.qualified_name
                    {
                        results.push((
                            file_path.clone(),
                            ConflictInfo {
                                qualified_name: my_claim.qualified_name.clone(),
                                kind: my_claim.kind.clone(),
                                conflicting_session: other_claim.session_id,
                                conflicting_agent: other_claim.agent_name.clone(),
                                first_touched_at: other_claim.first_touched_at,
                            },
                        ));
                        break;
                    }
                }
            }
        }
        results
    }

    async fn clear_session(&self, session_id: Uuid) -> Vec<ReleasedLock> {
        let mut released = Vec::new();
        let mut empty_keys = Vec::new();
        for mut entry in self.claims.iter_mut() {
            let key = entry.key().clone();
            let file_path = &key.1;
            let claims = entry.value_mut();

            for claim in claims.iter().filter(|c| c.session_id == session_id) {
                released.push(ReleasedLock {
                    file_path: file_path.clone(),
                    qualified_name: claim.qualified_name.clone(),
                    kind: claim.kind.clone(),
                    agent_name: claim.agent_name.clone(),
                });
            }

            claims.retain(|c| c.session_id != session_id);
            if claims.is_empty() {
                empty_keys.push(key);
            }
        }
        for key in empty_keys {
            self.claims.remove_if(&key, |_, v| v.is_empty());
        }
        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claim(session_id: Uuid, agent: &str, name: &str, kind: SymbolKind) -> SymbolClaim {
        SymbolClaim {
            session_id,
            agent_name: agent.to_string(),
            qualified_name: name.to_string(),
            kind,
            first_touched_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn no_conflict_different_symbols_same_file() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .record_claim(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;

        let conflicts = tracker
            .check_conflicts(repo, "src/lib.rs", session_b, &["fn_b".to_string()])
            .await;
        assert!(
            conflicts.is_empty(),
            "different symbols should not conflict"
        );
    }

    #[tokio::test]
    async fn conflict_same_symbol() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .record_claim(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;

        let conflicts = tracker
            .check_conflicts(repo, "src/lib.rs", session_b, &["fn_a".to_string()])
            .await;
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].qualified_name, "fn_a");
        assert_eq!(conflicts[0].conflicting_session, session_a);
        assert_eq!(conflicts[0].conflicting_agent, "agent-1");
    }

    #[tokio::test]
    async fn claims_cleared_on_session_destroy() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .record_claim(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;

        tracker.clear_session(session_a).await;

        let conflicts = tracker
            .check_conflicts(repo, "src/lib.rs", session_b, &["fn_a".to_string()])
            .await;
        assert!(
            conflicts.is_empty(),
            "cleared session should not cause conflicts"
        );
    }

    #[tokio::test]
    async fn same_session_no_self_conflict() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();

        tracker
            .record_claim(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;
        tracker
            .record_claim(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;

        let conflicts = tracker
            .check_conflicts(repo, "src/lib.rs", session_a, &["fn_a".to_string()])
            .await;
        assert!(
            conflicts.is_empty(),
            "same session should not conflict with itself"
        );
    }

    #[tokio::test]
    async fn multiple_conflicts() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .record_claim(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;
        tracker
            .record_claim(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_b", SymbolKind::Function),
            )
            .await;

        let conflicts = tracker
            .check_conflicts(
                repo,
                "src/lib.rs",
                session_b,
                &["fn_a".to_string(), "fn_b".to_string()],
            )
            .await;
        assert_eq!(conflicts.len(), 2);

        let names: Vec<&str> = conflicts
            .iter()
            .map(|c| c.qualified_name.as_str())
            .collect();
        assert!(names.contains(&"fn_a"));
        assert!(names.contains(&"fn_b"));
    }

    #[tokio::test]
    async fn acquire_lock_unclaimed_succeeds() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session = Uuid::new_v4();

        let result = tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn acquire_lock_same_session_succeeds() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session = Uuid::new_v4();

        tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await
            .unwrap();

        let result = tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn acquire_lock_cross_session_blocked() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await
            .unwrap();

        let result = tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_b, "agent-2", "fn_a", SymbolKind::Function),
            )
            .await;
        assert!(result.is_err());
        let locked = result.unwrap_err();
        assert_eq!(locked.qualified_name, "fn_a");
        assert_eq!(locked.locked_by_session, session_a);
        assert_eq!(locked.locked_by_agent, "agent-1");
    }

    #[tokio::test]
    async fn acquire_lock_different_symbols_same_file() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await
            .unwrap();

        let result = tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_b, "agent-2", "fn_b", SymbolKind::Function),
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn release_lock_single_symbol() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await
            .unwrap();

        tracker
            .release_lock(repo, "src/lib.rs", session_a, "fn_a")
            .await;

        let result = tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_b, "agent-2", "fn_a", SymbolKind::Function),
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn release_lock_cleans_empty_entries() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session = Uuid::new_v4();

        tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await
            .unwrap();

        tracker
            .release_lock(repo, "src/lib.rs", session, "fn_a")
            .await;

        let key = (repo, "src/lib.rs".to_string());
        assert!(tracker.claims.get(&key).is_none());
    }

    #[tokio::test]
    async fn release_locks_returns_released_entries() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session = Uuid::new_v4();

        tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await
            .unwrap();
        tracker
            .acquire_lock(
                repo,
                "src/api.rs",
                make_claim(session, "agent-1", "handler", SymbolKind::Function),
            )
            .await
            .unwrap();

        let released = tracker.release_locks(repo, session).await;
        assert_eq!(released.len(), 2);

        let names: Vec<&str> = released.iter().map(|r| r.qualified_name.as_str()).collect();
        assert!(names.contains(&"fn_a"));
        assert!(names.contains(&"handler"));
    }

    #[tokio::test]
    async fn release_locks_unblocks_other_session() {
        let tracker = LocalClaimTracker::new();
        let repo = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_a, "agent-1", "fn_a", SymbolKind::Function),
            )
            .await
            .unwrap();

        assert!(tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_b, "agent-2", "fn_a", SymbolKind::Function),
            )
            .await
            .is_err());

        tracker.release_locks(repo, session_a).await;

        assert!(tracker
            .acquire_lock(
                repo,
                "src/lib.rs",
                make_claim(session_b, "agent-2", "fn_a", SymbolKind::Function),
            )
            .await
            .is_ok());
    }
}

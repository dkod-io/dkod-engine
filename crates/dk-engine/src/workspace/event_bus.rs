//! Per-repo event bus for cross-session awareness.
//!
//! Uses `tokio::sync::broadcast` channels to fan out workspace events
//! to all subscribers within the same repository. Each repo gets its
//! own channel, lazily created on first publish or subscribe.

use dashmap::DashMap;
use dk_core::RepoId;
use tokio::sync::broadcast;
use uuid::Uuid;

// ── Event types ──────────────────────────────────────────────────────

/// Events broadcast within a repository for cross-session coordination.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// A new session workspace was created.
    SessionCreated {
        session_id: Uuid,
        agent_id: String,
        intent: String,
    },

    /// A file was modified in a session's overlay.
    FileModified {
        session_id: Uuid,
        file_path: String,
    },

    /// A changeset was submitted for review/merge.
    ChangesetSubmitted {
        session_id: Uuid,
        files_modified: usize,
    },

    /// A changeset was merged into the repository.
    ChangesetMerged {
        session_id: Uuid,
        commit_hash: String,
    },

    /// A session disconnected (workspace may still be persistent).
    SessionDisconnected {
        session_id: Uuid,
    },
}

// ── RepoEventBus ─────────────────────────────────────────────────────

/// Default broadcast channel capacity per repo.
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

/// A per-repository event bus backed by `tokio::sync::broadcast`.
///
/// Channels are lazily created on first `publish` or `subscribe` for a
/// given repo. Slow consumers that fall behind will receive
/// `RecvError::Lagged`, which is non-fatal — they skip missed events.
pub struct RepoEventBus {
    channels: DashMap<RepoId, broadcast::Sender<SessionEvent>>,
}

impl RepoEventBus {
    /// Create a new, empty event bus.
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }

    /// Publish an event to all subscribers of the given repository.
    ///
    /// If no subscribers exist yet, the event is silently dropped (the
    /// channel is still created for future subscribers).
    pub fn publish(&self, repo_id: RepoId, event: SessionEvent) {
        let sender = self.get_or_create_sender(repo_id);
        // send() returns Err if there are no receivers, which is fine.
        let _ = sender.send(event);
    }

    /// Subscribe to events for a repository.
    ///
    /// Returns a `broadcast::Receiver` that yields `SessionEvent`s.
    pub fn subscribe(&self, repo_id: RepoId) -> broadcast::Receiver<SessionEvent> {
        let sender = self.get_or_create_sender(repo_id);
        sender.subscribe()
    }

    /// Number of repositories with active channels.
    pub fn active_repos(&self) -> usize {
        self.channels.len()
    }

    /// Number of active subscribers for a given repo.
    ///
    /// Returns 0 if no channel exists for the repo.
    pub fn subscriber_count(&self, repo_id: RepoId) -> usize {
        self.channels
            .get(&repo_id)
            .map(|s| s.receiver_count())
            .unwrap_or(0)
    }

    /// Remove channels with no active subscribers.
    pub fn prune_dead_channels(&self) {
        self.channels.retain(|_repo_id, sender| sender.receiver_count() > 0);
    }

    /// Get or lazily create the broadcast sender for a repo.
    fn get_or_create_sender(&self, repo_id: RepoId) -> broadcast::Sender<SessionEvent> {
        self.channels
            .entry(repo_id)
            .or_insert_with(|| broadcast::channel(DEFAULT_CHANNEL_CAPACITY).0)
            .value()
            .clone()
    }
}

impl Default for RepoEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_and_receive() {
        let bus = RepoEventBus::new();
        let repo = Uuid::new_v4();

        let mut rx = bus.subscribe(repo);

        bus.publish(
            repo,
            SessionEvent::SessionCreated {
                session_id: Uuid::new_v4(),
                agent_id: "agent-1".into(),
                intent: "fix bug".into(),
            },
        );

        let event = rx.recv().await.expect("should receive event");
        match event {
            SessionEvent::SessionCreated { agent_id, .. } => {
                assert_eq!(agent_id, "agent-1");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_subscriber_does_not_panic() {
        let bus = RepoEventBus::new();
        let repo = Uuid::new_v4();

        // Publishing with no subscribers should not panic.
        bus.publish(
            repo,
            SessionEvent::SessionDisconnected {
                session_id: Uuid::new_v4(),
            },
        );
    }

    #[test]
    fn subscriber_count() {
        let bus = RepoEventBus::new();
        let repo = Uuid::new_v4();

        assert_eq!(bus.subscriber_count(repo), 0);

        let _rx1 = bus.subscribe(repo);
        assert_eq!(bus.subscriber_count(repo), 1);

        let _rx2 = bus.subscribe(repo);
        assert_eq!(bus.subscriber_count(repo), 2);
    }

    #[test]
    fn active_repos_count() {
        let bus = RepoEventBus::new();
        assert_eq!(bus.active_repos(), 0);

        let _rx = bus.subscribe(Uuid::new_v4());
        assert_eq!(bus.active_repos(), 1);

        let _rx2 = bus.subscribe(Uuid::new_v4());
        assert_eq!(bus.active_repos(), 2);
    }

    #[tokio::test]
    async fn multiple_subscribers_receive_same_event() {
        let bus = RepoEventBus::new();
        let repo = Uuid::new_v4();

        let mut rx1 = bus.subscribe(repo);
        let mut rx2 = bus.subscribe(repo);

        bus.publish(
            repo,
            SessionEvent::FileModified {
                session_id: Uuid::new_v4(),
                file_path: "src/main.rs".into(),
            },
        );

        let e1 = rx1.recv().await.expect("rx1 should receive");
        let e2 = rx2.recv().await.expect("rx2 should receive");

        match (e1, e2) {
            (
                SessionEvent::FileModified { file_path: p1, .. },
                SessionEvent::FileModified { file_path: p2, .. },
            ) => {
                assert_eq!(p1, "src/main.rs");
                assert_eq!(p2, "src/main.rs");
            }
            _ => panic!("both should receive FileModified"),
        }
    }
}

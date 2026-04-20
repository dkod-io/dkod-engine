use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::WatchEvent;

/// Special channel key that receives a copy of every event regardless of repo.
const ALL_CHANNEL: &str = "__all__";

/// Shared event bus for broadcasting repo events to watching agents.
///
/// Uses per-repo [`tokio::sync::broadcast`] channels so subscribers
/// only receive events for repos they care about.  A special "__all__"
/// channel receives a copy of every published event (used by the
/// platform bridge).
///
/// Events that are not consumed before the channel capacity (256) is
/// exhausted are silently dropped for lagged receivers.
#[derive(Clone)]
pub struct EventBus {
    channels: DashMap<String, broadcast::Sender<WatchEvent>>,
}

impl EventBus {
    /// Create a new event bus.
    pub fn new() -> Self {
        let channels = DashMap::new();
        // Pre-create the global "__all__" channel.
        let (tx, _) = broadcast::channel(256);
        channels.insert(ALL_CHANNEL.to_string(), tx);
        Self { channels }
    }

    /// Get or create the broadcast sender for the given key.
    fn get_or_create_sender(&self, key: &str) -> broadcast::Sender<WatchEvent> {
        self.channels
            .entry(key.to_string())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(256);
                tx
            })
            .clone()
    }

    /// Publish an event to a specific repo channel AND the global "__all__" channel.
    ///
    /// If there are no subscribers the event is silently discarded.
    pub fn publish(&self, event: WatchEvent) {
        let repo_id = &event.repo_id;

        // Publish to repo-specific channel if repo_id is set.
        if !repo_id.is_empty() {
            let tx = self.get_or_create_sender(repo_id);
            let _ = tx.send(event.clone());
        }

        // Always publish to the global "__all__" channel.
        if let Some(tx) = self.channels.get(ALL_CHANNEL) {
            let _ = tx.send(event);
        }
    }

    /// Subscribe to events for a specific repo.
    ///
    /// The receiver is created while the DashMap shard lock is held so that
    /// `cleanup_idle` cannot race and remove the channel between creation
    /// and subscription.
    pub fn subscribe(&self, repo_id: &str) -> broadcast::Receiver<WatchEvent> {
        self.channels
            .entry(repo_id.to_string())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(256);
                tx
            })
            .subscribe()
    }

    /// Subscribe to ALL events across all repos (for the platform bridge).
    pub fn subscribe_all(&self) -> broadcast::Receiver<WatchEvent> {
        self.channels
            .entry(ALL_CHANNEL.to_string())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(256);
                tx
            })
            .subscribe()
    }

    /// Remove the channel for a specific repo (e.g. when decommissioned).
    pub fn remove_repo(&self, repo_id: &str) {
        if repo_id != ALL_CHANNEL {
            self.channels.remove(repo_id);
        }
    }

    /// Remove channels that have no active receivers, excluding the global channel.
    /// Call periodically (e.g. every few minutes) to prevent unbounded growth.
    pub fn cleanup_idle(&self) {
        self.channels
            .retain(|key, sender| key == ALL_CHANNEL || sender.receiver_count() > 0);
    }
}

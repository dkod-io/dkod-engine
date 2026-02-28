use tokio::sync::broadcast;

use crate::WatchEvent;

/// Shared event bus for broadcasting repo events to watching agents.
///
/// Uses [`tokio::sync::broadcast`] so any number of subscribers can
/// receive a copy of every published event.  Events that are not
/// consumed before the channel capacity (256) is exhausted are
/// silently dropped for lagged receivers.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<WatchEvent>,
}

impl EventBus {
    /// Create a new event bus with a fixed capacity of 256 pending events.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    /// Publish an event to all current subscribers.
    ///
    /// If there are no subscribers the event is silently discarded.
    pub fn publish(&self, event: WatchEvent) {
        let _ = self.tx.send(event);
    }

    /// Create a new subscription.  The returned receiver will see all
    /// events published *after* this call.
    pub fn subscribe(&self) -> broadcast::Receiver<WatchEvent> {
        self.tx.subscribe()
    }
}

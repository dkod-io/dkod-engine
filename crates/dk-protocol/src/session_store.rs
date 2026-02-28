//! Trait abstraction for session storage.
//!
//! Allows swapping between in-memory (DashMap) and Redis-backed stores.

use async_trait::async_trait;
use uuid::Uuid;

use crate::session::{AgentSession, SessionSnapshot};

pub type SessionId = Uuid;

/// Trait for session storage backends.
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn create_session(
        &self,
        agent_id: String,
        codebase: String,
        intent: String,
        codebase_version: String,
    ) -> SessionId;

    async fn get_session(&self, id: &SessionId) -> Option<AgentSession>;
    async fn touch_session(&self, id: &SessionId) -> bool;
    async fn remove_session(&self, id: &SessionId) -> bool;
    async fn cleanup_expired(&self);
    async fn save_snapshot(&self, id: &SessionId, snapshot: SessionSnapshot);
    async fn take_snapshot(&self, id: &SessionId) -> Option<SessionSnapshot>;
}

/// In-memory session store backed by DashMap (default, no external deps).
#[async_trait]
impl SessionStore for crate::session::SessionManager {
    async fn create_session(
        &self,
        agent_id: String,
        codebase: String,
        intent: String,
        codebase_version: String,
    ) -> SessionId {
        self.create_session(agent_id, codebase, intent, codebase_version)
    }

    async fn get_session(&self, id: &SessionId) -> Option<AgentSession> {
        self.get_session(id)
    }

    async fn touch_session(&self, id: &SessionId) -> bool {
        self.touch_session(id)
    }

    async fn remove_session(&self, id: &SessionId) -> bool {
        self.remove_session(id)
    }

    async fn cleanup_expired(&self) {
        self.cleanup_expired()
    }

    async fn save_snapshot(&self, id: &SessionId, snapshot: SessionSnapshot) {
        self.save_snapshot(id, snapshot)
    }

    async fn take_snapshot(&self, id: &SessionId) -> Option<SessionSnapshot> {
        self.take_snapshot(id)
    }
}

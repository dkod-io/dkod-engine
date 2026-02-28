use dashmap::DashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub type SessionId = Uuid;

pub struct AgentSession {
    pub id: SessionId,
    pub agent_id: String,
    pub codebase: String,
    pub intent: String,
    pub codebase_version: String,
    pub created_at: Instant,
    pub last_active: Instant,
}

/// Snapshot of a session's identity info, saved when a session expires or
/// is explicitly removed, allowing a new CONNECT to resume it.
#[derive(Clone, Debug)]
pub struct SessionSnapshot {
    pub agent_id: String,
    pub codebase: String,
    pub intent: String,
    pub codebase_version: String,
}

pub struct SessionManager {
    sessions: DashMap<SessionId, AgentSession>,
    timeout: Duration,
    /// Snapshots of expired session workspaces for resume support.
    snapshots: DashMap<SessionId, SessionSnapshot>,
}

impl SessionManager {
    pub fn new(timeout: Duration) -> Self {
        Self {
            sessions: DashMap::new(),
            timeout,
            snapshots: DashMap::new(),
        }
    }

    pub fn create_session(
        &self,
        agent_id: String,
        codebase: String,
        intent: String,
        codebase_version: String,
    ) -> SessionId {
        let id = Uuid::new_v4();
        let now = Instant::now();
        self.sessions.insert(
            id,
            AgentSession {
                id,
                agent_id,
                codebase,
                intent,
                codebase_version,
                created_at: now,
                last_active: now,
            },
        );
        id
    }

    pub fn get_session(&self, id: &SessionId) -> Option<AgentSession> {
        let entry = self.sessions.get(id)?;
        if entry.last_active.elapsed() > self.timeout {
            drop(entry);
            self.sessions.remove(id);
            return None;
        }
        Some(AgentSession {
            id: entry.id,
            agent_id: entry.agent_id.clone(),
            codebase: entry.codebase.clone(),
            intent: entry.intent.clone(),
            codebase_version: entry.codebase_version.clone(),
            created_at: entry.created_at,
            last_active: entry.last_active,
        })
    }

    pub fn touch_session(&self, id: &SessionId) -> bool {
        if let Some(mut entry) = self.sessions.get_mut(id) {
            entry.last_active = Instant::now();
            true
        } else {
            false
        }
    }

    pub fn remove_session(&self, id: &SessionId) -> bool {
        self.sessions.remove(id).is_some()
    }

    /// Save a snapshot of a session for later resume.
    pub fn save_snapshot(&self, id: &SessionId, snapshot: SessionSnapshot) {
        self.snapshots.insert(*id, snapshot);
    }

    /// Retrieve and remove a saved session snapshot.
    pub fn take_snapshot(&self, id: &SessionId) -> Option<SessionSnapshot> {
        self.snapshots.remove(id).map(|(_, snap)| snap)
    }

    pub fn cleanup_expired(&self) {
        let mut expired = Vec::new();
        self.sessions.retain(|id, session| {
            let alive = session.last_active.elapsed() <= self.timeout;
            if !alive {
                expired.push((*id, SessionSnapshot {
                    agent_id: session.agent_id.clone(),
                    codebase: session.codebase.clone(),
                    intent: session.intent.clone(),
                    codebase_version: session.codebase_version.clone(),
                }));
            }
            alive
        });
        for (id, snap) in expired {
            self.snapshots.insert(id, snap);
        }
    }
}

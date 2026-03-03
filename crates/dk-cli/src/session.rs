//! Session state persistence for dk CLI.
//!
//! Stores the active session at `~/.config/dkod/session.json`.
//! `dk init` writes this file; all other commands read it.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub server: String,
    pub repo: String,
    pub session_id: String,
    pub changeset_id: String,
    pub workspace_id: String,
}

impl SessionState {
    /// Default session file path: `~/.config/dkod/session.json`
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dkod")
            .join("session.json")
    }

    pub fn load_from(path: &PathBuf) -> Result<Self> {
        let data = fs::read_to_string(path)
            .with_context(|| format!("no active session — run `dk init <repo>` first (looked at {})", path.display()))?;
        serde_json::from_str(&data).context("corrupt session file — run `dk init <repo>` to reset")
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::default_path())
    }

    pub fn save_to(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("failed to create config directory")?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, &data)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::default_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn round_trip_session_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let state = SessionState {
            server: "https://agent.dkod.io:443".to_string(),
            repo: "my-repo".to_string(),
            session_id: "sess-123".to_string(),
            changeset_id: "cs-456".to_string(),
            workspace_id: "ws-789".to_string(),
        };
        state.save_to(&path).unwrap();
        let loaded = SessionState::load_from(&path).unwrap();
        assert_eq!(loaded.server, "https://agent.dkod.io:443");
        assert_eq!(loaded.repo, "my-repo");
        assert_eq!(loaded.session_id, "sess-123");
    }

    #[test]
    fn load_returns_error_when_missing() {
        let result = SessionState::load_from(&PathBuf::from("/nonexistent/session.json"));
        assert!(result.is_err());
    }

    #[test]
    fn load_returns_error_on_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"not json").unwrap();
        assert!(SessionState::load_from(&path).is_err());
    }
}

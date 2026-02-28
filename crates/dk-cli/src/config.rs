use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub grpc_url: Option<String>,
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("could not determine config directory")?
            .join("dekode");
        Ok(dir.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .context("failed to read config file")?;
        toml::from_str(&content).context("failed to parse config file")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("failed to create config directory")?;
        }
        let content = toml::to_string_pretty(self)
            .context("failed to serialize config")?;
        std::fs::write(&path, &content)
            .context("failed to write config file")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn require_auth(&self) -> Result<(&str, &str)> {
        let url = self.server.url.as_deref()
            .context("not logged in — run `dk login <url>` first")?;
        let token = self.server.token.as_deref()
            .context("not logged in — run `dk login <url>` first")?;
        Ok((url, token))
    }
}

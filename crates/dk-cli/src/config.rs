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
            .join("dkod");
        Ok(dir.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        Self::load_from_path(&path)
    }

    pub fn load_from_path(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path).context("failed to read config file")?;
        toml::from_str(&content).context("failed to parse config file")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("failed to create config directory")?;
        }
        let content = toml::to_string_pretty(self).context("failed to serialize config")?;
        std::fs::write(&path, &content).context("failed to write config file")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn require_auth(&self) -> Result<(&str, &str)> {
        let url = self
            .server
            .url
            .as_deref()
            .context("not logged in — run `dk login <url>` first")?;
        let token = self
            .server
            .token
            .as_deref()
            .context("not logged in — run `dk login <url>` first")?;
        Ok((url, token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_not_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let non_existent_path = temp_dir.path().join("does_not_exist.toml");

        let config = Config::load_from_path(&non_existent_path).unwrap();
        assert!(config.server.url.is_none());
        assert!(config.server.token.is_none());
        assert!(config.server.grpc_url.is_none());
    }

    #[test]
    fn test_load_valid_toml() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let toml_content = r#"
            [server]
            url = "https://example.com"
            token = "secret_token"
        "#;
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load_from_path(temp_file.path()).unwrap();
        assert_eq!(config.server.url.as_deref(), Some("https://example.com"));
        assert_eq!(config.server.token.as_deref(), Some("secret_token"));
        assert!(config.server.grpc_url.is_none());
    }

    #[test]
    fn test_load_invalid_toml() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let invalid_toml = r#"
            [server
            url = "https://example.com"
        "#;
        temp_file.write_all(invalid_toml.as_bytes()).unwrap();

        let result = Config::load_from_path(temp_file.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert_eq!(err_msg, "failed to parse config file");
    }

    #[test]
    fn test_load_read_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        // Trying to read a directory as a file should result in a read error
        let result = Config::load_from_path(temp_dir.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert_eq!(err_msg, "failed to read config file");
    }
}

//! Token resolution for dk CLI authentication.
//!
//! Resolution priority:
//! 1. `DKOD_AUTH_TOKEN` env var (CI/automation)
//! 2. Cached token at `~/.config/dkod/token.json`
//! 3. OAuth device flow via `dk login`

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Serialize, Deserialize)]
struct CachedToken {
    token: String,
}

fn token_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dkod")
        .join("token.json")
}

fn read_cached_token_from(path: &PathBuf) -> Option<String> {
    let data = fs::read_to_string(path).ok()?;
    let cached: CachedToken = serde_json::from_str(&data).ok()?;
    if cached.token.is_empty() {
        return None;
    }
    Some(cached.token)
}

fn read_cached_token() -> Option<String> {
    read_cached_token_from(&token_path())
}

fn save_token_to(path: &PathBuf, token: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string(&CachedToken {
        token: token.to_string(),
    })?;
    fs::write(path, &data)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

pub fn save_token(token: &str) -> Result<()> {
    save_token_to(&token_path(), token)
}

#[derive(Deserialize)]
struct StartResponse {
    device_code: String,
    user_code: String,
    verification_url: String,
    #[allow(dead_code)]
    expires_in: u64,
}

#[derive(Deserialize)]
struct PollResponse {
    status: String,
    token: Option<String>,
}

pub async fn resolve_token(api_base: &str, env_token: Option<&str>) -> Result<String> {
    if let Some(token) = env_token {
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }
    if let Some(token) = read_cached_token() {
        return Ok(token);
    }
    run_device_flow(api_base).await
}

pub fn api_base_from_grpc(grpc_addr: &str) -> String {
    if grpc_addr.contains("localhost") || grpc_addr.contains("[::1]") || grpc_addr.contains("127.0.0.1") {
        "http://localhost:8080".to_string()
    } else {
        "https://api.dkod.io".to_string()
    }
}

pub async fn run_device_flow(api_base: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let start: StartResponse = client
        .post(format!("{api_base}/api/auth/device/start"))
        .send()
        .await?
        .json()
        .await
        .context("failed to start device flow")?;

    println!();
    println!("  To authenticate, open this URL in your browser:");
    println!();
    println!("    {}", start.verification_url);
    println!();
    println!("  Your code: {}", start.user_code);
    println!();

    let _ = open::that(&start.verification_url);

    let deadline = tokio::time::Instant::now() + POLL_TIMEOUT;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("device flow timed out after 15 minutes");
        }

        let resp: PollResponse = client
            .get(format!(
                "{api_base}/api/auth/device/poll?code={}",
                start.device_code
            ))
            .send()
            .await?
            .json()
            .await?;

        if resp.status == "complete" {
            if let Some(token) = resp.token {
                save_token(&token)?;
                println!("  Authenticated successfully!");
                println!();
                return Ok(token);
            }
        } else if resp.status == "denied" || resp.status == "expired" {
            anyhow::bail!("authentication {} by user", resp.status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn token_path_is_under_config_dir() {
        let path = token_path();
        assert!(path.ends_with("dkod/token.json"));
    }

    #[test]
    fn cached_token_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dkod").join("token.json");
        save_token_to(&path, "test-token-123").unwrap();
        let loaded = read_cached_token_from(&path);
        assert_eq!(loaded, Some("test-token-123".to_string()));
    }

    #[test]
    fn empty_cached_token_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(br#"{"token":""}"#).unwrap();
        assert_eq!(read_cached_token_from(&path), None);
    }

    #[tokio::test]
    async fn env_token_takes_priority() {
        let result = resolve_token("http://localhost:9999", Some("env-tok")).await;
        assert_eq!(result.unwrap(), "env-tok");
    }

    #[tokio::test]
    async fn empty_env_token_is_skipped() {
        // Empty env token should not be returned — resolve_token should skip it
        // and fall through to cached token or device flow.
        let result = resolve_token("http://localhost:9999", Some("")).await;
        // If there's a cached token, it succeeds; otherwise it errors.
        // Either way, the empty string must NOT be the result.
        if let Ok(ref token) = result {
            assert!(!token.is_empty(), "empty env token should have been skipped");
        }
    }
}

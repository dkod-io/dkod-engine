//! Token resolution for dk-mcp authentication.
//!
//! Resolution priority: env var → cached file → OAuth device flow.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(900); // 15 minutes

// ── Token cache ────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct CachedToken {
    token: String,
    /// Epoch seconds after which the cached token should be considered expired.
    #[serde(default)]
    valid_until: Option<u64>,
}

/// Conservative client-side TTL applied when the server does not provide an
/// explicit expiry.  The device-flow tokens issued by Clerk last 60 days;
/// 30 days gives plenty of margin while still avoiding silent staleness.
const DEFAULT_TTL_SECS: u64 = 30 * 24 * 3600; // 30 days

fn token_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dkod")
        .join("token.json")
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_cached_token() -> Option<String> {
    let path = token_path();
    let data = fs::read_to_string(&path).ok()?;
    let cached: CachedToken = serde_json::from_str(&data).ok()?;
    if cached.token.is_empty() {
        return None;
    }
    // Treat token as expired when valid_until is set and in the past.
    // Legacy cache files without the field are accepted (valid_until == None).
    if let Some(expiry) = cached.valid_until {
        if now_epoch() >= expiry {
            // Silently discard — the caller will trigger a fresh device flow.
            return None;
        }
    }
    Some(cached.token)
}

/// Remove the cached token file, forcing the next `resolve_token` call to
/// trigger a fresh device flow.
pub fn clear_cached_token() {
    let path = token_path();
    let _ = fs::remove_file(&path);
}

fn save_token(token: &str) -> Result<()> {
    let path = token_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string(&CachedToken {
        token: token.to_string(),
        valid_until: Some(now_epoch() + DEFAULT_TTL_SECS),
    })?;
    fs::write(&path, &data)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

// ── Device flow types ──────────────────────────────────────────────

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

// ── Public API ─────────────────────────────────────────────────────

/// Resolve a dkod session token.
///
/// 1. `DKOD_AUTH_TOKEN` env var (local dev mode)
/// 2. Cached token at `~/.config/dkod/token.json`
/// 3. OAuth device flow (opens browser, polls for approval)
pub async fn resolve_token(api_base: &str, env_token: Option<&str>) -> Result<String> {
    // 1. Env var (local dev)
    if let Some(token) = env_token {
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }

    // 2. Cached token
    if let Some(token) = read_cached_token() {
        return Ok(token);
    }

    // 3. Device flow
    run_device_flow(api_base).await
}

async fn run_device_flow(api_base: &str) -> Result<String> {
    let client = reqwest::Client::new();

    // Start
    let start: StartResponse = client
        .post(format!("{api_base}/api/auth/device/start"))
        .send()
        .await?
        .json()
        .await
        .context("failed to start device flow")?;

    // Print instructions to stderr (MCP uses stdout for protocol)
    eprintln!();
    eprintln!("  To authenticate, open this URL in your browser:");
    eprintln!();
    eprintln!("    {}", start.verification_url);
    eprintln!();
    eprintln!("  Your code: {}", start.user_code);
    eprintln!();

    // Try to open browser automatically
    let _ = open::that(&start.verification_url);

    // Poll until complete or timeout
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
            match resp.token {
                Some(token) => {
                    save_token(&token)?;
                    eprintln!("  Authenticated successfully!");
                    eprintln!();
                    return Ok(token);
                }
                None => {
                    anyhow::bail!(
                        "device flow: server returned 'complete' but no token was provided"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_path_is_under_config_dir() {
        let path = token_path();
        assert!(path.ends_with("dkod/token.json"));
    }

    #[tokio::test]
    async fn env_token_takes_priority() {
        // resolve_token should return the env token without hitting the network.
        let result = resolve_token("http://localhost:9999", Some("env-token")).await;
        assert_eq!(result.unwrap(), "env-token");
    }

    #[tokio::test]
    async fn empty_env_token_is_skipped() {
        // Empty env token should be treated as unset — resolve_token will
        // either return a cached token from disk or fail on device flow.
        // Both outcomes prove the empty env var path was skipped.
        let result = resolve_token("http://localhost:9999", Some("")).await;
        match result {
            Ok(t) => assert!(!t.is_empty(), "cached token should not be empty"),
            Err(_) => {} // no cached token, device flow unreachable — expected
        }
    }
}

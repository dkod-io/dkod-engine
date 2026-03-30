use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ── Session persistence ───────────────────────────────────────────

/// Path to the session cache file.
fn sessions_file() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dkod");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("sessions.json")
}

/// Save sessions to disk. Best-effort — errors are logged but don't propagate.
///
/// Uses atomic write (write to tmp file, then rename) to prevent corruption
/// if the process is killed mid-write. `rename` is atomic on POSIX when src
/// and dst are on the same filesystem, which is always the case here.
pub fn save_sessions(sessions: &HashMap<String, SessionData>) {
    let path = sessions_file();
    match serde_json::to_string(sessions) {
        Ok(json) => {
            let tmp = path.with_extension("json.tmp");
            if let Err(e) = std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, &path)) {
                tracing::warn!(path = %path.display(), error = %e, "failed to save sessions");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to serialize sessions"),
    }
}

/// Load sessions from disk. Returns empty map on any error.
pub fn load_sessions() -> HashMap<String, SessionData> {
    let path = sessions_file();
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Walk up from `start` looking for the workspace root (a `Cargo.toml` containing `[workspace]`).
/// Falls back to the first `Cargo.toml` found if none contain `[workspace]`.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    let mut first_cargo = None;
    loop {
        let manifest = current.join("Cargo.toml");
        if manifest.exists() {
            if first_cargo.is_none() {
                first_cargo = Some(current.clone());
            }
            if let Ok(content) = std::fs::read_to_string(&manifest) {
                if content.contains("[workspace]") {
                    return Some(current);
                }
            }
        }
        if !current.pop() {
            return first_cargo;
        }
    }
}

/// Parse `.env` then `.env.local` in `dir`, returning only `DKOD_*` variables.
/// `.env.local` overrides `.env`.
///
/// Handles common dotenv patterns:
/// - Strips surrounding single/double quotes from values
/// - Strips trailing inline comments (unquoted ` #`)
/// - Skips blank lines and full-line comments
pub(crate) fn load_dotenv_vars(dir: &Path) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for filename in &[".env", ".env.local"] {
        let path = dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((key, raw_value)) = trimmed.split_once('=') {
                    let key = key.trim();
                    let value = sanitize_dotenv_value(raw_value.trim());
                    if key.starts_with("DKOD_") {
                        vars.insert(key.to_string(), value);
                    }
                }
            }
        }
    }
    vars
}

/// Sanitize a dotenv value by handling quotes and stripping trailing inline comments.
///
/// For quoted values (`"..."` or `'...'`), the content between quotes is returned as-is
/// (inline comments after the closing quote are ignored, but content inside is preserved).
/// For unquoted values, trailing inline comments (` #`) are stripped.
fn sanitize_dotenv_value(raw: &str) -> String {
    let trimmed = raw.trim();

    // Handle quoted values: strip surrounding quotes and return inner content as-is.
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let quote_char = if first == b'"' || first == b'\'' {
            Some(first as char)
        } else {
            None
        };
        if let Some(q) = quote_char {
            // Find the matching closing quote (not the last char, but the first matching quote after pos 0).
            if let Some(end) = trimmed[1..].find(q) {
                return trimmed[1..1 + end].to_string();
            }
            // No closing quote found — strip the leading quote to avoid a stray
            // quote character in the returned value (e.g. a typo in .env.local).
            tracing::warn!(
                value = raw,
                "dotenv value has opening quote but no closing quote — stripping leading quote"
            );
            let without_quote = &trimmed[1..];
            return if let Some(pos) = without_quote.find(" #") {
                without_quote[..pos].trim_end().to_string()
            } else if let Some(pos) = without_quote.find("\t#") {
                without_quote[..pos].trim_end().to_string()
            } else {
                without_quote.to_string()
            };
        }
    }

    // Unquoted value: strip trailing inline comment (` #` or `\t#`).
    if let Some(pos) = trimmed.find(" #") {
        trimmed[..pos].trim_end().to_string()
    } else if let Some(pos) = trimmed.find("\t#") {
        trimmed[..pos].trim_end().to_string()
    } else {
        trimmed.to_string()
    }
}

/// TCP connect to localhost on the given port, trying both IPv6 (`[::1]`) and IPv4
/// (`127.0.0.1`) concurrently. Returns the HTTP base URL of the first address that
/// responds, short-circuiting without waiting for the slower probe to time out.
/// This ensures the probe works on hosts with IPv6 disabled and that the returned
/// address exactly matches the interface that was actually reachable.
///
/// IPv6 is preferred when both stacks respond simultaneously (`biased` select),
/// matching `dk-server`'s default binding to `[::1]`.
pub async fn probe_local_server(port: u16, timeout: Duration) -> Option<String> {
    let v6 = std::net::SocketAddr::V6(std::net::SocketAddrV6::new(
        std::net::Ipv6Addr::LOCALHOST,
        port,
        0,
        0,
    ));
    let v4 = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
        std::net::Ipv4Addr::LOCALHOST,
        port,
    ));

    // Race both probes with a shared timeout. `select!` returns as soon as one
    // branch matches, cancelling the other — so a successful connect on one stack
    // immediately returns without waiting for the other to time out.
    // Returns the exact address that succeeded, avoiding mismatches between
    // what was probed and what is used for the gRPC connection.
    let result = tokio::time::timeout(timeout, async move {
        tokio::select! {
            biased;
            Ok(_) = tokio::net::TcpStream::connect(v6) => Some(format!("http://[::1]:{port}")),
            Ok(_) = tokio::net::TcpStream::connect(v4) => Some(format!("http://127.0.0.1:{port}")),
            else => None,
        }
    })
    .await;

    result.unwrap_or_default() // timeout → None
}

/// Per-session data created after a successful CONNECT handshake.
///
/// Each `dk_connect` call creates a new `SessionData` entry in the session map,
/// enabling multiple concurrent agent sessions within the same dk-mcp process.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionData {
    /// Session ID returned by CONNECT.
    pub session_id: String,
    /// Workspace ID returned by CONNECT.
    pub workspace_id: String,
    /// Changeset ID returned by CONNECT.
    pub changeset_id: String,
    /// Repository / codebase name used in the CONNECT call.
    pub repo_name: String,
}

/// Connection-level state for the dk-mcp server.
///
/// Holds the gRPC connection parameters shared across all sessions.
/// Per-session state (session_id, workspace_id, changeset_id, repo_name)
/// is stored in a separate `SessionData` map.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// gRPC endpoint for the dkod server.
    /// Defaults to the value of `DKOD_GRPC_ADDR` env var, or `"http://[::1]:50051"`.
    pub server_addr: String,

    /// Auth token from env var. `None` if not set (will use device flow).
    pub auth_token: Option<String>,
}

impl SessionState {
    /// Create a new `SessionState` from environment variables with sensible defaults.
    pub fn from_env() -> Self {
        Self {
            server_addr: std::env::var("DKOD_GRPC_ADDR")
                .unwrap_or_else(|_| "https://agent.dkod.io:443".to_string()),
            auth_token: std::env::var("DKOD_AUTH_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    /// Create a new `SessionState` by resolving configuration from dotenv files,
    /// environment variables, and local server probing.
    ///
    /// Resolution order:
    /// - Auth token: env var > dotenv > None (device flow)
    /// - Server address: explicit `DKOD_GRPC_ADDR` (env or dotenv) > local probe (port 50051) > production
    pub async fn resolve() -> Self {
        // Try to find repo root and load dotenv vars.
        let cwd = std::env::current_dir().unwrap_or_default();
        let dotenv_vars = find_repo_root(&cwd)
            .map(|root| load_dotenv_vars(&root))
            .unwrap_or_default();

        // Resolve auth token: env var > dotenv > None
        let auth_token = std::env::var("DKOD_AUTH_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                dotenv_vars
                    .get("DKOD_AUTH_TOKEN")
                    .filter(|s| !s.is_empty())
                    .cloned()
            });

        // Resolve server address: explicit env/dotenv > local probe > production
        let explicit_addr = std::env::var("DKOD_GRPC_ADDR")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                dotenv_vars
                    .get("DKOD_GRPC_ADDR")
                    .filter(|s| !s.is_empty())
                    .cloned()
            });

        let server_addr = if let Some(addr) = explicit_addr {
            tracing::info!(endpoint = %addr, source = "explicit", "resolved server address");
            addr
        } else if let Some(addr) = probe_local_server(50051, Duration::from_millis(200)).await {
            tracing::info!(endpoint = %addr, source = "local_probe", "resolved server address");
            addr
        } else {
            let addr = "https://agent.dkod.io:443".to_string();
            tracing::info!(endpoint = %addr, source = "production_default", "resolved server address");
            addr
        };

        Self {
            server_addr,
            auth_token,
        }
    }

    /// Create a SessionState for HTTP MCP transport.
    ///
    /// The `auth_token` is a short-lived HS256 JWT minted by the platform
    /// from `AUTH_TOKEN`, used for server-to-server gRPC authentication
    /// via the `SessionTokenInterceptor`. It is NOT the raw secret or the
    /// per-user Clerk JWT — user authentication is handled by the HTTP auth
    /// middleware before requests reach the MCP layer.
    ///
    /// The `server_addr` is the internal gRPC endpoint (always loopback
    /// since the MCP HTTP handler runs in the same process as dk-server).
    pub fn from_http(server_addr: String, auth_token: String) -> Self {
        Self {
            server_addr,
            auth_token: Some(auth_token),
        }
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "server_addr:  {}", self.server_addr)?;
        match &self.auth_token {
            Some(t) => writeln!(f, "auth_token:   [{}chars]", t.len())?,
            None => writeln!(f, "auth_token:   [device flow]")?,
        }
        Ok(())
    }
}

impl std::fmt::Display for SessionData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "session_id:   {}", self.session_id)?;
        writeln!(f, "workspace_id: {}", self.workspace_id)?;
        writeln!(f, "changeset_id: {}", self.changeset_id)?;
        writeln!(f, "repo_name:    {}", self.repo_name)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;
    use std::sync::Mutex;

    /// Global mutex to serialize tests that manipulate environment variables.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// RAII guard that removes a directory on drop (even if a test panics).
    struct CleanupDir(std::path::PathBuf);
    impl Drop for CleanupDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Helper: save current values of our env vars, remove them, run the
    /// closure, then restore the originals. Panic-safe: always restores
    /// env vars even if the closure panics (needed for `#[should_panic]` tests).
    fn with_clean_env<F: FnOnce() + panic::UnwindSafe>(f: F) {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_addr = std::env::var("DKOD_GRPC_ADDR").ok();
        let saved_token = std::env::var("DKOD_AUTH_TOKEN").ok();

        std::env::remove_var("DKOD_GRPC_ADDR");
        std::env::remove_var("DKOD_AUTH_TOKEN");

        let result = panic::catch_unwind(f);

        // Restore originals (even on panic)
        match saved_addr {
            Some(v) => std::env::set_var("DKOD_GRPC_ADDR", v),
            None => std::env::remove_var("DKOD_GRPC_ADDR"),
        }
        match saved_token {
            Some(v) => std::env::set_var("DKOD_AUTH_TOKEN", v),
            None => std::env::remove_var("DKOD_AUTH_TOKEN"),
        }

        if let Err(e) = result {
            panic::resume_unwind(e);
        }
    }

    #[test]
    fn test_session_state_defaults() {
        with_clean_env(|| {
            let state = SessionState::from_env();
            assert_eq!(state.server_addr, "https://agent.dkod.io:443");
            assert!(state.auth_token.is_none());
        });
    }

    #[test]
    fn test_session_state_env_override() {
        with_clean_env(|| {
            std::env::set_var("DKOD_GRPC_ADDR", "http://custom:9999");
            std::env::set_var("DKOD_AUTH_TOKEN", "my-secret-token");

            let state = SessionState::from_env();
            assert_eq!(state.server_addr, "http://custom:9999");
            assert_eq!(state.auth_token, Some("my-secret-token".to_string()));
        });
    }

    #[test]
    fn test_session_data_display() {
        let data = SessionData {
            session_id: "sid-abc".into(),
            workspace_id: "wid-xyz".into(),
            changeset_id: "cid-123".into(),
            repo_name: "dkod".into(),
        };
        let output = data.to_string();
        assert!(
            output.contains("session_id:   sid-abc"),
            "expected session_id in:\n{output}"
        );
        assert!(
            output.contains("workspace_id: wid-xyz"),
            "expected workspace_id in:\n{output}"
        );
        assert!(
            output.contains("changeset_id: cid-123"),
            "expected changeset_id in:\n{output}"
        );
        assert!(
            output.contains("repo_name:    dkod"),
            "expected repo_name in:\n{output}"
        );
    }

    #[test]
    fn test_display_connection_state() {
        with_clean_env(|| {
            std::env::set_var("DKOD_AUTH_TOKEN", "test-token");
            let state = SessionState::from_env();
            let output = state.to_string();

            assert!(
                output.contains("server_addr:"),
                "expected 'server_addr:' in:\n{output}"
            );
            assert!(
                output.contains("auth_token:"),
                "expected 'auth_token:' in:\n{output}"
            );
        });
    }

    #[test]
    fn test_missing_auth_token_is_none() {
        with_clean_env(|| {
            let state = SessionState::from_env();
            assert!(state.auth_token.is_none());
        });
    }

    #[test]
    fn test_empty_auth_token_is_none() {
        with_clean_env(|| {
            std::env::set_var("DKOD_AUTH_TOKEN", "");
            let state = SessionState::from_env();
            assert!(state.auth_token.is_none());
        });
    }

    #[test]
    fn test_load_dotenv_parses_key_value() {
        let dir = std::env::temp_dir().join("dk-mcp-test-dotenv");
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = CleanupDir(dir.clone());
        std::fs::write(
            dir.join(".env.local"),
            "DKOD_AUTH_TOKEN=from-dotenv\nIGNORED_VAR=foo\n",
        )
        .unwrap();

        let vars = load_dotenv_vars(&dir);
        assert_eq!(vars.get("DKOD_AUTH_TOKEN").unwrap(), "from-dotenv");
        assert!(!vars.contains_key("IGNORED_VAR"));
    }

    #[test]
    fn test_load_dotenv_skips_comments_and_blanks() {
        let dir = std::env::temp_dir().join("dk-mcp-test-dotenv-comments");
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = CleanupDir(dir.clone());
        std::fs::write(
            dir.join(".env"),
            "# comment\n\nDKOD_AUTH_TOKEN=val\n  # another comment\n",
        )
        .unwrap();

        let vars = load_dotenv_vars(&dir);
        assert_eq!(vars.get("DKOD_AUTH_TOKEN").unwrap(), "val");
    }

    #[tokio::test]
    async fn test_probe_local_server_unreachable() {
        // Bind on both IPv4 and IPv6 to get a guaranteed-free ephemeral port,
        // then drop both listeners so probe_local_server finds nothing on either stack.
        let v4 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = v4.local_addr().unwrap().port();
        // Best-effort IPv6 bind on same port (may fail if IPv6 is disabled)
        let _v6 = tokio::net::TcpListener::bind(format!("[::1]:{port}"))
            .await
            .ok();
        drop(v4);
        drop(_v6);
        let result = probe_local_server(port, std::time::Duration::from_millis(50)).await;
        assert!(result.is_none());
    }

    #[test]
    fn test_sanitize_strips_double_quotes() {
        assert_eq!(sanitize_dotenv_value(r#""my-token""#), "my-token");
    }

    #[test]
    fn test_sanitize_strips_single_quotes() {
        assert_eq!(sanitize_dotenv_value("'my-token'"), "my-token");
    }

    #[test]
    fn test_sanitize_strips_inline_comment() {
        assert_eq!(
            sanitize_dotenv_value("my-token # this is a comment"),
            "my-token"
        );
    }

    #[test]
    fn test_sanitize_preserves_value_without_comment() {
        assert_eq!(sanitize_dotenv_value("my-token"), "my-token");
    }

    #[test]
    fn test_sanitize_preserves_hash_without_leading_space() {
        // A bare # without a leading space should NOT be treated as a comment
        assert_eq!(sanitize_dotenv_value("color#fff"), "color#fff");
    }

    #[test]
    fn test_sanitize_unclosed_quote_strips_leading_quote() {
        // An unclosed quote should not leave a stray quote character in the value
        assert_eq!(sanitize_dotenv_value("\"my-token"), "my-token");
        assert_eq!(sanitize_dotenv_value("'my-token"), "my-token");
    }

    #[test]
    fn test_load_dotenv_strips_quotes_and_comments() {
        let dir = std::env::temp_dir().join("dk-mcp-test-dotenv-sanitize");
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = CleanupDir(dir.clone());
        std::fs::write(
            dir.join(".env.local"),
            "DKOD_AUTH_TOKEN=\"quoted-token\" # inline comment\nDKOD_GRPC_ADDR='http://localhost:50051'\n",
        )
        .unwrap();

        let vars = load_dotenv_vars(&dir);
        assert_eq!(vars.get("DKOD_AUTH_TOKEN").unwrap(), "quoted-token");
        assert_eq!(
            vars.get("DKOD_GRPC_ADDR").unwrap(),
            "http://localhost:50051"
        );
    }
}

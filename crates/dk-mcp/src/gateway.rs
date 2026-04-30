//! MCP gateway: aggregate tools from multiple upstream MCP servers and proxy
//! `call_tool` to the right one.
//!
//! At startup, [`Gateway::connect`] reads the embedded
//! [`Registry`](crate::registry::Registry) and, for every entry whose
//! `auth_env_vars` are all set in the process environment AND whose launch
//! info (`command` for stdio, `url` for http) is populated, spawns an `rmcp`
//! client and discovers the upstream's tools.
//!
//! Tools are exposed under the prefix `{server_id}___{tool_name}` (triple
//! underscore — same convention as `gh-aw-mcpg`). The triple underscore is
//! reserved: native dkod tools (`dk_*`) never contain it, so a single split
//! cleanly recovers `(server, tool)` for routing.
//!
//! ## Failure isolation
//!
//! A failure to connect or list-tools on one upstream NEVER fails the gateway
//! as a whole. The error is logged with `tracing` and the offending entry is
//! omitted from the aggregated tool list. Subsequent calls referring to a
//! missing upstream return a structured `tool_error` instead of crashing.
//!
//! ## Large-payload offload
//!
//! `call_tool` results larger than [`PAYLOAD_OFFLOAD_THRESHOLD`] bytes are
//! written to a file under `dirs::cache_dir().join("dkod/mcp-payloads")` and
//! the call result is rewritten to a small JSON pointer of the form
//! `{ "_offloaded": true, "path": "...", "size": N }`. This keeps the agent's
//! context window from blowing up on bulk SQL results, log dumps, etc.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, CallToolResult, Content, Tool};
use rmcp::service::{Peer, RoleClient, RunningService, ServiceExt};
use rmcp::transport::common::client_side_sse::ExponentialBackoff;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use serde_json::Value as JsonValue;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::registry::{McpEntry, Registry};

/// Above this size (in bytes) a tool result is offloaded to disk.
pub const PAYLOAD_OFFLOAD_THRESHOLD: usize = 10 * 1024;

/// Separator between `<server>` and `<tool>` in aggregated tool names.
pub const TOOL_NAME_SEPARATOR: &str = "___";

/// One connected upstream MCP server.
#[allow(dead_code)] // _service is held purely to keep the running task alive
pub struct UpstreamClient {
    /// Canonical server ID from the registry.
    pub server_id: String,
    /// Cached list of tools advertised by the upstream (unprefixed).
    pub tools: Vec<Tool>,
    /// Clonable peer used to send `call_tool` and `list_tools` requests.
    pub peer: Peer<RoleClient>,
    /// Running service handle. Dropping this cancels the connection.
    _service: Arc<RunningService<RoleClient, ()>>,
}

/// Aggregated state of every connected upstream.
#[derive(Default)]
pub struct GatewayState {
    pub clients: HashMap<String, UpstreamClient>,
}

/// Gateway facade — clone-able handle to the shared state behind an `RwLock`.
#[derive(Clone, Default)]
pub struct Gateway {
    state: Arc<RwLock<GatewayState>>,
}

impl Gateway {
    /// Build an empty gateway. Call [`Gateway::connect_all`] to populate it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read [`Registry::load_embedded`] and try to connect to every entry that
    /// is both *configured* (all `auth_env_vars` present) and *launchable*
    /// (has the transport-specific launch fields). Failures are logged and
    /// quarantined per-server.
    ///
    /// Returns the populated gateway. Always succeeds — connect failures are
    /// reported via `tracing::warn!`, not surfaced as `Err`.
    pub async fn connect_all(self) -> Self {
        let registry = Registry::load_embedded();
        for entry in registry.mcp_servers() {
            if !Registry::is_mcp_configured(entry) {
                debug!(
                    upstream = entry.name,
                    "skipping unconfigured MCP (auth_env_vars not all set)"
                );
                continue;
            }
            match self.connect_one(entry).await {
                Ok(()) => info!(upstream = entry.name, "mounted MCP upstream"),
                Err(err) => warn!(upstream = entry.name, %err, "failed to mount MCP upstream"),
            }
        }
        let mounted = self.state.read().await.clients.len();
        info!(mounted, "MCP gateway ready");
        self
    }

    /// Connect to one upstream and store it. Errors are returned to the caller
    /// so the loop in [`Self::connect_all`] can log them per-server.
    async fn connect_one(&self, entry: &McpEntry) -> anyhow::Result<()> {
        let service = match entry.transport.as_str() {
            "stdio" => connect_stdio(entry).await?,
            "http" => connect_http(entry).await?,
            other => anyhow::bail!("unknown transport `{other}` for `{}`", entry.name),
        };
        let peer = service.peer().clone();
        let tools = peer.list_all_tools().await?;
        debug!(
            upstream = entry.name,
            tool_count = tools.len(),
            "discovered upstream tools"
        );
        let client = UpstreamClient {
            server_id: entry.name.clone(),
            tools,
            peer,
            _service: Arc::new(service),
        };
        self.state
            .write()
            .await
            .clients
            .insert(entry.name.clone(), client);
        Ok(())
    }

    /// Return the union of all upstream tools, prefixed with their server ID.
    pub async fn aggregated_tools(&self) -> Vec<JsonValue> {
        let state = self.state.read().await;
        let mut out = Vec::new();
        for (server_id, client) in &state.clients {
            for tool in &client.tools {
                out.push(serde_json::json!({
                    "name": format!("{server_id}{TOOL_NAME_SEPARATOR}{}", tool.name),
                    "server": server_id,
                    "upstream_name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                }));
            }
        }
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        out
    }

    /// Names of all currently mounted upstreams.
    pub async fn mounted_servers(&self) -> Vec<String> {
        let state = self.state.read().await;
        let mut names: Vec<_> = state.clients.keys().cloned().collect();
        names.sort();
        names
    }

    /// Dispatch a `call_tool` request. `tool_name` may be either prefixed
    /// (`server___tool`) or unprefixed (in which case `server_hint` MUST be
    /// supplied). Large results (>[`PAYLOAD_OFFLOAD_THRESHOLD`] bytes
    /// serialized) are written to disk and replaced with a pointer.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        server_hint: Option<&str>,
        arguments: Option<serde_json::Map<String, JsonValue>>,
    ) -> anyhow::Result<CallToolResult> {
        let (server_id, upstream_tool) = resolve_target(tool_name, server_hint)?;
        let peer = {
            let state = self.state.read().await;
            state
                .clients
                .get(&server_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "upstream `{server_id}` is not mounted (configured? \
                         missing auth_env_vars? launch info?)"
                    )
                })?
                .peer
                .clone()
        };
        let result = peer
            .call_tool(CallToolRequestParams {
                meta: None,
                name: upstream_tool.into(),
                arguments,
                task: None,
            })
            .await?;
        Ok(maybe_offload(&server_id, result))
    }
}

/// Split a (possibly prefixed) tool name into `(server, upstream_tool)`.
fn resolve_target(tool_name: &str, server_hint: Option<&str>) -> anyhow::Result<(String, String)> {
    if let Some((server, tool)) = tool_name.split_once(TOOL_NAME_SEPARATOR) {
        if let Some(hint) = server_hint {
            anyhow::ensure!(
                hint == server,
                "tool name `{tool_name}` is prefixed with `{server}` but \
                 server_hint=`{hint}` was passed"
            );
        }
        Ok((server.to_string(), tool.to_string()))
    } else {
        let hint = server_hint.ok_or_else(|| {
            anyhow::anyhow!(
                "unprefixed tool `{tool_name}` requires a server hint \
                 (use `{{server}}{TOOL_NAME_SEPARATOR}{{tool}}` or pass `server`)"
            )
        })?;
        Ok((hint.to_string(), tool_name.to_string()))
    }
}

/// Offload large results to disk and replace them with a small JSON pointer.
///
/// `server_id` is used to namespace the on-disk file (e.g. `context7-<uuid>.json`,
/// `local-sql-<uuid>.json`). Exposed as `pub(crate)` so other tools (notably
/// `dk_run_local_sql`) can reuse the same offload semantics.
pub(crate) fn maybe_offload(server_id: &str, result: CallToolResult) -> CallToolResult {
    let serialized = match serde_json::to_string(&result) {
        Ok(s) => s,
        Err(_) => return result,
    };
    if serialized.len() <= PAYLOAD_OFFLOAD_THRESHOLD {
        return result;
    }
    let dir = match payload_dir() {
        Some(d) => d,
        None => return result,
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        warn!(
            ?err,
            "failed to create payload offload dir; returning inline"
        );
        return result;
    }
    let id = uuid::Uuid::new_v4();
    let path = dir.join(format!("{server_id}-{id}.json"));
    if let Err(err) = std::fs::write(&path, &serialized) {
        warn!(?err, "failed to write offload payload; returning inline");
        return result;
    }
    let pointer = serde_json::json!({
        "_offloaded": true,
        "path": path.to_string_lossy(),
        "size": serialized.len(),
        "server": server_id,
    });
    let text = serde_json::to_string(&pointer).unwrap_or_else(|_| "{}".into());
    CallToolResult::success(vec![Content::text(text)])
}

fn payload_dir() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("dkod").join("mcp-payloads"))
}

async fn connect_stdio(entry: &McpEntry) -> anyhow::Result<RunningService<RoleClient, ()>> {
    let cmd_str = entry
        .command
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("stdio entry `{}` is missing `command`", entry.name))?;
    let args: Vec<String> = entry.args.iter().map(|a| resolve_placeholders(a)).collect();
    let envs: Vec<(String, String)> = entry
        .env
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), resolve_placeholders_strict(v)?)))
        .collect();

    let owned_cmd = cmd_str.to_string();
    let proc = TokioChildProcess::new(Command::new(&owned_cmd).configure(move |c| {
        c.args(&args).envs(envs);
    }))?;
    let service = ().serve(proc).await?;
    Ok(service)
}

async fn connect_http(entry: &McpEntry) -> anyhow::Result<RunningService<RoleClient, ()>> {
    let url = entry
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("http entry `{}` is missing `url`", entry.name))?;
    let url = resolve_placeholders(url);

    let mut custom_headers = http::HeaderMap::new();
    let mut auth_header_value: Option<String> = None;
    for (k, v) in &entry.headers {
        let Some(resolved) = resolve_placeholders_strict(v) else {
            warn!(
                upstream = entry.name,
                header = k,
                "dropping header with unresolved env var"
            );
            continue;
        };
        if k.eq_ignore_ascii_case("authorization") {
            auth_header_value = Some(strip_bearer(&resolved));
        } else {
            let name = http::HeaderName::try_from(k.as_str())
                .map_err(|e| anyhow::anyhow!("invalid header name `{k}`: {e}"))?;
            let value = http::HeaderValue::try_from(resolved.as_str())
                .map_err(|e| anyhow::anyhow!("invalid header value for `{k}`: {e}"))?;
            custom_headers.insert(name, value);
        }
    }

    let mut config = StreamableHttpClientTransportConfig {
        uri: url.into(),
        retry_config: Arc::new(ExponentialBackoff::default()),
        channel_buffer_capacity: 16,
        allow_stateless: false,
        auth_header: auth_header_value,
        custom_headers: custom_headers
            .into_iter()
            .filter_map(|(name, value)| name.map(|n| (n, value)))
            .collect(),
    };
    // The default URI sentinel is `localhost`; ensure we set the real one.
    config.uri = entry.url.clone().unwrap().into();

    let transport = StreamableHttpClientTransport::from_config(config);
    let service = ().serve(transport).await?;
    Ok(service)
}

/// Replace `${VAR}` occurrences with the value of the env var, leaving
/// missing vars as the literal placeholder. Used for non-credential strings
/// where a bare `${VAR}` is OK (e.g. URLs that don't actually need substitution).
fn resolve_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('}') else {
            out.push_str(rest);
            return out;
        };
        let var_name = &rest[2..end];
        match std::env::var(var_name) {
            Ok(val) => out.push_str(&val),
            Err(_) => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Strict variant: returns `None` if any `${VAR}` placeholder is unset.
/// Used for credential values where an unresolved placeholder must NOT leak.
fn resolve_placeholders_strict(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find('}')?;
        let var_name = &rest[2..end];
        out.push_str(&std::env::var(var_name).ok()?);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn strip_bearer(s: &str) -> String {
    s.strip_prefix("Bearer ")
        .or_else(|| s.strip_prefix("bearer "))
        .unwrap_or(s)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_target_prefixed_ok() {
        let (s, t) = resolve_target("supabase___query", None).unwrap();
        assert_eq!(s, "supabase");
        assert_eq!(t, "query");
    }

    #[test]
    fn resolve_target_unprefixed_with_hint() {
        let (s, t) = resolve_target("query", Some("supabase")).unwrap();
        assert_eq!(s, "supabase");
        assert_eq!(t, "query");
    }

    #[test]
    fn resolve_target_unprefixed_no_hint_errors() {
        assert!(resolve_target("query", None).is_err());
    }

    #[test]
    fn resolve_target_hint_mismatch_errors() {
        assert!(resolve_target("supabase___query", Some("redis")).is_err());
    }

    #[test]
    fn resolve_placeholders_substitutes_set_vars() {
        // SAFETY: tests in this module run sequentially with a unique var name.
        unsafe { std::env::set_var("DK_MCP_TEST_VAR_A", "hello") };
        assert_eq!(resolve_placeholders("x=${DK_MCP_TEST_VAR_A}!"), "x=hello!");
    }

    #[test]
    fn resolve_placeholders_keeps_unset_literal() {
        unsafe { std::env::remove_var("DK_MCP_TEST_VAR_B") };
        assert_eq!(
            resolve_placeholders("x=${DK_MCP_TEST_VAR_B}!"),
            "x=${DK_MCP_TEST_VAR_B}!"
        );
    }

    #[test]
    fn resolve_placeholders_strict_returns_none_on_unset() {
        unsafe { std::env::remove_var("DK_MCP_TEST_VAR_C") };
        assert!(resolve_placeholders_strict("x=${DK_MCP_TEST_VAR_C}!").is_none());
    }

    #[test]
    fn strip_bearer_handles_prefix() {
        assert_eq!(strip_bearer("Bearer abc"), "abc");
        assert_eq!(strip_bearer("bearer abc"), "abc");
        assert_eq!(strip_bearer("abc"), "abc");
    }
}

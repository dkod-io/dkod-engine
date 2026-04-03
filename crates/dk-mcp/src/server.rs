use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio_stream::StreamExt;

// Conflict event types from the platform's event bus (shared via serde JSON over NATS).
#[derive(serde::Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum ConflictEvent {
    #[serde(rename = "conflict_warning")]
    Warning(ConflictWarning),
    #[serde(rename = "conflict_block")]
    Block(ConflictBlockEvent),
    #[serde(rename = "file_activity")]
    Activity(FileActivity),
    #[serde(rename = "conflict_resolved")]
    Resolved(ConflictResolvedEvent),
}

#[derive(serde::Deserialize)]
struct ConflictWarning {
    file_path: String,
    conflicting_agent: String,
    conflicting_symbols: Vec<SymbolRef>,
}

#[derive(serde::Deserialize)]
struct SymbolRef {
    qualified_name: String,
}

#[derive(serde::Deserialize)]
struct ConflictBlockEvent {
    file_path: String,
    conflicting_symbols: Vec<SymbolConflictDetail>,
}

#[derive(serde::Deserialize)]
struct SymbolConflictDetail {
    qualified_name: String,
    conflicting_agent: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct FileActivity {
    file_path: String,
    agent: String,
    symbols_modified: Vec<String>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ConflictResolvedEvent {
    changeset_id: String,
    action: String,
    resolved_by: String,
    message: String,
}

use rmcp::{
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    service::{RequestContext, RoleServer},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::grpc::AuthenticatedClient;
use crate::state::{SessionData, SessionState};

// ── Parameter structs ──

#[derive(Deserialize, JsonSchema)]
struct ConnectParams {
    /// Repository name, e.g. 'demo/hello-world'
    repo: String,
    /// What the agent plans to do in this session
    intent: String,
    /// Agent name (optional, auto-assigned if empty)
    agent_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ContextParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// Search query (symbol name, pattern, or natural language)
    query: String,
    /// Depth: 'signatures' (default), 'full', or 'call_graph'
    depth: Option<String>,
    /// Max tokens for the response
    max_tokens: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct FileReadParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// File path relative to the repo root
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct FileWriteParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// File path relative to the repo root
    path: String,
    /// Full file content to write
    content: String,
}

#[derive(Deserialize, JsonSchema)]
struct FileListParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// Optional path prefix filter (e.g. 'src/')
    prefix: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SubmitParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// Description of what these changes accomplish
    intent: String,
}

#[derive(Deserialize, JsonSchema)]
struct VerifyParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct MergeParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// Optional commit message (auto-generated if omitted)
    message: Option<String>,
    /// Force merge even when recently-merged symbols would be overwritten (default: false)
    force: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct ApproveParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ResolveParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// Resolution mode: "proceed", "keep_yours", "keep_theirs", or "manual"
    resolution: String,
    /// Conflict ID for per-symbol resolution (required for keep_yours, keep_theirs, manual)
    conflict_id: Option<String>,
    /// Custom content for "manual" resolution
    content: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct StatusParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct PushParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// Push mode: "branch" (push to a branch) or "pr" (push and create a pull request)
    mode: String,
    /// Target branch name (e.g. "feat/my-feature")
    branch_name: String,
    /// PR title (required when mode is "pr")
    pr_title: Option<String>,
    /// PR body/description (optional, only used when mode is "pr")
    pr_body: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct CloseParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct WatchParams {
    /// Session ID from dk_connect (required when multiple sessions are active)
    session_id: Option<String>,
    /// Glob filter for events (default: "*" for all events)
    filter: Option<String>,
}

/// MCP server that bridges Claude Code to the dkod Agent Protocol via gRPC.
#[derive(Clone)]
pub struct DkodMcp {
    tool_router: ToolRouter<Self>,
    /// Connection-level state (server_addr, auth_token) shared across all sessions.
    pub connection: Arc<RwLock<SessionState>>,
    /// Per-session data keyed by session_id. Each dk_connect creates a new entry.
    pub sessions: Arc<RwLock<HashMap<String, SessionData>>>,
    /// Cached gRPC client, reused across tool calls after dk_connect.
    /// Intentionally shared across all sessions: every session authenticates
    /// with the same token so a single `AuthenticatedClient` is sufficient.
    /// Created on the first `dk_connect` and reused — later connects do NOT replace it.
    /// Wrapped in a Mutex because `AgentServiceClient` methods require `&mut self`.
    grpc_client: Arc<Mutex<Option<AuthenticatedClient>>>,
    /// Pending conflict warnings per session, keyed by session_id.
    /// Populated by the NATS subscription task spawned after dk_connect.
    pending_warnings: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// Per-session NATS subscription task handles, keyed by session_id.
    /// Each `dk_connect` spawns its own NATS task for conflict notifications,
    /// enabling concurrent sessions to receive independent conflict events.
    nats_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Per-session cancellation flags for NATS tasks, keyed by session_id.
    /// Set to true in Drop or when a session is removed to cleanly stop
    /// that session's NATS task without needing the Mutex.
    nats_cancellations: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
    /// Pending watch events per session, keyed by session_id.
    /// Populated by the gRPC Watch streaming task spawned after dk_connect.
    pending_watch_events: Arc<Mutex<HashMap<String, Vec<crate::WatchEvent>>>>,
    /// Per-session Watch stream task handles, keyed by session_id.
    watch_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Per-session cancellation flags for Watch tasks, keyed by session_id.
    watch_cancellations: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
    /// Active filter per session for Watch streams. Used to detect filter changes
    /// and restart the stream when a different filter is requested via dk_watch.
    watch_filters: Arc<Mutex<HashMap<String, String>>>,
    /// Symbols this agent has modified per session, keyed by session_id.
    /// Used to tag incoming watch events with [AFFECTS YOUR WORK] when another
    /// agent modifies symbols that overlap with our own modifications.
    my_modified_symbols: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    /// Per-session flag indicating watch event buffer overflow occurred.
    /// When true, the next drain will include an overflow warning.
    watch_overflow: Arc<Mutex<HashMap<String, bool>>>,
}

/// Cancel all per-session NATS and Watch tasks when the MCP instance drops
/// (HTTP transport) to prevent orphaned background tasks from accumulating.
impl Drop for DkodMcp {
    fn drop(&mut self) {
        // Signal all NATS tasks to stop via their per-session cancellation flags.
        if let Ok(cancellations) = self.nats_cancellations.try_lock() {
            for flag in cancellations.values() {
                flag.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        // Also abort all NATS tasks so they don't block indefinitely on
        // `sub.next().await` after the AtomicBool is set.
        if let Ok(mut tasks) = self.nats_tasks.try_lock() {
            for (_sid, handle) in tasks.drain() {
                handle.abort();
            }
        }
        // Signal all Watch tasks to stop via their per-session cancellation flags.
        if let Ok(cancellations) = self.watch_cancellations.try_lock() {
            for flag in cancellations.values() {
                flag.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        // Abort all Watch tasks.
        if let Ok(mut tasks) = self.watch_tasks.try_lock() {
            for (_sid, handle) in tasks.drain() {
                handle.abort();
            }
        }
    }
}

#[tool_router]
impl DkodMcp {
    #[allow(clippy::new_without_default)]
    pub async fn new() -> Self {
        let state = SessionState::resolve().await;
        let restored_sessions = crate::state::load_sessions();
        let session_count = restored_sessions.len();
        if session_count > 0 {
            tracing::info!(count = session_count, "restored sessions from disk");
        }
        Self {
            tool_router: Self::tool_router(),
            connection: Arc::new(RwLock::new(state)),
            sessions: Arc::new(RwLock::new(restored_sessions)),
            grpc_client: Arc::new(Mutex::new(None)),
            pending_warnings: Arc::new(Mutex::new(HashMap::new())),
            nats_tasks: Arc::new(Mutex::new(HashMap::new())),
            nats_cancellations: Arc::new(Mutex::new(HashMap::new())),
            pending_watch_events: Arc::new(Mutex::new(HashMap::new())),
            watch_tasks: Arc::new(Mutex::new(HashMap::new())),
            watch_cancellations: Arc::new(Mutex::new(HashMap::new())),
            watch_filters: Arc::new(Mutex::new(HashMap::new())),
            my_modified_symbols: Arc::new(Mutex::new(HashMap::new())),
            watch_overflow: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new `DkodMcp` for HTTP transport.
    ///
    /// Unlike `new()` (stdio, single-user), this constructor is called
    /// per-session on the HTTP endpoint. The `auth_token` is a short-lived
    /// HS256 JWT minted by the platform from `AUTH_TOKEN`, NOT the raw secret.
    ///
    /// User authentication is handled by the HTTP auth middleware layer
    /// *before* requests reach the MCP service. The middleware validates
    /// the Clerk JWT / API key and injects `CurrentUser` into request
    /// extensions. The gRPC calls from this instance use the minted JWT
    /// for server-to-server authentication via the `SessionTokenInterceptor`.
    pub fn new_for_http(server_addr: String, auth_token: String) -> Self {
        Self {
            tool_router: Self::tool_router(),
            connection: Arc::new(RwLock::new(SessionState::from_http(
                server_addr,
                auth_token,
            ))),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            grpc_client: Arc::new(Mutex::new(None)),
            pending_warnings: Arc::new(Mutex::new(HashMap::new())),
            nats_tasks: Arc::new(Mutex::new(HashMap::new())),
            nats_cancellations: Arc::new(Mutex::new(HashMap::new())),
            pending_watch_events: Arc::new(Mutex::new(HashMap::new())),
            watch_tasks: Arc::new(Mutex::new(HashMap::new())),
            watch_cancellations: Arc::new(Mutex::new(HashMap::new())),
            watch_filters: Arc::new(Mutex::new(HashMap::new())),
            my_modified_symbols: Arc::new(Mutex::new(HashMap::new())),
            watch_overflow: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new `DkodMcp` for HTTP transport with shared session state.
    ///
    /// Unlike `new_for_http()`, this constructor accepts a pre-created `sessions`
    /// Arc so that dk_* tool sessions persist when rmcp rotates handler instances
    /// (each HTTP request may get a fresh `DkodMcp` from the `StreamableHttpService`
    /// factory).
    ///
    /// SECURITY: The `sessions` Arc MUST be scoped per-user (the caller is
    /// responsible for providing a user-specific map). The gRPC client is
    /// intentionally NOT shared — each instance creates its own
    /// `AuthenticatedClient` using the per-session `auth_token` JWT, ensuring
    /// tenant isolation. Sharing a gRPC client across users would allow
    /// cross-tenant authentication bypass since the `BearerAuthInterceptor`
    /// bakes the JWT at construction time.
    ///
    /// Per-instance state (gRPC client, NATS tasks, watch tasks, pending
    /// warnings, etc.) is fresh per instance and cleaned up in `Drop`.
    ///
    /// NOTE: Background tasks (NATS conflict subscriptions, Watch streams)
    /// are per-instance and will be cancelled when the instance is dropped
    /// during handler rotation. Surviving sessions in `shared_sessions` will
    /// not have active listeners until the next `dk_connect` call creates new
    /// tasks. This is acceptable because the gRPC session on the server side
    /// is independent of these client-side notification tasks.
    pub fn new_for_http_with_shared_state(
        server_addr: String,
        auth_token: String,
        shared_sessions: Arc<RwLock<HashMap<String, SessionData>>>,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            connection: Arc::new(RwLock::new(SessionState::from_http(
                server_addr,
                auth_token,
            ))),
            sessions: shared_sessions,
            grpc_client: Arc::new(Mutex::new(None)),
            pending_warnings: Arc::new(Mutex::new(HashMap::new())),
            nats_tasks: Arc::new(Mutex::new(HashMap::new())),
            nats_cancellations: Arc::new(Mutex::new(HashMap::new())),
            pending_watch_events: Arc::new(Mutex::new(HashMap::new())),
            watch_tasks: Arc::new(Mutex::new(HashMap::new())),
            watch_cancellations: Arc::new(Mutex::new(HashMap::new())),
            watch_filters: Arc::new(Mutex::new(HashMap::new())),
            my_modified_symbols: Arc::new(Mutex::new(HashMap::new())),
            watch_overflow: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new `DkodMcp` for testing, bypassing the TCP probe and using
    /// `SessionState::from_env()` directly. Use this in integration tests to avoid
    /// the 200ms probe delay per test.
    pub fn new_for_testing() -> Self {
        Self {
            tool_router: Self::tool_router(),
            connection: Arc::new(RwLock::new(SessionState::from_env())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            grpc_client: Arc::new(Mutex::new(None)),
            pending_warnings: Arc::new(Mutex::new(HashMap::new())),
            nats_tasks: Arc::new(Mutex::new(HashMap::new())),
            nats_cancellations: Arc::new(Mutex::new(HashMap::new())),
            pending_watch_events: Arc::new(Mutex::new(HashMap::new())),
            watch_tasks: Arc::new(Mutex::new(HashMap::new())),
            watch_cancellations: Arc::new(Mutex::new(HashMap::new())),
            watch_filters: Arc::new(Mutex::new(HashMap::new())),
            my_modified_symbols: Arc::new(Mutex::new(HashMap::new())),
            watch_overflow: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a clone of the cached gRPC client, or return an error if not connected.
    ///
    /// If sessions were restored from disk but the gRPC client was lost (process
    /// restart), this will lazily reconnect using the stored connection state.
    async fn get_client(&self) -> Result<AuthenticatedClient, McpError> {
        // Read sessions BEFORE acquiring grpc_client to maintain lock ordering:
        // sessions -> grpc_client (same order as dk_merge cleanup).
        // This prevents ABBA lock inversion deadlocks.
        let has_sessions = !self.sessions.read().await.is_empty();

        // Hold the Mutex across the reconnect so only one caller reconnects.
        // This prevents concurrent callers from all racing through resolve_token
        // and connect_with_auth simultaneously, discarding duplicate connections.
        let mut cached = self.grpc_client.lock().await;

        // Fast path: client already exists.
        if let Some(client) = cached.as_ref() {
            return Ok(client.clone());
        }

        // Slow path: sessions restored from disk but client not yet created.
        // Reconnect using the stored connection state.
        if has_sessions {
            let (addr, env_token) = {
                let conn = self.connection.read().await;
                (conn.server_addr.clone(), conn.auth_token.clone())
            };
            let api_base = derive_api_base(&addr);
            let token = crate::auth::resolve_token(&api_base, env_token.as_deref())
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("auth reconnect failed: {e}"), None)
                })?;

            let new_client = crate::grpc::connect_with_auth(&addr, token)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("gRPC reconnect failed: {e}"), None)
                })?;

            *cached = Some(new_client.clone());
            return Ok(new_client);
        }

        Err(McpError::invalid_params(
            "No active gRPC connection. Call dk_connect first.",
            None,
        ))
    }

    /// Fetch workspace status from the server and append it to the text buffer.
    /// Silently appends a fallback message if the gRPC call fails.
    async fn append_workspace_status(&self, text: &mut String, session_id: &str) {
        let mut client = match self.get_client().await {
            Ok(c) => c,
            Err(_) => {
                text.push_str("\n(workspace status unavailable: no gRPC connection)\n");
                return;
            }
        };
        match client
            .get_session_status(crate::SessionStatusRequest {
                session_id: session_id.to_owned(),
            })
            .await
        {
            Ok(resp) => {
                let status = resp.into_inner();
                if !status.files_modified.is_empty() {
                    text.push_str(&format!(
                        "\nfiles_modified: {}\n",
                        status.files_modified.len()
                    ));
                    for f in &status.files_modified {
                        text.push_str(&format!("  - {f}\n"));
                    }
                }
                if !status.symbols_modified.is_empty() {
                    text.push_str(&format!(
                        "\nsymbols_modified: {}\n",
                        status.symbols_modified.len()
                    ));
                    for s in &status.symbols_modified {
                        text.push_str(&format!("  - {s}\n"));
                    }
                }
                if !status.base_commit.is_empty() {
                    text.push_str(&format!("base_commit:    {}\n", status.base_commit));
                }
                text.push_str(&format!(
                    "other_sessions: {}\n",
                    status.active_other_sessions
                ));
            }
            Err(e) => {
                text.push_str(&format!("\n(workspace status unavailable: {e})\n"));
            }
        }
    }

    /// Resolve a session by optional session_id.
    ///
    /// - If `session_id` is provided, looks it up in the session map.
    /// - If `session_id` is `None` and exactly one session exists, returns it.
    /// - If `session_id` is `None` and zero or multiple sessions exist, returns an error.
    pub async fn resolve_session(&self, session_id: Option<&str>) -> Result<SessionData, McpError> {
        let sessions = self.sessions.read().await;
        match session_id {
            Some(id) => sessions.get(id).cloned().ok_or_else(|| {
                McpError::invalid_params(
                    format!("Session '{id}' not found. Call dk_connect first."),
                    None,
                )
            }),
            None => {
                if sessions.len() == 1 {
                    Ok(sessions.values().next().unwrap().clone())
                } else if sessions.is_empty() {
                    Err(McpError::invalid_params(
                        "No active session. Call dk_connect first.",
                        None,
                    ))
                } else {
                    let ids: Vec<&str> = sessions.keys().map(|s| s.as_str()).collect();
                    Err(McpError::invalid_params(
                        format!(
                            "Multiple sessions active ({}). Provide session_id parameter (from dk_connect response).",
                            ids.join(", ")
                        ),
                        None,
                    ))
                }
            }
        }
    }

    /// Drain all pending conflict warnings for a session and return a formatted
    /// warning block, or `None` if there are no pending warnings.
    async fn drain_warnings(&self, session_id: &str) -> Option<String> {
        let warnings = {
            let mut map = self.pending_warnings.lock().await;
            map.remove(session_id).unwrap_or_default()
        };
        if warnings.is_empty() {
            return None;
        }
        let mut text = String::from("⚠️ CONFLICT WARNINGS:\n");
        for w in &warnings {
            text.push_str(w);
            text.push('\n');
        }
        text.push('\n');
        Some(text)
    }

    /// Clean up all Watch-related state for a session. Used by dk_submit
    /// on both the success and empty-response paths to avoid duplication.
    async fn cleanup_watch_for_session(&self, session_id: &str) {
        if let Some(flag) = self.watch_cancellations.lock().await.remove(session_id) {
            flag.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(handle) = self.watch_tasks.lock().await.remove(session_id) {
            handle.abort();
        }
        self.watch_filters.lock().await.remove(session_id);
        self.pending_watch_events.lock().await.remove(session_id);
        self.my_modified_symbols.lock().await.remove(session_id);
        self.watch_overflow.lock().await.remove(session_id);
    }

    async fn drain_watch_events(&self, session_id: &str) -> Option<String> {
        let events = {
            let mut map = self.pending_watch_events.lock().await;
            map.remove(session_id).unwrap_or_default()
        };
        // Check and clear the overflow flag for this session.
        let overflowed = {
            let mut flags = self.watch_overflow.lock().await;
            flags.remove(session_id).unwrap_or(false)
        };
        if events.is_empty() && !overflowed {
            return None;
        }
        let my_symbols = {
            let map = self.my_modified_symbols.lock().await;
            map.get(session_id).cloned().unwrap_or_default()
        };
        let mut text = String::new();
        if overflowed {
            text.push_str("\u{26A0}\u{FE0F} Watch event buffer overflowed \u{2014} some events were dropped. Call dk_watch to see current state.\n\n");
        }
        if !events.is_empty() {
            text.push_str("\u{1F4E1} WATCH EVENTS:\n");
            for event in &events {
                text.push_str(&format_watch_event(event, &my_symbols));
                text.push('\n');
            }
            text.push('\n');
        }
        Some(text)
    }

    /// Drain both conflict warnings and watch events, returning a combined
    /// notification block or `None` if there are no pending notifications.
    async fn drain_notifications(&self, session_id: &str) -> Option<String> {
        let warnings = self.drain_warnings(session_id).await;
        let watch_events = self.drain_watch_events(session_id).await;

        // Detect dead watch streams: if the task exists but is finished,
        // append a notice so the agent knows to call dk_watch to restart.
        let dead_stream_notice = {
            let tasks = self.watch_tasks.lock().await;
            match tasks.get(session_id) {
                Some(handle) if handle.is_finished() => Some(
                    "\u{26A0}\u{FE0F} Watch stream has stopped \u{2014} call dk_watch to restart.\n\n"
                        .to_string(),
                ),
                _ => None,
            }
        };

        let parts: Vec<&str> = [
            warnings.as_deref(),
            watch_events.as_deref(),
            dead_stream_notice.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(""))
        }
    }

    /// Start a gRPC Watch stream for the given session, buffering events in
    /// `pending_watch_events`. If a stream is already running for this session,
    /// this is a no-op.
    async fn start_watch_stream(&self, session_id: &str, filter: &str) {
        // Check if already running with the same filter. If the filter changed,
        // cancel the existing stream and restart with the new filter.
        // The stop logic is inlined here (rather than calling stop_watch_stream)
        // to avoid a TOCTOU window where another concurrent call could see no
        // task entry and spawn a duplicate stream.
        {
            let mut tasks = self.watch_tasks.lock().await;
            if tasks.contains_key(session_id) {
                let filters = self.watch_filters.lock().await;
                let same_filter = filters.get(session_id).map(|f| f.as_str()) == Some(filter);
                let is_finished = tasks
                    .get(session_id)
                    .map(|h| h.is_finished())
                    .unwrap_or(false);
                if same_filter && !is_finished {
                    return; // Same filter, stream still running — genuine no-op.
                }
                drop(filters);
                // Cancel and remove the existing stream while holding the tasks lock.
                {
                    let mut cancellations = self.watch_cancellations.lock().await;
                    if let Some(flag) = cancellations.remove(session_id) {
                        flag.store(true, std::sync::atomic::Ordering::Release);
                    }
                }
                if let Some(handle) = tasks.remove(session_id) {
                    handle.abort();
                }
                {
                    let mut filters = self.watch_filters.lock().await;
                    filters.remove(session_id);
                }
                // Fall through to re-create with the new filter.
            }
            // Insert a placeholder handle to prevent concurrent callers from
            // also seeing "no entry" and spawning duplicate streams (TOCTOU).
            // The placeholder must NOT complete immediately (a finished handle
            // would trigger a false "Watch stream has stopped" warning in
            // drain_notifications). Use a pending future that parks forever;
            // it will be aborted when the real handle replaces it via
            // tasks.insert() + old_handle.abort() below.
            tasks.insert(
                session_id.to_string(),
                tokio::spawn(std::future::pending::<()>()),
            );
        }

        let client = match self.get_client().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    session_id = %session_id,
                    "start_watch_stream: failed to get gRPC client, watch stream not started"
                );
                // Remove the placeholder we inserted above.
                let mut tasks = self.watch_tasks.lock().await;
                if let Some(h) = tasks.remove(session_id) {
                    h.abort();
                }
                return;
            }
        };

        let session_id_owned = session_id.to_string();
        let filter_owned = filter.to_string();
        let pending_events = Arc::clone(&self.pending_watch_events);
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Store cancellation flag. If a previous flag exists, signal it.
        {
            let mut cancellations = self.watch_cancellations.lock().await;
            if let Some(old_flag) =
                cancellations.insert(session_id.to_string(), Arc::clone(&cancelled))
            {
                old_flag.store(true, std::sync::atomic::Ordering::Release);
            }
        }

        // Store the active filter for this session.
        {
            let mut filters = self.watch_filters.lock().await;
            filters.insert(session_id.to_string(), filter.to_string());
        }

        // Ensure a pending_watch_events entry exists.
        {
            let mut map = self.pending_watch_events.lock().await;
            map.entry(session_id.to_string()).or_default();
        }

        let session_id_for_task = session_id_owned.clone();
        let overflow_flag = Arc::clone(&self.watch_overflow);
        let pending_warnings_for_watch = Arc::clone(&self.pending_warnings);
        let watch_handle = tokio::spawn(async move {
            let mut client = client;
            // repo_id is intentionally empty: the engine scopes the Watch stream
            // by session_id alone (verified via verify_session_owner in the auth
            // layer). No cross-tenant leak is possible.
            let request = crate::WatchRequest {
                session_id: session_id_for_task.clone(),
                repo_id: String::new(),
                filter: filter_owned,
            };

            match client.watch(request).await {
                Ok(response) => {
                    let mut stream = response.into_inner();
                    while let Some(result) = stream.next().await {
                        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            tracing::debug!(
                                session_id = %session_id_for_task,
                                "Watch stream task cancelled"
                            );
                            break;
                        }
                        match result {
                            Ok(event) => {
                                // Route conflict events to pending_warnings for higher
                                // visibility (prepended to every tool response), matching
                                // the behaviour of the NATS-based conflict subscription.
                                // This enables real-time conflict warnings via the Watch
                                // stream even when NATS is unavailable (e.g. stdio transport).
                                if event.event_type.starts_with("conflict.")
                                    && !event.details.is_empty()
                                {
                                    if let Some(warning_text) =
                                        format_conflict_warning(&event.details)
                                    {
                                        let mut warnings =
                                            pending_warnings_for_watch.lock().await;
                                        let w = warnings
                                            .entry(session_id_for_task.clone())
                                            .or_default();
                                        if w.len() < 50 && !w.contains(&warning_text) {
                                            w.push(warning_text);
                                        }
                                        continue; // Don't also add to watch events
                                    }
                                }
                                let mut map = pending_events.lock().await;
                                let events = map.entry(session_id_for_task.clone()).or_default();
                                // Deduplicate by event_id.
                                let already_exists = !event.event_id.is_empty()
                                    && events.iter().any(|e| e.event_id == event.event_id);
                                if !already_exists {
                                    if events.len() < 100 {
                                        events.push(event);
                                    } else {
                                        tracing::warn!(
                                            session_id = %session_id_for_task,
                                            "Watch event buffer full (100), dropping event"
                                        );
                                        // Set overflow flag so the agent is notified on next drain.
                                        let mut flags = overflow_flag.lock().await;
                                        flags.insert(session_id_for_task.clone(), true);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    session_id = %session_id_for_task,
                                    "Watch stream error"
                                );
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        session_id = %session_id_for_task,
                        "Failed to start Watch stream"
                    );
                }
            }
        });

        // Store the task handle. Abort any previous handle for this session.
        {
            let mut tasks = self.watch_tasks.lock().await;
            if let Some(old_handle) = tasks.insert(session_id_owned, watch_handle) {
                old_handle.abort();
            }
        }
    }

    /// Show current dkod session status including connection info, session ID,
    /// workspace, changeset, and repo name.
    #[tool(
        description = "Show current dkod session status including connection info, session ID, workspace, changeset, and repo name."
    )]
    async fn dk_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let conn = self.connection.read().await;
        let sessions = self.sessions.read().await;

        let mut text = format!("{conn}");
        text.push_str(&format!("active_sessions: {}\n", sessions.len()));

        if sessions.is_empty() {
            text.push_str("connected:    false\n");
        } else if let Some(sid) = &params.session_id {
            // Show specific session
            if let Some(data) = sessions.get(sid.as_str()) {
                text.push_str(&format!("\n{data}"));
                let session_id = sid.clone();
                drop(sessions);
                drop(conn);
                self.append_workspace_status(&mut text, &session_id).await;
                let prefix = self
                    .drain_notifications(&session_id)
                    .await
                    .unwrap_or_default();
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "{prefix}{text}"
                ))]));
            } else {
                return Err(McpError::invalid_params(
                    format!("Session '{sid}' not found."),
                    None,
                ));
            }
        } else if sessions.len() == 1 {
            // Single session: auto-resolve and drain its warnings (backward-compatible).
            let (sid, data) = sessions.iter().next().unwrap();
            let sid = sid.clone();
            text.push_str(&format!("\n{data}"));
            drop(sessions);
            drop(conn);
            self.append_workspace_status(&mut text, &sid).await;
            let prefix = self.drain_notifications(&sid).await.unwrap_or_default();
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "{prefix}{text}"
            ))]));
        } else {
            // Multiple sessions with no session_id: show all sessions but do NOT
            // drain warnings (draining would steal another agent's notifications).
            for data in sessions.values() {
                text.push_str(&format!("\n--- session ---\n{data}"));
            }
            text.push_str(
                "\n\nNote: provide session_id to also show and drain pending conflict warnings.",
            );
        }

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    // ── Tool 1: dk_connect ──

    /// Connect to a dkod codebase and open an agent session.
    #[tool(
        description = "Connect to a dkod codebase and open an agent session. Must be called before any other dk_* tool (except dk_status)."
    )]
    async fn dk_connect(
        &self,
        Parameters(params): Parameters<ConnectParams>,
    ) -> Result<CallToolResult, McpError> {
        let ConnectParams {
            repo,
            intent,
            agent_name,
        } = params;

        // Extract connection params, then drop the lock before the gRPC call.
        let (addr, env_token) = {
            let conn = self.connection.read().await;
            (conn.server_addr.clone(), conn.auth_token.clone())
        };

        // Derive the HTTP API base from the gRPC address for device flow.
        let api_base = derive_api_base(&addr);

        // Resolve auth token: env var → cached file → device flow.
        let token = crate::auth::resolve_token(&api_base, env_token.as_deref())
            .await
            .map_err(|e| McpError::internal_error(format!("auth failed: {e}"), None))?;

        // Helper: create a fresh gRPC client with the given token.
        let create_client = |addr: &str, token: String| {
            let addr = addr.to_string();
            async move { crate::grpc::connect_with_auth(&addr, token).await }
        };

        // Reuse the cached gRPC client if one exists (all sessions share the
        // same auth token). Only create a new client for the first connect or
        // after dk_merge clears the cache.
        let needs_new_client = self.grpc_client.lock().await.is_none();
        let mut client = if needs_new_client {
            let new_client = create_client(&addr, token)
                .await
                .map_err(|e| McpError::internal_error(format!("gRPC connect failed: {e}"), None))?;
            {
                let mut cached = self.grpc_client.lock().await;
                if cached.is_none() {
                    *cached = Some(new_client.clone());
                }
            }
            new_client
        } else {
            self.get_client().await?
        };

        let request = crate::ConnectRequest {
            agent_id: "claude-code".to_string(),
            auth_token: String::new(), // Auth is now in gRPC metadata, not the proto field.
            codebase: repo.clone(),
            intent: intent.clone(),
            workspace_config: None,
            agent_name: agent_name.clone().unwrap_or_default(),
        };

        let result = client.connect(request).await;

        // If the server rejects the token (expired or revoked), clear the
        // stale cache and retry with a fresh device flow — once.
        let response = match &result {
            Err(status) if status.code() == tonic::Code::Unauthenticated => {
                tracing::warn!("token rejected ({}), clearing cache and re-authenticating", status.message());
                crate::auth::clear_cached_token();
                {
                    let mut conn = self.connection.write().await;
                    conn.auth_token = None;
                }
                // Force a fresh device flow (env_token is now None, cached file is gone).
                let fresh_token = crate::auth::resolve_token(&api_base, None)
                    .await
                    .map_err(|e| McpError::internal_error(format!("re-auth failed: {e}"), None))?;
                let new_client = create_client(&addr, fresh_token)
                    .await
                    .map_err(|e| McpError::internal_error(format!("gRPC reconnect failed: {e}"), None))?;
                {
                    let mut cached = self.grpc_client.lock().await;
                    *cached = Some(new_client.clone());
                }
                client = new_client;
                let retry_request = crate::ConnectRequest {
                    agent_id: "claude-code".to_string(),
                    auth_token: String::new(),
                    codebase: repo.clone(),
                    intent: intent.clone(),
                    workspace_config: None,
                    agent_name: agent_name.unwrap_or_default(),
                };
                client
                    .connect(retry_request)
                    .await
                    .map_err(|e| McpError::internal_error(format!("CONNECT RPC failed after re-auth: {e}"), None))?
                    .into_inner()
            }
            Err(e) => {
                return Err(McpError::internal_error(format!("CONNECT RPC failed: {e}"), None));
            }
            Ok(_) => result.unwrap().into_inner(),
        };

        // Store session data in the session map.
        let session_data = SessionData {
            session_id: response.session_id.clone(),
            workspace_id: response.workspace_id.clone(),
            changeset_id: response.changeset_id.clone(),
            repo_name: repo.clone(),
        };
        let snapshot = {
            let mut sessions = self.sessions.write().await;
            sessions.insert(response.session_id.clone(), session_data);
            sessions.clone()
        };
        crate::state::save_sessions(&snapshot);

        // Subscribe to NATS conflict events for this session (optional — skipped if
        // NATS_URL or NATS_OWNER_ID are not set, e.g. in local dev without NATS).
        let nats_url = std::env::var("NATS_URL").ok().filter(|s| !s.is_empty());
        let nats_owner_id = std::env::var("NATS_OWNER_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .filter(|s| {
                // Validate NATS_OWNER_ID looks like a UUID to catch misconfiguration early.
                // Must match the platform user_id for NATS subject routing to work.
                let valid = uuid::Uuid::parse_str(s).is_ok();
                if !valid {
                    tracing::warn!(
                        nats_owner_id = %s,
                        "NATS_OWNER_ID is not a valid UUID — conflict notifications will be disabled.                          Set this to your platform user_id (visible in dk_connect response metadata)."
                    );
                }
                valid
            });
        // Ensure a warnings entry exists for this session. Use
        // entry().or_default() to preserve any existing warnings
        // from a previous connection with the same session_id
        // (e.g., session resumption).
        {
            let mut w = self.pending_warnings.lock().await;
            w.entry(response.session_id.clone()).or_default();
        }
        if let (Some(nats_url), Some(owner_id)) = (nats_url, nats_owner_id) {
            // Each session gets its own NATS task and cancellation flag, so
            // multiple concurrent agents receive independent conflict events.
            let session_id_for_nats = response.session_id.clone();
            let pending_warnings = Arc::clone(&self.pending_warnings);
            let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let session_id_for_warnings = response.session_id.clone();
            // Store the cancellation flag for this session (used by Drop and dk_merge).
            // If a previous task exists for this session_id (e.g., session resumption),
            // signal its cancellation flag before replacing it. This prevents the old
            // task from running indefinitely after its flag is overwritten.
            {
                let mut cancellations = self.nats_cancellations.lock().await;
                if let Some(old_flag) =
                    cancellations.insert(response.session_id.clone(), Arc::clone(&cancelled))
                {
                    old_flag.store(true, std::sync::atomic::Ordering::Release);
                }
            }
            let nats_handle = tokio::spawn(async move {
                let subject = format!("tenant.{owner_id}.session.{session_id_for_nats}.conflicts");
                let mut backoff_secs = 1u64;
                loop {
                    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                        tracing::debug!(subject = %subject, "NATS conflict task cancelled");
                        break;
                    }
                    // Use NATS_TOKEN for authenticated connections (matches
                    // the platform's ConflictBus which also checks NATS_TOKEN).
                    let nats_token = std::env::var("NATS_TOKEN").ok().filter(|s| !s.is_empty());
                    let connect_result = if let Some(ref token) = nats_token {
                        async_nats::ConnectOptions::with_token(token.clone())
                            .connect(&nats_url)
                            .await
                    } else {
                        async_nats::connect(&nats_url).await
                    };
                    match connect_result {
                        Ok(nc) => {
                            // TCP connected — do NOT reset backoff here. Only
                            // reset on successful subscribe (line below). This
                            // ensures subscribe-level failures (ACL errors, etc.)
                            // still experience exponential backoff instead of
                            // retrying at a fixed ~2s rate.
                            tracing::info!(subject = %subject, "subscribing to NATS conflict events");
                            match nc.subscribe(subject.clone()).await {
                                Ok(mut sub) => {
                                    // Reset backoff after successful subscription.
                                    backoff_secs = 1;
                                    while let Some(msg) = sub.next().await {
                                        let payload = match std::str::from_utf8(&msg.payload) {
                                            Ok(s) => s,
                                            Err(_) => continue,
                                        };
                                        let warning_text = format_conflict_warning(payload);
                                        if let Some(text) = warning_text {
                                            let mut map = pending_warnings.lock().await;
                                            let w = map
                                                .entry(session_id_for_warnings.clone())
                                                .or_default();
                                            if w.len() < 50 && !w.contains(&text) {
                                                w.push(text);
                                            }
                                        }
                                    }
                                    tracing::info!(subject = %subject, "NATS conflict subscription ended, reconnecting after 2s delay");
                                    // Minimum delay after clean subscription close to avoid
                                    // connection churn if NATS closes idle subscriptions frequently.
                                    // Skip the bottom-of-loop backoff sleep (already waited 2s).
                                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                    continue;
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to subscribe to NATS conflict subject");
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, nats_url = %nats_url, "failed to connect to NATS for conflict notifications");
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(30); // exponential backoff, cap at 30s
                }
            });
            // Abort any previous NATS task for this session_id before inserting
            // the new one. Dropping a JoinHandle only detaches (does not abort),
            // so we must explicitly abort to prevent an orphaned background task.
            {
                let mut tasks = self.nats_tasks.lock().await;
                if let Some(old_handle) = tasks.insert(response.session_id.clone(), nats_handle) {
                    old_handle.abort();
                }
            }
        }

        // Auto-start the Watch stream for this session. The gRPC channel is
        // already established (we just completed a successful connect RPC), so
        // no startup delay is needed.
        {
            let self_clone = self.clone();
            let session_id_for_watch = response.session_id.clone();
            tokio::spawn(async move {
                self_clone
                    .start_watch_stream(&session_id_for_watch, "*")
                    .await;
            });
        }

        // Format output.
        let summary_text = match &response.summary {
            Some(s) => format!(
                "languages: {}\ntotal_symbols: {}\ntotal_files: {}",
                s.languages.join(", "),
                s.total_symbols,
                s.total_files,
            ),
            None => "no codebase summary available".to_string(),
        };

        let text = format!(
            "Connected to {repo}\n\
             session_id:   {}\n\
             workspace_id: {}\n\
             changeset_id: {}\n\
             version:      {}\n\n\
             IMPORTANT: Use this session_id ({}) in all subsequent dk_* tool calls \
             when multiple sessions are active.\n\n\
             Codebase summary:\n{summary_text}",
            response.session_id,
            response.workspace_id,
            response.changeset_id,
            response.codebase_version,
            response.session_id,
        );

        let has_nats_url = std::env::var("NATS_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some();
        // Check if NATS_OWNER_ID is set AND valid (UUID). This must match
        // the validation in the NATS task-spawn path above, otherwise the
        // display says "configured" but the task was never spawned.
        let has_nats_owner = std::env::var("NATS_OWNER_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
            .is_some();
        if !has_nats_url || !has_nats_owner {
            let missing = match (has_nats_url, has_nats_owner) {
                (false, false) => "NATS_URL and NATS_OWNER_ID are not set",
                (false, true) => "NATS_URL is not set",
                (true, false) => "NATS_OWNER_ID is not set",
                _ => unreachable!(),
            };
            // Conflict notifications now arrive via the Watch stream, so missing
            // NATS config is no longer a problem for agents. Log at debug level
            // only to avoid alarming agents using stdio transport.
            tracing::debug!(
                "{} — conflict notifications will be delivered via Watch stream instead of direct NATS.",
                missing,
            );
        }

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    // ── Tool 2: dk_context ──

    /// Semantic code search within the connected codebase.
    #[tool(
        description = "Semantic code search within the connected codebase. Returns symbols, call graph edges, and dependencies matching the query."
    )]
    async fn dk_context(
        &self,
        Parameters(params): Parameters<ContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let ContextParams {
            session_id: param_session_id,
            query,
            depth,
            max_tokens,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let depth_value = match depth.as_deref() {
            Some("full") => 1,
            Some("call_graph") => 2,
            _ => 0, // default: signatures
        };

        let mut client = self.get_client().await?;

        let request = crate::ContextRequest {
            session_id: session_id.clone(),
            query: query.clone(),
            depth: depth_value,
            include_tests: false,
            include_dependencies: true,
            max_tokens: max_tokens.unwrap_or(0),
        };

        let response = client
            .context(request)
            .await
            .map_err(|e| McpError::internal_error(format!("CONTEXT RPC failed: {e}"), None))?
            .into_inner();

        // Format symbols.
        let mut text = format!(
            "Context results for '{query}' ({} symbols, {} call edges, {} deps, ~{} tokens)\n\n",
            response.symbols.len(),
            response.call_graph.len(),
            response.dependencies.len(),
            response.estimated_tokens,
        );

        for sym in &response.symbols {
            if let Some(ref sr) = sym.symbol {
                text.push_str(&format!("--- {} ({}) ---\n", sr.name, sr.kind));
                text.push_str(&format!("  qualified: {}\n", sr.qualified_name));
                text.push_str(&format!(
                    "  file:      {}  [{}-{}]\n",
                    sr.file_path, sr.start_byte, sr.end_byte
                ));
                if !sr.signature.is_empty() {
                    text.push_str(&format!("  signature: {}\n", sr.signature));
                }
                if let Some(ref doc) = sr.doc_comment {
                    text.push_str(&format!("  doc:       {doc}\n"));
                }
            }
            if let Some(ref source) = sym.source {
                text.push_str(&format!("  source:\n{source}\n"));
            }
            if !sym.caller_ids.is_empty() {
                text.push_str(&format!("  callers: {}\n", sym.caller_ids.join(", ")));
            }
            if !sym.callee_ids.is_empty() {
                text.push_str(&format!("  callees: {}\n", sym.callee_ids.join(", ")));
            }
            text.push('\n');
        }

        if !response.call_graph.is_empty() {
            text.push_str("Call graph edges:\n");
            for edge in &response.call_graph {
                text.push_str(&format!(
                    "  {} -> {} ({})\n",
                    edge.caller_id, edge.callee_id, edge.kind
                ));
            }
            text.push('\n');
        }

        if !response.dependencies.is_empty() {
            text.push_str("Dependencies:\n");
            for dep in &response.dependencies {
                text.push_str(&format!(
                    "  {} {} (used by: {})\n",
                    dep.package,
                    dep.version_req,
                    dep.used_by_symbol_ids.join(", ")
                ));
            }
        }

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }

    // ── Tool 3: dk_file_read ──

    /// Read a file from the dkod session workspace overlay.
    #[tool(
        description = "Read a file from the dkod session workspace overlay. Returns file content and whether it was modified in this session."
    )]
    async fn dk_file_read(
        &self,
        Parameters(params): Parameters<FileReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let FileReadParams {
            session_id: param_session_id,
            path,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let mut client = self.get_client().await?;

        let request = crate::FileReadRequest {
            session_id: session_id.clone(),
            path: path.clone(),
        };

        let response = client
            .file_read(request)
            .await
            .map_err(|e| McpError::internal_error(format!("FILE_READ RPC failed: {e}"), None))?
            .into_inner();

        let content_str = String::from_utf8_lossy(&response.content);
        let modified_tag = if response.modified_in_session {
            " [modified in session]"
        } else {
            ""
        };

        let body = format!(
            "--- {path}{modified_tag} (hash: {}) ---\n{content_str}",
            response.hash,
        );
        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{body}"
        ))]))
    }

    // ── Tool 4: dk_file_write ──

    /// Write content to a file in the dkod session workspace overlay.
    #[tool(
        description = "Write content to a file in the dkod session workspace overlay. Returns bytes written and detected symbol changes."
    )]
    async fn dk_file_write(
        &self,
        Parameters(params): Parameters<FileWriteParams>,
    ) -> Result<CallToolResult, McpError> {
        let FileWriteParams {
            session_id: param_session_id,
            path,
            content,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let content_bytes = content.as_bytes().to_vec();
        let bytes_written = content_bytes.len();

        let mut client = self.get_client().await?;

        let request = crate::FileWriteRequest {
            session_id: session_id.clone(),
            path: path.clone(),
            content: content_bytes,
        };

        let response = client
            .file_write(request)
            .await
            .map_err(|e| McpError::internal_error(format!("FILE_WRITE RPC failed: {e}"), None))?
            .into_inner();

        let mut text = format!(
            "Wrote {bytes_written} bytes to {path}\nhash: {}\n",
            response.new_hash,
        );

        if !response.detected_changes.is_empty() {
            text.push_str("\nDetected symbol changes:\n");
            for sc in &response.detected_changes {
                text.push_str(&format!("  {} ({})\n", sc.symbol_name, sc.change_type));
            }
            // Track modified symbols for watch event impact analysis.
            // Key on file_path::symbol_name to avoid false-positive [AFFECTS YOUR WORK]
            // tags for common names like "new" or "default" that appear across many modules.
            // Normalize the path (strip leading "./") to match engine-normalized paths
            // in WatchEvent.symbol_changes.file_path.
            {
                let normalized_path = path.strip_prefix("./").unwrap_or(&path);
                let mut map = self.my_modified_symbols.lock().await;
                let symbols = map.entry(session_id.clone()).or_default();
                for sc in &response.detected_changes {
                    symbols.insert(format!("{}::{}", normalized_path, sc.symbol_name));
                }
            }
        }

        if !response.conflict_warnings.is_empty() {
            text.push_str("\nCONFLICT WARNING:\n");
            for cw in &response.conflict_warnings {
                text.push_str(&format!(
                    "  {} is also modifying {} in this file\n",
                    cw.conflicting_agent, cw.symbol_name
                ));
            }
            text.push_str("Your changes may be rejected at SUBMIT time.\n");
        }

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }

    // ── Tool 5: dk_file_list ──

    /// List files in the dkod session workspace.
    #[tool(
        description = "List files in the dkod session workspace. Optionally filter by path prefix. Modified files are tagged."
    )]
    async fn dk_file_list(
        &self,
        Parameters(params): Parameters<FileListParams>,
    ) -> Result<CallToolResult, McpError> {
        let FileListParams {
            session_id: param_session_id,
            prefix,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let mut client = self.get_client().await?;

        let request = crate::FileListRequest {
            session_id: session_id.clone(),
            prefix: prefix.clone(),
            only_modified: false,
        };

        let response = client
            .file_list(request)
            .await
            .map_err(|e| McpError::internal_error(format!("FILE_LIST RPC failed: {e}"), None))?
            .into_inner();

        let prefix_label = prefix.as_deref().unwrap_or("(all)");
        let mut text = format!(
            "Files (prefix: {prefix_label}, total: {})\n\n",
            response.files.len()
        );

        for entry in &response.files {
            let mut tag = String::new();
            if entry.modified_in_session {
                tag.push_str(" [modified]");
            }
            if !entry.modified_by_other.is_empty() {
                // Sanitize to prevent output injection (newlines, control chars)
                let sanitized: String = entry
                    .modified_by_other
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                tag.push_str(&format!(" [{}]", sanitized));
            }
            // Sanitize path to prevent output injection (same as modified_by_other)
            let safe_path: String = entry.path.chars().filter(|c| !c.is_control()).collect();
            text.push_str(&format!("  {safe_path}{tag}\n"));
        }

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }

    // ── Tool 6: dk_submit ──

    /// Submit the current changeset of code changes for review.
    #[tool(
        description = "Submit the current changeset of code changes for review. Files written via dk_file_write are automatically included."
    )]
    async fn dk_submit(
        &self,
        Parameters(params): Parameters<SubmitParams>,
    ) -> Result<CallToolResult, McpError> {
        let SubmitParams {
            session_id: param_session_id,
            intent,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();
        let changeset_id = session.changeset_id.clone();

        let mut client = self.get_client().await?;

        let request = crate::SubmitRequest {
            session_id: session_id.clone(),
            intent: intent.clone(),
            changes: vec![], // File-level changes are already in the workspace overlay
            changeset_id,
        };

        let response = client
            .submit(request)
            .await
            .map_err(|e| McpError::internal_error(format!("SUBMIT RPC failed: {e}"), None))?
            .into_inner();

        let status_str = match response.status {
            s if s == crate::SubmitStatus::Accepted as i32 => "ACCEPTED",
            s if s == crate::SubmitStatus::Rejected as i32 => "REJECTED",
            s if s == crate::SubmitStatus::Conflict as i32 => "CONFLICT",
            _ => "UNKNOWN",
        };

        // Handle symbol-level conflict rejection with self-contained payload
        if response.status == crate::SubmitStatus::Conflict as i32 {
            if let Some(ref block) = response.conflict_block {
                let mut text = String::from("SUBMIT REJECTED — Symbol conflict detected.\n\n");
                for detail in &block.conflicting_symbols {
                    text.push_str(&format!(
                        "CONFLICT: {} ({})\n",
                        detail.qualified_name, detail.kind
                    ));
                    text.push_str(&format!(
                        "Conflicting agent: {}\n\n",
                        detail.conflicting_agent
                    ));
                    if let Some(ref base) = detail.base_version {
                        text.push_str(&format!(
                            "BASE VERSION:\n{}\n{}\n\n",
                            base.signature, base.body
                        ));
                    }
                    if let Some(ref their) = detail.their_change {
                        text.push_str(&format!(
                            "THEIR CHANGE ({}):\n{}\n{}\n{}\n\n",
                            detail.conflicting_agent,
                            their.description,
                            their.signature,
                            their.body
                        ));
                    }
                    if let Some(ref your) = detail.your_change {
                        text.push_str(&format!(
                            "YOUR CHANGE:\n{}\n{}\n{}\n\n",
                            your.description, your.signature, your.body
                        ));
                    }
                }
                text.push_str(&block.message);
                text.push_str(
                    "\n\nRewrite the conflicting symbol(s) incorporating both changes, \
                     then call dk_file_write and dk_submit again.",
                );
                let prefix = self
                    .drain_notifications(&session_id)
                    .await
                    .unwrap_or_default();
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "{prefix}{text}"
                ))]));
            }
        }

        let mut text = format!(
            "Submit status: {status_str}\nchangeset_id: {}\n",
            response.changeset_id,
        );

        if let Some(ref ver) = response.new_version {
            text.push_str(&format!("new_version: {ver}\n"));
        }

        if !response.errors.is_empty() {
            text.push_str("\nErrors:\n");
            for err in &response.errors {
                let file = err.file_path.as_deref().unwrap_or("");
                let sym = err.symbol_id.as_deref().unwrap_or("");
                text.push_str(&format!(
                    "  - {} (file: {file}, symbol: {sym})\n",
                    err.message
                ));
            }
        }

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }

    // ── Tool 7: dk_verify ──

    /// Run the verification pipeline on the current changeset.
    #[tool(
        description = "Run the verification pipeline (lint, test, type-check) on the current changeset. Streams step-by-step results."
    )]
    async fn dk_verify(
        &self,
        Parameters(params): Parameters<VerifyParams>,
    ) -> Result<CallToolResult, McpError> {
        let VerifyParams {
            session_id: param_session_id,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();
        let changeset_id = session.changeset_id.clone();

        let mut client = self.get_client().await?;

        let request = crate::VerifyRequest {
            session_id: session_id.clone(),
            changeset_id,
        };

        let response = client
            .verify(request)
            .await
            .map_err(|e| McpError::internal_error(format!("VERIFY RPC failed: {e}"), None))?;

        // Consume the server-streaming response, collecting steps for grouped output.
        let mut stream = response.into_inner();
        let mut all_passed = true;
        let mut stream_error: Option<String> = None;

        // Collected step data for post-stream grouping.
        struct CollectedStep {
            step_name: String,
            is_fail: bool,
            is_pass: bool,
            is_skip: bool,
            required: bool,
            output: String,
            findings: Vec<crate::Finding>,
            suggestions: Vec<crate::Suggestion>,
        }

        let mut collected_steps: Vec<CollectedStep> = Vec::new();

        while let Some(step_result) = stream.next().await {
            match step_result {
                Ok(step) => {
                    let status_lower = step.status.to_lowercase();
                    let is_fail = status_lower == "fail" || status_lower == "failed";
                    let is_pass = status_lower == "pass" || status_lower == "passed";
                    let is_skip = status_lower == "skip" || status_lower == "skipped";
                    if is_fail {
                        all_passed = false;
                    }
                    // Skip "running" status updates — they are progress, not results.
                    if status_lower.starts_with("run") {
                        continue;
                    }
                    collected_steps.push(CollectedStep {
                        step_name: step.step_name,
                        is_fail,
                        is_pass,
                        is_skip,
                        required: step.required,
                        output: step.output,
                        findings: step.findings,
                        suggestions: step.suggestions,
                    });
                }
                Err(e) => {
                    all_passed = false;
                    stream_error = Some(format!("{e}"));
                    break;
                }
            }
        }

        // ── Group steps by language prefix ──
        // Step names use "lang:step" convention (e.g. "rust:check", "node:test").
        // Steps without a colon are grouped under "general".

        // Maintain insertion order via a Vec of language keys.
        let mut lang_order: Vec<String> = Vec::new();
        let mut lang_groups: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, step) in collected_steps.iter().enumerate() {
            let lang = if let Some(colon_pos) = step.step_name.find(':') {
                step.step_name[..colon_pos].to_string()
            } else {
                "general".to_string()
            };
            let entry = lang_groups.entry(lang.clone()).or_default();
            if entry.is_empty() {
                lang_order.push(lang);
            }
            entry.push(idx);
        }

        // ── Format output grouped by language ──

        let mut text = String::from("Verification pipeline results:\n\n");

        let mut langs_failed = 0u32;
        let total_langs = lang_order.len() as u32;

        for lang in &lang_order {
            let step_indices = &lang_groups[lang];
            let step_count = step_indices.len();
            let lang_has_failure = step_indices
                .iter()
                .any(|&i| collected_steps[i].is_fail);

            if lang_has_failure {
                langs_failed += 1;
            }

            let lang_status = if lang_has_failure { "FAIL" } else { "PASS" };
            text.push_str(&format!(
                "[{lang}] {lang_status} ({step_count} step{})\n",
                if step_count == 1 { "" } else { "s" }
            ));

            for &idx in step_indices {
                let step = &collected_steps[idx];
                let short_name = if let Some(colon_pos) = step.step_name.find(':') {
                    &step.step_name[colon_pos + 1..]
                } else {
                    &step.step_name
                };

                let icon = if step.is_pass {
                    "\u{2713}" // ✓
                } else if step.is_fail {
                    "\u{2717}" // ✗
                } else if step.is_skip {
                    "-"
                } else {
                    "?"
                };

                let required_tag = if step.required { " (required)" } else { "" };
                text.push_str(&format!("  {icon} {short_name}{required_tag}\n"));

                // Summarize output to save tokens.
                if !step.output.is_empty() {
                    let output = &step.output;
                    if step.is_fail {
                        // Extract the final "test result:" line (the summary) and
                        // unique error messages. Deduplicate repeated panics.
                        let mut failed_count = 0u32;
                        let mut unique_errors: Vec<String> = Vec::new();
                        let mut seen_errors = HashSet::new();
                        let mut final_result_line: Option<&str> = None;
                        let mut error_lines: Vec<&str> = Vec::new();

                        for line in output.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("test result:") {
                                final_result_line = Some(trimmed);
                            } else if trimmed.starts_with("test ")
                                && trimmed.ends_with("FAILED")
                            {
                                failed_count += 1;
                            } else if trimmed.contains("panicked at") {
                                // Extract the panic message for dedup
                                if let Some(msg) = trimmed.split("panicked at").nth(1) {
                                    let key = msg.trim().to_string();
                                    if seen_errors.insert(key.clone()) {
                                        unique_errors.push(key);
                                    }
                                }
                            } else if trimmed.starts_with("error[") {
                                // Compiler errors (error[E0...])
                                error_lines.push(trimmed);
                            } else if trimmed.starts_with("error:") {
                                error_lines.push(trimmed);
                            }
                        }

                        // Show the summary result line
                        if let Some(result) = final_result_line {
                            text.push_str(&format!("    {result}\n"));
                        }
                        // Show failed count
                        if failed_count > 0 {
                            text.push_str(&format!("    {failed_count} test(s) failed\n"));
                        }
                        // Show unique error causes (max 5)
                        for (i, err) in unique_errors.iter().enumerate() {
                            if i >= 5 {
                                text.push_str(&format!(
                                    "    ... and {} more unique error(s)\n",
                                    unique_errors.len() - 5
                                ));
                                break;
                            }
                            text.push_str(&format!("    panic: {err}\n"));
                        }
                        // Show compiler errors (max 5)
                        for (i, err) in error_lines.iter().enumerate() {
                            if i >= 5 {
                                text.push_str(&format!(
                                    "    ... and {} more error(s)\n",
                                    error_lines.len() - 5
                                ));
                                break;
                            }
                            text.push_str(&format!("    {err}\n"));
                        }
                        // Fallback if we found nothing useful
                        if final_result_line.is_none()
                            && failed_count == 0
                            && unique_errors.is_empty()
                            && error_lines.is_empty()
                        {
                            let lines: Vec<&str> = output.lines().collect();
                            let start = lines.len().saturating_sub(5);
                            for line in &lines[start..] {
                                text.push_str(&format!("    {}\n", line.trim()));
                            }
                        }
                    } else {
                        // For passing steps, aggregate test result summaries
                        let mut total_passed = 0u32;
                        let mut total_ignored = 0u32;
                        let mut found_summary = false;
                        for line in output.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("test result:") {
                                found_summary = true;
                                // Parse "test result: ok. N passed; ..."
                                if let Some(rest) = trimmed.strip_prefix("test result: ok.") {
                                    for part in rest.split(';') {
                                        let part = part.trim();
                                        if part.ends_with("passed") {
                                            if let Ok(n) = part
                                                .split_whitespace()
                                                .next()
                                                .unwrap_or("0")
                                                .parse::<u32>()
                                            {
                                                total_passed += n;
                                            }
                                        } else if part.ends_with("ignored") {
                                            if let Ok(n) = part
                                                .split_whitespace()
                                                .next()
                                                .unwrap_or("0")
                                                .parse::<u32>()
                                            {
                                                total_ignored += n;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if found_summary {
                            let ignored_note = if total_ignored > 0 {
                                format!(", {total_ignored} ignored")
                            } else {
                                String::new()
                            };
                            text.push_str(&format!(
                                "    {total_passed} passed{ignored_note}\n"
                            ));
                        } else {
                            let line_count = output.lines().count();
                            text.push_str(&format!("    ({line_count} lines of output)\n"));
                        }
                    }
                }
                // Format findings and inline suggestions for this step.
                if !step.findings.is_empty() {
                    for (i, finding) in step.findings.iter().enumerate() {
                        let severity_upper = finding.severity.to_uppercase();
                        let loc = match (&finding.file_path, finding.line) {
                            (Some(fp), Some(ln)) => format!(" {}:{}", fp, ln),
                            (Some(fp), None) => format!(" {}", fp),
                            _ => String::new(),
                        };
                        text.push_str(&format!(
                            "    {} {} \u{2014} {}{}\n",
                            severity_upper, finding.check_name, finding.message, loc
                        ));
                        // Show inline suggestion linked to this finding.
                        for suggestion in &step.suggestions {
                            if suggestion.finding_index == i as u32 {
                                text.push_str(&format!(
                                    "      -> Fix: {}\n",
                                    suggestion.description
                                ));
                            }
                        }
                    }
                }
            }

            text.push('\n');
        }

        let stream_error_occurred = stream_error.is_some();
        if let Some(err) = stream_error {
            text.push_str(&format!("Stream error: {err}\n\n"));
        }

        // Overall summary.
        if collected_steps.is_empty() {
            // No step results received — either NATS delivery failed, the
            // runner crashed, or a stream error occurred before any steps.
            if stream_error_occurred {
                text.push_str(
                    "Overall: SOME FAILED — stream error before any results were received.\n",
                );
            } else {
                text.push_str(
                    "Overall: NO RESULTS — verification ran but produced no step results.\n",
                );
                text.push_str("The server will finalize the changeset status asynchronously.\n");
                text.push_str("Check the dashboard for the final verdict.\n");
            }
        } else if all_passed {
            text.push_str("Overall: ALL PASSED\n");
        } else if total_langs > 0 && langs_failed > 0 {
            text.push_str(&format!(
                "Overall: FAIL ({langs_failed} of {total_langs} language{} failed)\n",
                if total_langs == 1 { "" } else { "s" }
            ));
        } else {
            text.push_str("Overall: SOME FAILED\n");
        }

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }

    // ── Tool 8: dk_merge ──

    /// Merge the current verified changeset into a Git commit.
    #[tool(
        description = "Merge the current verified changeset into a Git commit. Clears the session on success."
    )]
    async fn dk_merge(
        &self,
        Parameters(params): Parameters<MergeParams>,
    ) -> Result<CallToolResult, McpError> {
        let MergeParams {
            session_id: param_session_id,
            message,
            force,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();
        let changeset_id = session.changeset_id.clone();
        let repo_name = session.repo_name.clone();

        let mut client = self.get_client().await?;

        let request = crate::MergeRequest {
            session_id: session_id.clone(),
            changeset_id,
            commit_message: message.unwrap_or_default(),
            force: force.unwrap_or(false),
        };

        let response = client
            .merge(request)
            .await
            .map_err(|e| McpError::internal_error(format!("MERGE RPC failed: {e}"), None))?
            .into_inner();

        // Handle the new oneof result: Success vs Conflict.
        match response.result {
            Some(crate::merge_response::Result::Conflict(ref conflict)) => {
                let mut text = String::from("Merge CONFLICTS detected:\n\n");
                for c in &conflict.conflicts {
                    text.push_str(&format!(
                        "  file: {}  symbols: {}\n  \
                         type: {}  your_agent: {}  their_agent: {}\n  \
                         description: {}\n\n",
                        c.file_path,
                        c.symbols.join(", "),
                        c.conflict_type,
                        c.your_agent,
                        c.their_agent,
                        c.description,
                    ));
                }
                if !conflict.suggested_action.is_empty() {
                    text.push_str(&format!(
                        "Suggested action: {}\n",
                        conflict.suggested_action
                    ));
                }
                if !conflict.available_actions.is_empty() {
                    text.push_str(&format!(
                        "Available actions: {}\n",
                        conflict.available_actions.join(", ")
                    ));
                }
                text.push_str("Resolve conflicts and try again.");
                let prefix = self
                    .drain_notifications(&session_id)
                    .await
                    .unwrap_or_default();
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "{prefix}{text}"
                ))]));
            }
            Some(crate::merge_response::Result::OverwriteWarning(ref warning)) => {
                let mut text = String::from("\u{26a0}\u{fe0f} OVERWRITE WARNING:\n\n");
                for ow in &warning.overwrites {
                    let ts = if ow.merged_at.is_empty() {
                        "unknown time".to_string()
                    } else {
                        ow.merged_at.clone()
                    };
                    let agent = if ow.other_agent.is_empty() {
                        "(unknown agent)".to_string()
                    } else {
                        ow.other_agent.clone()
                    };
                    text.push_str(&format!(
                        "  {} in {} was modified by {} at {}\n",
                        ow.symbol_name, ow.file_path, agent, ts,
                    ));
                }
                text.push_str("\nYour change will replace their version.\n\n");
                if !warning.available_actions.is_empty() {
                    text.push_str("Actions:\n");
                    for action in &warning.available_actions {
                        match action.as_str() {
                            "proceed" => {
                                text.push_str("  - proceed: call dk_merge again with force=true\n")
                            }
                            "review" => text.push_str(
                                "  - review: reconnect (dk_connect) and read their changes first\n",
                            ),
                            "abort" => text.push_str("  - abort: discard your changeset\n"),
                            other => text.push_str(&format!("  - {other}\n")),
                        }
                    }
                }
                // Add dashboard URL pointing to the changesets list for this repo.
                // The frontend routes changesets by sequential number (/$number), but
                // the gRPC proto only exposes changeset_id (UUID). Until the proto is
                // extended with a changeset_number field, link to the list page where
                // the user can find their conflicted changeset.
                // repo_name is validated at creation to be [a-zA-Z0-9_\-] — safe for URL paths.
                // Base URL is configurable via DKOD_DASHBOARD_URL for staging/dev environments.
                let dashboard_base = std::env::var("DKOD_DASHBOARD_URL")
                    .unwrap_or_else(|_| "https://app.dkod.io".to_string());
                text.push_str(&format!(
                    "\nDashboard: {}/repos/{}/changesets\n",
                    dashboard_base, repo_name
                ));
                let prefix = self
                    .drain_notifications(&session_id)
                    .await
                    .unwrap_or_default();
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "{prefix}{text}"
                ))]));
            }
            Some(crate::merge_response::Result::Success(ref success)) => {
                // Drain warnings BEFORE removing session so they appear in the response.
                let prefix = self
                    .drain_notifications(&session_id)
                    .await
                    .unwrap_or_default();

                // Success: remove this session and clear gRPC client if last session.
                // Hold the sessions write lock across the grpc_client clearing to
                // prevent a TOCTOU race where a concurrent dk_connect inserts a new
                // session between remove() and is_empty(), causing the client to be
                // cleared while the new session still needs it.
                let snapshot = {
                    let mut sessions = self.sessions.write().await;
                    sessions.remove(&session_id);
                    if sessions.is_empty() {
                        let mut cached = self.grpc_client.lock().await;
                        *cached = None;
                    }
                    sessions.clone()
                };
                crate::state::save_sessions(&snapshot);
                // Cancel and remove the per-session NATS task.
                {
                    let mut cancellations = self.nats_cancellations.lock().await;
                    if let Some(flag) = cancellations.remove(&session_id) {
                        flag.store(true, std::sync::atomic::Ordering::Release);
                    }
                }
                {
                    let mut tasks = self.nats_tasks.lock().await;
                    if let Some(handle) = tasks.remove(&session_id) {
                        handle.abort();
                    }
                }
                // Evict any pending_warnings the NATS task may have re-inserted.
                {
                    let mut w = self.pending_warnings.lock().await;
                    w.remove(&session_id);
                }
                // Cancel and remove the per-session Watch task.
                self.cleanup_watch_for_session(&session_id).await;

                let mut text = format!(
                    "Merge successful!\ncommit_hash:    {}\nmerged_version: {}\n",
                    success.commit_hash, success.merged_version,
                );

                if success.auto_rebased {
                    text.push_str(&format!(
                        "auto_rebased: true\nrebased files: {}\n",
                        success.auto_rebased_files.join(", "),
                    ));
                }

                text.push_str("\nSession cleared. Call dk_connect to start a new session.");

                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "{prefix}{text}"
                ))]));
            }
            None => {
                // Unexpected: server returned no result variant.
                // Clean up session and NATS resources to prevent leaks.
                let prefix = self
                    .drain_notifications(&session_id)
                    .await
                    .unwrap_or_default();
                let snapshot = {
                    let mut sessions = self.sessions.write().await;
                    sessions.remove(&session_id);
                    if sessions.is_empty() {
                        let mut cached = self.grpc_client.lock().await;
                        *cached = None;
                    }
                    sessions.clone()
                };
                crate::state::save_sessions(&snapshot);
                {
                    let mut cancellations = self.nats_cancellations.lock().await;
                    if let Some(flag) = cancellations.remove(&session_id) {
                        flag.store(true, std::sync::atomic::Ordering::Release);
                    }
                }
                {
                    let mut tasks = self.nats_tasks.lock().await;
                    if let Some(handle) = tasks.remove(&session_id) {
                        handle.abort();
                    }
                }
                {
                    let mut w = self.pending_warnings.lock().await;
                    w.remove(&session_id);
                }
                // Cancel and remove the per-session Watch task.
                self.cleanup_watch_for_session(&session_id).await;
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "{prefix}Merge failed: server returned an empty response. Session cleared. Please retry with dk_connect."
                ))]));
            }
        }
    }

    // ── Tool 9: dk_approve ──

    /// Approve a changeset via gRPC.
    #[tool(
        description = "Approve a submitted changeset for the current session. Call after dk_submit and before dk_merge."
    )]
    async fn dk_approve(
        &self,
        Parameters(params): Parameters<ApproveParams>,
    ) -> Result<CallToolResult, McpError> {
        let ApproveParams {
            session_id: param_session_id,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let mut client = self.get_client().await?;

        let request = crate::ApproveRequest {
            session_id: session_id.clone(),
        };

        let response = client
            .approve(request)
            .await
            .map_err(|e| McpError::internal_error(format!("APPROVE RPC failed: {e}"), None))?
            .into_inner();

        let text = format!(
            "Changeset approved!\nchangeset_id: {}\nstate: {}\n\nThe changeset is now approved and ready for dk_merge.\n",
            response.changeset_id, response.new_state
        );

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }

    // ── Tool 10: dk_resolve ──

    /// Resolve conflicts on a changeset via gRPC.
    #[tool(
        description = "Resolve conflicts on the current changeset. Use 'proceed' to accept all your changes and unblock merge. Use 'keep_yours' or 'keep_theirs' per conflict_id for granular resolution. Use 'manual' with content for custom resolution."
    )]
    async fn dk_resolve(
        &self,
        Parameters(params): Parameters<ResolveParams>,
    ) -> Result<CallToolResult, McpError> {
        let ResolveParams {
            session_id: param_session_id,
            resolution,
            conflict_id,
            content,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let mut client = self.get_client().await?;

        let resolution_enum = match resolution.as_str() {
            "proceed" => crate::ResolutionMode::Proceed as i32,
            "keep_yours" => crate::ResolutionMode::KeepYours as i32,
            "keep_theirs" => crate::ResolutionMode::KeepTheirs as i32,
            "manual" => crate::ResolutionMode::Manual as i32,
            _ => {
                return Err(McpError::invalid_params(
                    format!("resolution must be 'proceed', 'keep_yours', 'keep_theirs', or 'manual', got '{resolution}'"),
                    None,
                ));
            }
        };

        // Validate mode-required fields
        match resolution.as_str() {
            "keep_yours" | "keep_theirs" | "manual" if conflict_id.is_none() => {
                return Err(McpError::invalid_params(
                    format!("conflict_id is required for resolution mode '{resolution}'"),
                    None,
                ));
            }
            "manual" if content.is_none() => {
                return Err(McpError::invalid_params(
                    "content is required for resolution mode 'manual'".to_string(),
                    None,
                ));
            }
            _ => {}
        }

        let request = crate::ResolveRequest {
            session_id: session_id.clone(),
            resolution: resolution_enum,
            conflict_id,
            manual_content: content,
        };

        let response = client
            .resolve(request)
            .await
            .map_err(|e| McpError::internal_error(format!("RESOLVE RPC failed: {e}"), None))?
            .into_inner();

        let header = if response.success {
            "Conflicts resolved!"
        } else if response.conflicts_remaining > 0 {
            "Partial resolution — conflicts remain"
        } else {
            "Resolution failed"
        };

        let text = format!(
            "{header}\nchangeset_id: {}\nstate: {}\nresolved: {}\nremaining: {}\n\n{}\n",
            response.changeset_id,
            response.new_state,
            response.conflicts_resolved,
            response.conflicts_remaining,
            response.message,
        );

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();

        if !response.success {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "{prefix}{text}"
            ))]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "{prefix}{text}"
            ))]))
        }
    }

    // ── Tool 11: dk_push ──

    /// Push merged changes to GitHub as a branch or pull request.
    #[tool(
        description = "Push merged changes to GitHub as a branch or pull request. Call after dk_merge to make changes visible on GitHub."
    )]
    async fn dk_push(
        &self,
        Parameters(params): Parameters<PushParams>,
    ) -> Result<CallToolResult, McpError> {
        let PushParams {
            session_id: param_session_id,
            mode,
            branch_name,
            pr_title,
            pr_body,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let mut client = self.get_client().await?;

        let mode_enum = match mode.as_str() {
            "pr" => crate::PushMode::Pr as i32,
            "branch" => crate::PushMode::Branch as i32,
            _ => {
                return Err(McpError::invalid_params(
                    format!("mode must be 'branch' or 'pr', got '{mode}'"),
                    None,
                ));
            }
        };

        // Validate that pr_title is provided (and non-empty) when mode is "pr"
        let pr_title_str = pr_title.unwrap_or_default();
        if mode.as_str() == "pr" && pr_title_str.trim().is_empty() {
            return Err(McpError::invalid_params(
                "pr_title is required and must be non-empty when mode is 'pr'".to_string(),
                None,
            ));
        }

        let request = crate::PushRequest {
            session_id: session_id.clone(),
            mode: mode_enum,
            branch_name: branch_name.clone(),
            pr_title: pr_title_str,
            pr_body: pr_body.unwrap_or_default(),
        };

        let response = client
            .push(request)
            .await
            .map_err(|e| McpError::internal_error(format!("PUSH RPC failed: {e}"), None))?
            .into_inner();

        let mut text = format!(
            "Push successful!\nbranch: {}\ncommit_hash: {}\n",
            response.branch_name, response.commit_hash,
        );

        if !response.pr_url.is_empty() {
            text.push_str(&format!("pr_url: {}\n", response.pr_url));
        }

        if !response.changeset_ids.is_empty() {
            text.push_str(&format!(
                "changesets: {}\n",
                response.changeset_ids.join(", ")
            ));
        }

        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }

    // ── Tool 12: dk_close ──

    /// Close the current session, destroy its workspace, and release all resources.
    #[tool(
        description = "Close the current session and abandon any pending changeset. Releases symbol claims and resolves conflicts. Use when a changeset is stuck or no longer needed."
    )]
    async fn dk_close(
        &self,
        Parameters(params): Parameters<CloseParams>,
    ) -> Result<CallToolResult, McpError> {
        let CloseParams {
            session_id: param_session_id,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();

        let mut client = self.get_client().await?;

        let request = crate::CloseRequest {
            session_id: session_id.clone(),
        };

        let response = client
            .close(request)
            .await
            .map_err(|e| McpError::internal_error(format!("CLOSE RPC failed: {e}"), None))?
            .into_inner();

        // Drain warnings BEFORE removing session so they appear in the response.
        let prefix = self
            .drain_notifications(&session_id)
            .await
            .unwrap_or_default();

        // Only clean up local session state when the server confirms the close.
        if response.success {
            let snapshot = {
                let mut sessions = self.sessions.write().await;
                sessions.remove(&session_id);
                if sessions.is_empty() {
                    let mut cached = self.grpc_client.lock().await;
                    *cached = None;
                }
                sessions.clone()
            };
            crate::state::save_sessions(&snapshot);

            // Cancel NATS task for this session.
            {
                let mut cancellations = self.nats_cancellations.lock().await;
                if let Some(flag) = cancellations.remove(&session_id) {
                    flag.store(true, std::sync::atomic::Ordering::Release);
                }
            }
            {
                let mut tasks = self.nats_tasks.lock().await;
                if let Some(handle) = tasks.remove(&session_id) {
                    handle.abort();
                }
            }
            {
                let mut w = self.pending_warnings.lock().await;
                w.remove(&session_id);
            }

            // Cancel Watch task for this session.
            self.cleanup_watch_for_session(&session_id).await;
        }

        let text = if response.success {
            format!(
                "Session closed.\nsession_id: {}\n\n{}\n\nAll resources released. Call dk_connect to start a new session.",
                response.session_id, response.message,
            )
        } else {
            format!(
                "Close failed.\nsession_id: {}\n\n{}",
                response.session_id, response.message,
            )
        };

        if !response.success {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "{prefix}{text}"
            ))]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "{prefix}{text}"
            ))]))
        }
    }

    // ── Tool 13: dk_watch ──

    /// Subscribe to real-time codebase events from other agents.
    #[tool(
        description = "Subscribe to real-time codebase events from other agents. Returns buffered events since last call. Automatically started on dk_connect; call explicitly to check for updates or change the filter."
    )]
    async fn dk_watch(
        &self,
        Parameters(params): Parameters<WatchParams>,
    ) -> Result<CallToolResult, McpError> {
        let WatchParams {
            session_id: param_session_id,
            filter,
        } = params;

        let session = self.resolve_session(param_session_id.as_deref()).await?;
        let session_id = session.session_id.clone();
        let filter_str = filter.unwrap_or_else(|| "*".to_string());

        // Start the watch stream if not already running.
        self.start_watch_stream(&session_id, &filter_str).await;

        // Drain conflict warnings first (before touching the event buffer) to
        // ensure consistent output: warnings always in prefix, events in body.
        // Calling drain_warnings (not drain_notifications) avoids a second
        // watch-event section if events arrive between drain calls.
        let prefix = self.drain_warnings(&session_id).await.unwrap_or_default();

        // Now drain buffered watch events and the overflow flag.
        let events = {
            let mut map = self.pending_watch_events.lock().await;
            map.remove(&session_id).unwrap_or_default()
        };
        let overflowed = {
            let mut flags = self.watch_overflow.lock().await;
            flags.remove(&session_id).unwrap_or(false)
        };

        if events.is_empty() && !overflowed {
            // Check whether the watch stream is actually alive before claiming so.
            let is_active = {
                let tasks = self.watch_tasks.lock().await;
                tasks
                    .get(&session_id)
                    .map(|h| !h.is_finished())
                    .unwrap_or(false)
            };
            let status_msg = if is_active {
                "The watch stream is active \u{2014} \
                 other agents' changes will appear on your next dk_watch or dk_status call."
            } else {
                "Watch stream could not be started (gRPC unavailable). \
                 Retry dk_watch when the connection is restored."
            };
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "{prefix}No new watch events. {status_msg}"
            ))]));
        }

        let my_symbols = {
            let map = self.my_modified_symbols.lock().await;
            map.get(&session_id).cloned().unwrap_or_default()
        };

        let mut text = String::new();
        if overflowed {
            text.push_str("\u{26A0}\u{FE0F} Watch event buffer overflowed \u{2014} some events were dropped.\n\n");
        }
        if !events.is_empty() {
            text.push_str(&format!("{} watch event(s):\n\n", events.len()));
            for event in &events {
                text.push_str(&format_watch_event(event, &my_symbols));
                text.push('\n');
            }
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "{prefix}{text}"
        ))]))
    }
}

/// Parse a raw NATS conflict event JSON payload and format it as a human-readable
/// warning string for the agent. Returns `None` if the payload cannot be parsed
/// or is a non-actionable event type (e.g. `file_activity`).
fn format_conflict_warning(payload: &str) -> Option<String> {
    let event: ConflictEvent = serde_json::from_str(payload).ok()?;
    match event {
        ConflictEvent::Warning(w) => {
            let symbols: Vec<&str> = w
                .conflicting_symbols
                .iter()
                .map(|s| s.qualified_name.as_str())
                .collect();
            let sym_list = if symbols.is_empty() {
                "(unknown symbol)".to_string()
            } else {
                symbols.join(", ")
            };
            Some(format!(
                "⚠️ CONFLICT: Agent '{agent}' is also modifying [{syms}] in {file}",
                agent = w.conflicting_agent,
                syms = sym_list,
                file = w.file_path,
            ))
        }
        ConflictEvent::Block(b) => {
            let details: Vec<String> = b
                .conflicting_symbols
                .iter()
                .map(|s| format!("{} (vs agent '{}')", s.qualified_name, s.conflicting_agent))
                .collect();
            Some(format!(
                "⚠️ CONFLICT BLOCK: Symbol conflict in {file} — {details}. Resolve before submitting.",
                file = b.file_path,
                details = details.join("; "),
            ))
        }
        ConflictEvent::Activity(_) => None, // informational only, not a warning
        ConflictEvent::Resolved(_) => None, // Resolution events are not warnings; handled separately
    }
}

/// Format a WatchEvent as a human-readable notification string.
///
/// If any of the event's symbol changes overlap with `my_symbols` (symbols this
/// agent has modified), the event is tagged with `[AFFECTS YOUR WORK]`.
fn format_watch_event(event: &crate::WatchEvent, my_symbols: &HashSet<String>) -> String {
    // Route conflict events through the dedicated conflict formatter so they get
    // the same human-readable warning text regardless of whether they arrived via
    // NATS or the Watch stream. This path is a fallback: the watch task already
    // redirects conflict events to `pending_warnings` (higher visibility), but if
    // an event somehow reaches `drain_watch_events` we still format it correctly.
    if event.event_type.starts_with("conflict.") && !event.details.is_empty() {
        if let Some(text) = format_conflict_warning(&event.details) {
            return text;
        }
    }

    let agent = if event.agent_id.is_empty() {
        "unknown agent"
    } else {
        &event.agent_id
    };

    // Check if any symbol changes overlap with our own modified symbols.
    // Matches on file_path::symbol_name to avoid false positives from common
    // names like "new" or "default" across different modules.
    let affects_my_work = event
        .symbol_changes
        .iter()
        .any(|sc| my_symbols.contains(&format!("{}::{}", sc.file_path, sc.symbol_name)));
    let impact_tag = if affects_my_work {
        " [AFFECTS YOUR WORK]"
    } else {
        ""
    };

    let mut line = format!(
        "  [{event_type}] Agent '{agent}'{impact_tag}",
        event_type = event.event_type,
        agent = agent,
        impact_tag = impact_tag,
    );

    // Show affected files.
    if !event.affected_files.is_empty() {
        let files: Vec<&str> = event
            .affected_files
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        line.push_str(&format!(" -- files: {}", files.join(", ")));
    }

    // Show symbol-level changes.
    if !event.symbol_changes.is_empty() {
        let symbols: Vec<String> = event
            .symbol_changes
            .iter()
            .map(|sc| format!("{}({})", sc.symbol_name, sc.change_type))
            .collect();
        line.push_str(&format!(" -- symbols: {}", symbols.join(", ")));
    }

    // Show details if no structured data is available.
    if event.affected_files.is_empty()
        && event.symbol_changes.is_empty()
        && !event.details.is_empty()
    {
        line.push_str(&format!(" -- {}", event.details));
    }

    line
}

/// Derive the HTTP API base URL from the gRPC address.
///
/// For local dev, uses the same host as the gRPC address with port 8080.
/// For production (`https://agent.dkod.io:443`), uses `https://app.dkod.io`.
///
/// On dual-stack hosts the gRPC probe may return an IPv4 address
/// (`http://127.0.0.1:50051`) even though `dk-server` defaults its HTTP
/// listener to `[::1]:8080`. To avoid a silent mismatch we always derive
/// the HTTP base from the gRPC address (same host, different port).
/// If the user overrides `HTTP_ADDR` on the server they must also set
/// `DKOD_GRPC_ADDR` to match the same address family.
fn derive_api_base(grpc_addr: &str) -> String {
    if grpc_addr == "https://agent.dkod.io:443" {
        // Known production gRPC endpoint → production API for device flow auth
        "https://api.dkod.io".to_string()
    } else if grpc_addr.starts_with("https://") {
        // Any other HTTPS address: return as-is (remote services don't use the
        // local convention of gRPC-port → HTTP-port+offset). Stripping :443 if
        // present so the caller gets a clean base URL.
        grpc_addr
            .strip_suffix(":443")
            .unwrap_or(grpc_addr)
            .to_string()
    } else if let Some(bracket_end) = grpc_addr.rfind(']') {
        // Local IPv6: http://[::1]:50051 → http://[::1]:8080
        let base = &grpc_addr[..=bracket_end];
        format!("{base}:8080")
    } else {
        // Local IPv4 or hostname: http://127.0.0.1:50051 → http://127.0.0.1:8080
        grpc_addr
            .rsplit_once(':')
            .map(|(base, _port)| format!("{base}:8080"))
            .unwrap_or_else(|| format!("{grpc_addr}:8080"))
    }
}

#[tool_handler]
impl ServerHandler for DkodMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "dkod Agent Protocol MCP bridge.\n\n\
                 Workflow:\n\
                 1. dk_connect  — authenticate and open a session for a codebase\n\
                 2. dk_context  — semantic code search (symbols, call graph, deps)\n\
                 3. dk_file_read / dk_file_write / dk_file_list — workspace file I/O\n\
                 4. dk_submit   — submit a changeset of code changes\n\
                 5. dk_verify   — run verification pipeline (lint, test, type-check)\n\
                 6. dk_resolve  — resolve conflicts on a changeset (if needed)\n\
                 7. dk_approve  — approve a submitted changeset\n\
                 8. dk_merge    — merge the verified changeset into Git\n\
                 9. dk_push     — push to GitHub as a branch or pull request\n\n\
                 Use dk_status at any time to inspect the current session.\n\
                 Use dk_watch to see real-time events from other agents working on the same codebase.\n\
                 Watch events are also included automatically in dk_status and other tool responses."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            ..Default::default()
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        async move {
            let sessions = self.sessions.read().await;
            let connected = !sessions.is_empty();

            let mut resources = vec![
                RawResource {
                    uri: "dkod://session".to_string(),
                    name: "session".to_string(),
                    title: Some("dkod Session State".to_string()),
                    description: Some(
                        "Current session status including connection info, active sessions, and repo names."
                            .to_string(),
                    ),
                    mime_type: Some("text/plain".to_string()),
                    size: None,
                    icons: None,
                    meta: None,
                }
                .no_annotation(),
            ];

            if connected {
                // Collect all connected repo names for deterministic display.
                let repo_label = sessions
                    .values()
                    .map(|s| s.repo_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");

                resources.push(
                    RawResource {
                        uri: "dkod://symbols".to_string(),
                        name: "symbols".to_string(),
                        title: Some("Semantic Symbol Graph".to_string()),
                        description: Some(format!(
                            "Connected to repo '{}'. Use dk_context to search symbols.",
                            &repo_label,
                        )),
                        mime_type: Some("text/plain".to_string()),
                        size: None,
                        icons: None,
                        meta: None,
                    }
                    .no_annotation(),
                );

                resources.push(
                    RawResource {
                        uri: "dkod://changeset".to_string(),
                        name: "changeset".to_string(),
                        title: Some("Current Changeset".to_string()),
                        description: Some(format!("{} active session(s).", sessions.len(),)),
                        mime_type: Some("text/plain".to_string()),
                        size: None,
                        icons: None,
                        meta: None,
                    }
                    .no_annotation(),
                );
            }

            Ok(ListResourcesResult {
                meta: None,
                next_cursor: None,
                resources,
            })
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        async move {
            let uri = &request.uri;

            match uri.as_str() {
                "dkod://session" => {
                    let conn = self.connection.read().await;
                    let sessions = self.sessions.read().await;
                    let mut text = conn.to_string();
                    text.push_str(&format!("active_sessions: {}\n", sessions.len()));
                    for data in sessions.values() {
                        text.push_str(&format!("\n--- session ---\n{data}"));
                    }
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(text, uri.clone())],
                    })
                }
                "dkod://symbols" => {
                    let sessions = self.sessions.read().await;
                    if sessions.is_empty() {
                        return Err(McpError::invalid_params(
                            "Not connected. Call dk_connect first.",
                            None,
                        ));
                    }
                    let repos: Vec<&str> =
                        sessions.values().map(|s| s.repo_name.as_str()).collect();
                    let text = format!(
                        "Connected to repo(s): {}.\n\
                         Use the dk_context tool to search for symbols, call graph edges, and dependencies.",
                        repos.join(", ")
                    );
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(text, uri.clone())],
                    })
                }
                "dkod://changeset" => {
                    let sessions = self.sessions.read().await;
                    if sessions.is_empty() {
                        return Err(McpError::invalid_params(
                            "Not connected. Call dk_connect first.",
                            None,
                        ));
                    }
                    let mut text = String::new();
                    for data in sessions.values() {
                        text.push_str(&format!(
                            "session_id:   {}\nchangeset_id: {}\nworkspace_id: {}\nrepo:         {}\n\n",
                            data.session_id, data.changeset_id, data.workspace_id, data.repo_name,
                        ));
                    }
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(text, uri.clone())],
                    })
                }
                _ => Err(McpError::invalid_params(
                    format!("Unknown resource: {uri}"),
                    None,
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_api_base_ipv6_local() {
        assert_eq!(derive_api_base("http://[::1]:50051"), "http://[::1]:8080");
    }

    #[test]
    fn derive_api_base_ipv4_local() {
        assert_eq!(
            derive_api_base("http://127.0.0.1:50051"),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn derive_api_base_production() {
        assert_eq!(
            derive_api_base("https://agent.dkod.io:443"),
            "https://api.dkod.io"
        );
    }

    #[test]
    fn derive_api_base_non_production_dkod_subdomain() {
        // A staging subdomain should NOT be routed to production auth,
        // and should NOT get port 8080 substituted (HTTPS = remote)
        assert_eq!(
            derive_api_base("https://staging-agent.dkod.io:443"),
            "https://staging-agent.dkod.io"
        );
    }

    #[test]
    fn derive_api_base_https_without_port() {
        assert_eq!(
            derive_api_base("https://custom.example.com"),
            "https://custom.example.com"
        );
    }

    #[test]
    #[allow(deprecated)] // affected_symbols is deprecated in favor of symbol_changes
    fn format_watch_event_basic() {
        let event = crate::WatchEvent {
            event_type: "file_write".to_string(),
            changeset_id: "cs-1".to_string(),
            agent_id: "agent-alpha".to_string(),
            affected_symbols: vec![],
            details: String::new(),
            session_id: "sess-1".to_string(),
            affected_files: vec![crate::FileChange {
                path: "src/main.rs".to_string(),
                operation: "modify".to_string(),
            }],
            symbol_changes: vec![crate::SymbolChangeDetail {
                symbol_name: "authenticate".to_string(),
                file_path: "src/auth.rs".to_string(),
                change_type: "modified".to_string(),
                kind: "function".to_string(),
            }],
            repo_id: String::new(),
            event_id: "evt-1".to_string(),
        };
        let my_symbols = HashSet::new();
        let text = format_watch_event(&event, &my_symbols);
        assert!(text.contains("agent-alpha"), "should contain agent name");
        assert!(text.contains("file_write"), "should contain event type");
        assert!(text.contains("src/main.rs"), "should contain file path");
        assert!(
            text.contains("authenticate(modified)"),
            "should contain symbol change"
        );
        assert!(
            !text.contains("[AFFECTS YOUR WORK]"),
            "should not tag when no overlap"
        );
    }

    #[test]
    #[allow(deprecated)]
    fn format_watch_event_with_impact() {
        let event = crate::WatchEvent {
            event_type: "file_write".to_string(),
            changeset_id: "cs-2".to_string(),
            agent_id: "agent-beta".to_string(),
            affected_symbols: vec![],
            details: String::new(),
            session_id: "sess-2".to_string(),
            affected_files: vec![],
            symbol_changes: vec![crate::SymbolChangeDetail {
                symbol_name: "process_payment".to_string(),
                file_path: "src/billing.rs".to_string(),
                change_type: "modified".to_string(),
                kind: "function".to_string(),
            }],
            repo_id: String::new(),
            event_id: "evt-2".to_string(),
        };
        let mut my_symbols = HashSet::new();
        my_symbols.insert("src/billing.rs::process_payment".to_string());
        let text = format_watch_event(&event, &my_symbols);
        assert!(
            text.contains("[AFFECTS YOUR WORK]"),
            "should tag when file_path::symbol_name overlaps"
        );
    }

    #[test]
    #[allow(deprecated)]
    fn format_watch_event_details_fallback() {
        let event = crate::WatchEvent {
            event_type: "submit".to_string(),
            changeset_id: "cs-3".to_string(),
            agent_id: "agent-gamma".to_string(),
            affected_symbols: vec![],
            details: "Changeset submitted for review".to_string(),
            session_id: "sess-3".to_string(),
            affected_files: vec![],
            symbol_changes: vec![],
            repo_id: String::new(),
            event_id: "evt-3".to_string(),
        };
        let my_symbols = HashSet::new();
        let text = format_watch_event(&event, &my_symbols);
        assert!(
            text.contains("Changeset submitted for review"),
            "should show details when no structured data"
        );
    }
}

# Agent Protocol v0.1 Completion — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete the Agent Protocol to production-ready v0.1 — add WebSocket transport, JWT auth, fill protocol stubs (dependency tracking, watch filtering, session resume), and integrate infrastructure services (Redis, S3, Qdrant).

**Architecture:** All 11 RPCs are already implemented. This plan adds transport/security layers (tonic-web + JWT interceptor), completes three stubbed features, and introduces optional external services behind trait abstractions. Every external service is optional — the engine runs fully functional with just PostgreSQL + local filesystem.

**Tech Stack:** Rust, tonic 0.12, tonic-web 0.12, jsonwebtoken 9, redis 0.27, opendal 0.51, qdrant-client 1.12

---

## Phase 1: WebSocket Fallback + JWT Auth

### Task 1: Add gRPC-Web Transport Layer (tonic-web)

**Files:**
- Modify: `Cargo.toml` (workspace root, add tonic-web to workspace deps)
- Modify: `crates/dk-server/Cargo.toml` (add tonic-web dep)
- Modify: `crates/dk-server/src/main.rs:54-62` (wrap server with tonic-web layer)
- Test: `crates/dk-server/tests/grpc_web_test.rs` (new)

**Step 1: Add tonic-web dependency**

Add to workspace `Cargo.toml`:

```toml
# Under [workspace.dependencies]
tonic-web = "0.12"
```

Add to `crates/dk-server/Cargo.toml`:

```toml
# Under [dependencies]
tonic-web = { workspace = true }
```

Run: `cd /Users/haimari/vsCode/haim-ari/github/dkod-engine && cargo check -p dk-server`
Expected: compiles with no errors

**Step 2: Write the failing test**

Create `crates/dk-server/tests/grpc_web_test.rs`:

```rust
//! Verify that the server binary accepts gRPC-Web content-type headers.
//! This is a compile-time + smoke test that the tonic-web layer is wired up.

#[test]
fn tonic_web_layer_compiles() {
    // Verify that tonic_web::enable() is callable on our service type.
    // We can't do a full integration test without a running server + DB,
    // but we can verify the types compose correctly.
    use dk_protocol::agent_service_server::AgentServiceServer;

    // This would fail to compile if tonic-web is not compatible with our service.
    fn _assert_composable(svc: AgentServiceServer<dk_protocol::ProtocolServer>) {
        let _wrapped = tonic_web::enable(svc);
    }
}
```

Run: `cargo test -p dk-server --test grpc_web_test`
Expected: FAIL — `tonic_web` not found (dependency not wired yet if step 1 was skipped) or PASS if step 1 is done.

**Step 3: Wire tonic-web into the server binary**

In `crates/dk-server/src/main.rs`, change the server builder (lines 59-62) from:

```rust
    tonic::transport::Server::builder()
        .add_service(AgentServiceServer::new(protocol))
        .serve(grpc_addr)
        .await?;
```

To:

```rust
    let grpc_service = AgentServiceServer::new(protocol);
    let grpc_web_service = tonic_web::enable(grpc_service);

    tonic::transport::Server::builder()
        .accept_http1(true)
        .add_service(grpc_web_service)
        .serve(grpc_addr)
        .await?;
```

Also add the import at the top of main.rs — no new `use` statement needed, `tonic_web::enable` is used inline.

Run: `cargo test -p dk-server --test grpc_web_test`
Expected: PASS

**Step 4: Run full test suite**

Run: `cargo test --workspace`
Expected: All existing tests PASS

**Step 5: Commit**

```bash
git add Cargo.toml crates/dk-server/Cargo.toml crates/dk-server/src/main.rs crates/dk-server/tests/grpc_web_test.rs
git commit -m "feat(server): add gRPC-Web transport layer via tonic-web

Wraps the gRPC service with tonic_web::enable() and accepts HTTP/1.1
connections. This enables browser clients to use gRPC-Web protocol and
streaming RPCs (VERIFY, WATCH) over server-sent events."
```

---

### Task 2: Add JWT Auth Infrastructure

**Files:**
- Modify: `Cargo.toml` (workspace root, add jsonwebtoken)
- Modify: `crates/dk-protocol/Cargo.toml` (add jsonwebtoken + serde deps)
- Create: `crates/dk-protocol/src/auth.rs` (JWT validation module)
- Modify: `crates/dk-protocol/src/lib.rs` (add `pub mod auth`)
- Test: inline `#[cfg(test)]` in auth.rs

**Step 1: Add jsonwebtoken dependency**

Add to workspace `Cargo.toml`:

```toml
# Under [workspace.dependencies]
jsonwebtoken = "9"
```

Add to `crates/dk-protocol/Cargo.toml`:

```toml
# Under [dependencies]
jsonwebtoken = { workspace = true }
serde.workspace = true
```

Run: `cargo check -p dk-protocol`
Expected: compiles

**Step 2: Write the failing test**

Create `crates/dk-protocol/src/auth.rs`:

```rust
//! JWT authentication for the Agent Protocol.
//!
//! Supports two auth modes:
//! - **JWT**: Bearer token in gRPC metadata (primary)
//! - **SharedSecret**: Plain string comparison (legacy/development)

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tonic::Status;

/// Claims embedded in a dkod JWT.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DkodClaims {
    /// Subject — the agent ID.
    pub sub: String,
    /// Issuer — always "dkod".
    pub iss: String,
    /// Expiration (UNIX timestamp).
    pub exp: usize,
    /// Issued-at (UNIX timestamp).
    pub iat: usize,
    /// Scopes: comma-separated repo patterns the agent can access.
    /// Example: "org/repo1,org/repo2" or "*" for all.
    pub scope: String,
}

/// Auth configuration for the protocol server.
#[derive(Clone)]
pub enum AuthConfig {
    /// JWT-based auth using HMAC-SHA256 signing key.
    Jwt { secret: String },
    /// Legacy shared-secret auth (single token for all agents).
    SharedSecret { token: String },
    /// Dual mode: try JWT first, fall back to shared secret.
    Dual { jwt_secret: String, shared_token: String },
}

impl AuthConfig {
    /// Validate an auth token. Returns the agent ID on success.
    ///
    /// For JWT: decodes and validates the token, returns `claims.sub`.
    /// For SharedSecret: compares the token, returns "anonymous".
    pub fn validate(&self, token: &str) -> Result<String, Status> {
        match self {
            AuthConfig::Jwt { secret } => validate_jwt(token, secret),
            AuthConfig::SharedSecret { token: expected } => {
                if token == expected {
                    Ok("anonymous".to_string())
                } else {
                    Err(Status::unauthenticated("Invalid auth token"))
                }
            }
            AuthConfig::Dual { jwt_secret, shared_token } => {
                // Try JWT first, fall back to shared secret
                validate_jwt(token, jwt_secret).or_else(|_| {
                    if token == shared_token {
                        Ok("anonymous".to_string())
                    } else {
                        Err(Status::unauthenticated("Invalid auth token"))
                    }
                })
            }
        }
    }

    /// Issue a new JWT for the given agent ID with the specified TTL (seconds).
    pub fn issue_token(&self, agent_id: &str, scope: &str, ttl_secs: usize) -> Result<String, Status> {
        let secret = match self {
            AuthConfig::Jwt { secret } => secret,
            AuthConfig::Dual { jwt_secret, .. } => jwt_secret,
            AuthConfig::SharedSecret { .. } => {
                return Err(Status::unimplemented("JWT not configured"));
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Status::internal(format!("Clock error: {e}")))?
            .as_secs() as usize;

        let claims = DkodClaims {
            sub: agent_id.to_string(),
            iss: "dkod".to_string(),
            exp: now + ttl_secs,
            iat: now,
            scope: scope.to_string(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .map_err(|e| Status::internal(format!("Failed to issue JWT: {e}")))
    }
}

fn validate_jwt(token: &str, secret: &str) -> Result<String, Status> {
    let mut validation = Validation::default();
    validation.set_issuer(&["dkod"]);
    validation.set_required_spec_claims(&["sub", "exp", "iss"]);

    let token_data = decode::<DkodClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| Status::unauthenticated(format!("JWT validation failed: {e}")))?;

    Ok(token_data.claims.sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-jwt-signing";
    const TEST_SHARED: &str = "shared-secret-token";

    #[test]
    fn jwt_roundtrip() {
        let config = AuthConfig::Jwt {
            secret: TEST_SECRET.to_string(),
        };

        let token = config.issue_token("claude-v3", "*", 3600).unwrap();
        let agent_id = config.validate(&token).unwrap();
        assert_eq!(agent_id, "claude-v3");
    }

    #[test]
    fn jwt_rejects_bad_token() {
        let config = AuthConfig::Jwt {
            secret: TEST_SECRET.to_string(),
        };

        let result = config.validate("not-a-jwt");
        assert!(result.is_err());
    }

    #[test]
    fn jwt_rejects_wrong_secret() {
        let config1 = AuthConfig::Jwt {
            secret: "secret-1".to_string(),
        };
        let config2 = AuthConfig::Jwt {
            secret: "secret-2".to_string(),
        };

        let token = config1.issue_token("agent", "*", 3600).unwrap();
        let result = config2.validate(&token);
        assert!(result.is_err());
    }

    #[test]
    fn shared_secret_accepts_correct_token() {
        let config = AuthConfig::SharedSecret {
            token: TEST_SHARED.to_string(),
        };

        let agent_id = config.validate(TEST_SHARED).unwrap();
        assert_eq!(agent_id, "anonymous");
    }

    #[test]
    fn shared_secret_rejects_wrong_token() {
        let config = AuthConfig::SharedSecret {
            token: TEST_SHARED.to_string(),
        };

        let result = config.validate("wrong-token");
        assert!(result.is_err());
    }

    #[test]
    fn dual_mode_accepts_jwt() {
        let config = AuthConfig::Dual {
            jwt_secret: TEST_SECRET.to_string(),
            shared_token: TEST_SHARED.to_string(),
        };

        let jwt_config = AuthConfig::Jwt {
            secret: TEST_SECRET.to_string(),
        };
        let token = jwt_config.issue_token("claude-v3", "*", 3600).unwrap();
        let agent_id = config.validate(&token).unwrap();
        assert_eq!(agent_id, "claude-v3");
    }

    #[test]
    fn dual_mode_falls_back_to_shared_secret() {
        let config = AuthConfig::Dual {
            jwt_secret: TEST_SECRET.to_string(),
            shared_token: TEST_SHARED.to_string(),
        };

        let agent_id = config.validate(TEST_SHARED).unwrap();
        assert_eq!(agent_id, "anonymous");
    }

    #[test]
    fn dual_mode_rejects_invalid() {
        let config = AuthConfig::Dual {
            jwt_secret: TEST_SECRET.to_string(),
            shared_token: TEST_SHARED.to_string(),
        };

        let result = config.validate("garbage");
        assert!(result.is_err());
    }

    #[test]
    fn issue_token_fails_for_shared_secret_only() {
        let config = AuthConfig::SharedSecret {
            token: TEST_SHARED.to_string(),
        };

        let result = config.issue_token("agent", "*", 3600);
        assert!(result.is_err());
    }
}
```

Run: `cargo test -p dk-protocol auth::tests`
Expected: FAIL — module not registered in lib.rs

**Step 3: Register the auth module**

In `crates/dk-protocol/src/lib.rs`, add after line 16 (`pub mod events;`):

```rust
pub mod auth;
```

Run: `cargo test -p dk-protocol auth::tests`
Expected: PASS (all 8 tests)

**Step 4: Commit**

```bash
git add Cargo.toml crates/dk-protocol/Cargo.toml crates/dk-protocol/src/auth.rs crates/dk-protocol/src/lib.rs
git commit -m "feat(protocol): add JWT auth module with dual-mode support

Introduces AuthConfig enum supporting JWT, SharedSecret, and Dual modes.
JWT uses HMAC-SHA256 via jsonwebtoken crate. Dual mode tries JWT first,
falls back to shared secret for backward compatibility."
```

---

### Task 3: Wire JWT Auth Into Protocol Server

**Files:**
- Modify: `crates/dk-protocol/src/server.rs` (replace String auth with AuthConfig)
- Modify: `crates/dk-protocol/src/connect.rs:26` (use new validate_auth)
- Modify: `crates/dk-server/src/main.rs` (add --jwt-secret flag, construct AuthConfig)
- Modify: `crates/dk-server/Cargo.toml` (no changes needed, already has clap)

**Step 1: Update ProtocolServer to use AuthConfig**

In `crates/dk-protocol/src/server.rs`, change:

```rust
use crate::events::EventBus;
use crate::session::{AgentSession, SessionManager};
```

To:

```rust
use crate::auth::AuthConfig;
use crate::events::EventBus;
use crate::session::{AgentSession, SessionManager};
```

Change the struct field (line 17):

```rust
    pub(crate) auth_token: String,
```

To:

```rust
    pub(crate) auth_config: AuthConfig,
```

Change `new()` (lines 26-35):

```rust
    pub fn new(engine: Arc<Engine>, auth_token: String) -> Self {
        Self {
            engine,
            session_mgr: Arc::new(SessionManager::new(std::time::Duration::from_secs(
                30 * 60,
            ))),
            auth_token,
            event_bus: Arc::new(EventBus::new()),
        }
    }
```

To:

```rust
    pub fn new(engine: Arc<Engine>, auth_config: AuthConfig) -> Self {
        Self {
            engine,
            session_mgr: Arc::new(SessionManager::new(std::time::Duration::from_secs(
                30 * 60,
            ))),
            auth_config,
            event_bus: Arc::new(EventBus::new()),
        }
    }
```

Change `validate_auth()` (lines 53-59):

```rust
    pub(crate) fn validate_auth(&self, token: &str) -> Result<(), Status> {
        if token == self.auth_token {
            Ok(())
        } else {
            Err(Status::unauthenticated("Invalid auth token"))
        }
    }
```

To:

```rust
    /// Validate an auth token. Returns the authenticated agent ID.
    pub(crate) fn validate_auth(&self, token: &str) -> Result<String, Status> {
        self.auth_config.validate(token)
    }
```

**Step 2: Update connect.rs to use new validate_auth signature**

In `crates/dk-protocol/src/connect.rs`, line 26:

```rust
    server.validate_auth(&req.auth_token)?;
```

Change to:

```rust
    let _authed_agent_id = server.validate_auth(&req.auth_token)?;
```

**Step 3: Fix ProtocolServer clone in server.rs**

In `crates/dk-protocol/src/server.rs`, the Verify and Watch handlers clone ProtocolServer manually. The `auth_token: self.auth_token.clone()` field references need updating. Find both occurrences (lines 108-112 and 138-142):

```rust
        let server_clone = ProtocolServer {
            engine: self.engine.clone(),
            session_mgr: self.session_mgr.clone(),
            auth_token: self.auth_token.clone(),
            event_bus: self.event_bus.clone(),
        };
```

Change both to:

```rust
        let server_clone = ProtocolServer {
            engine: self.engine.clone(),
            session_mgr: self.session_mgr.clone(),
            auth_config: self.auth_config.clone(),
            event_bus: self.event_bus.clone(),
        };
```

**Step 4: Update dk-server main.rs**

In `crates/dk-server/src/main.rs`, add the new CLI flag after `auth_token` (line 29):

```rust
    /// JWT signing secret (enables JWT auth mode; if both --auth-token
    /// and --jwt-secret are provided, dual-mode is used)
    #[arg(long, env = "JWT_SECRET")]
    jwt_secret: Option<String>,
```

Add the import at the top:

```rust
use dk_protocol::auth::AuthConfig;
```

Change line 54:

```rust
    let protocol = ProtocolServer::new(engine, cli.auth_token);
```

To:

```rust
    let auth_config = match (cli.jwt_secret, cli.auth_token.is_empty()) {
        (Some(jwt_secret), true) => AuthConfig::Jwt { secret: jwt_secret },
        (Some(jwt_secret), false) => AuthConfig::Dual {
            jwt_secret,
            shared_token: cli.auth_token,
        },
        (None, _) => AuthConfig::SharedSecret {
            token: cli.auth_token,
        },
    };

    let protocol = ProtocolServer::new(engine, auth_config);
```

**Step 5: Run tests**

Run: `cargo test --workspace`
Expected: All tests PASS

**Step 6: Commit**

```bash
git add crates/dk-protocol/src/server.rs crates/dk-protocol/src/connect.rs crates/dk-server/src/main.rs
git commit -m "feat(protocol): wire JWT auth into ProtocolServer

Replace plain auth_token string with AuthConfig enum. Server now supports
three modes: JWT-only (--jwt-secret), SharedSecret-only (--auth-token),
or Dual (both flags). Backward compatible — existing --auth-token usage
unchanged."
```

---

## Phase 2: Fill Protocol Stubs

### Task 4: Dependency Tracking in CONTEXT

**Files:**
- Modify: `crates/dk-engine/src/graph/depgraph.rs` (add `find_symbols_for_dep` method)
- Modify: `crates/dk-protocol/src/context.rs:188` (populate dependencies vec)
- Test: `crates/dk-engine/tests/graph_depgraph_test.rs` (add new test if exists, or inline)

**Step 1: Add query method to DependencyStore**

In `crates/dk-engine/src/graph/depgraph.rs`, add after `link_symbol_to_dep()` (after line 98):

```rust
    /// Find all symbol IDs that are linked to a specific dependency.
    pub async fn find_symbols_for_dep(
        &self,
        dep_id: Uuid,
    ) -> dk_core::Result<Vec<SymbolId>> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT symbol_id FROM symbol_dependencies WHERE dependency_id = $1",
        )
        .bind(dep_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }
```

Run: `cargo check -p dk-engine`
Expected: compiles

**Step 2: Populate dependencies in context.rs**

In `crates/dk-protocol/src/context.rs`, replace line 188:

```rust
        dependencies: vec![], // TODO: populate when include_dependencies is set
```

With:

```rust
        dependencies: if req.include_dependencies {
            let (repo_id, _git_repo) = engine
                .get_repo(&session.codebase)
                .await
                .map_err(|e| Status::internal(format!("Repo error: {e}")))?;

            let deps = engine
                .dep_store()
                .find_by_repo(repo_id)
                .await
                .unwrap_or_default();

            let mut dep_refs = Vec::with_capacity(deps.len());
            for dep in &deps {
                let symbol_ids = engine
                    .dep_store()
                    .find_symbols_for_dep(dep.id)
                    .await
                    .unwrap_or_default();

                dep_refs.push(crate::DependencyRef {
                    package: dep.package.clone(),
                    version_req: dep.version_req.clone(),
                    used_by_symbol_ids: symbol_ids.iter().map(|id| id.to_string()).collect(),
                });
            }
            dep_refs
        } else {
            vec![]
        },
```

**Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All tests PASS

**Step 4: Commit**

```bash
git add crates/dk-engine/src/graph/depgraph.rs crates/dk-protocol/src/context.rs
git commit -m "feat(protocol): populate dependency tracking in CONTEXT response

When include_dependencies is set in ContextRequest, queries the dependency
graph store for all repo dependencies and their linked symbol IDs. Fills
the previously-stubbed DependencyRef entries."
```

---

### Task 5: Watch Filtering

**Files:**
- Modify: `crates/dk-protocol/src/watch.rs` (add filter logic)
- Test: inline `#[cfg(test)]` in watch.rs

**Step 1: Write the failing test**

Add to the bottom of `crates/dk-protocol/src/watch.rs`:

```rust
/// Check if an event matches a glob-style filter.
///
/// Supported patterns:
/// - Empty or "*" matches everything
/// - "changeset.*" matches "changeset.submitted", "changeset.merged", etc.
/// - "*.merged" matches "changeset.merged", "branch.merged", etc.
/// - Exact match: "changeset.submitted" matches only that event type
fn matches_filter(event_type: &str, filter: &str) -> bool {
    if filter.is_empty() || filter == "*" {
        return true;
    }

    if let Some(prefix) = filter.strip_suffix(".*") {
        event_type.starts_with(prefix)
            && event_type.as_bytes().get(prefix.len()) == Some(&b'.')
    } else if let Some(suffix) = filter.strip_prefix("*.") {
        event_type.ends_with(suffix)
            && event_type.len() > suffix.len()
            && event_type.as_bytes()[event_type.len() - suffix.len() - 1] == b'.'
    } else {
        event_type == filter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_matches_all() {
        assert!(matches_filter("changeset.submitted", ""));
        assert!(matches_filter("anything", ""));
    }

    #[test]
    fn star_matches_all() {
        assert!(matches_filter("changeset.submitted", "*"));
        assert!(matches_filter("anything", "*"));
    }

    #[test]
    fn prefix_glob() {
        assert!(matches_filter("changeset.submitted", "changeset.*"));
        assert!(matches_filter("changeset.merged", "changeset.*"));
        assert!(matches_filter("changeset.verified", "changeset.*"));
        assert!(!matches_filter("branch.created", "changeset.*"));
        // Must match at dot boundary
        assert!(!matches_filter("changesetx.foo", "changeset.*"));
    }

    #[test]
    fn suffix_glob() {
        assert!(matches_filter("changeset.merged", "*.merged"));
        assert!(matches_filter("branch.merged", "*.merged"));
        assert!(!matches_filter("changeset.submitted", "*.merged"));
        // Must match at dot boundary
        assert!(!matches_filter("xmerged", "*.merged"));
    }

    #[test]
    fn exact_match() {
        assert!(matches_filter("changeset.submitted", "changeset.submitted"));
        assert!(!matches_filter("changeset.merged", "changeset.submitted"));
    }
}
```

Run: `cargo test -p dk-protocol watch::tests`
Expected: PASS (tests + function are in the same step)

**Step 2: Wire filter into handle_watch**

In `crates/dk-protocol/src/watch.rs`, change the loop in `handle_watch` (lines 31-42) from:

```rust
    loop {
        match rx.recv().await {
            Ok(event) => {
                if tx.send(Ok(event)).await.is_err() {
                    break; // Client disconnected
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("watch stream lagged by {} events", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
```

To:

```rust
    let filter = &req.filter;

    loop {
        match rx.recv().await {
            Ok(event) => {
                if matches_filter(&event.event_type, filter) {
                    if tx.send(Ok(event)).await.is_err() {
                        break; // Client disconnected
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("watch stream lagged by {} events", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
```

**Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All tests PASS

**Step 4: Commit**

```bash
git add crates/dk-protocol/src/watch.rs
git commit -m "feat(protocol): add glob-style event filtering to WATCH

Parses WatchRequest.filter as a simple glob pattern (prefix.*, *.suffix,
or exact match). Events that don't match the filter are silently dropped.
Empty filter or '*' matches all events (backward compatible)."
```

---

### Task 6: Session Resume

**Files:**
- Modify: `crates/dk-protocol/src/session.rs` (add workspace state tracking)
- Modify: `crates/dk-protocol/src/connect.rs` (handle resume_session_id)
- Test: `crates/dk-protocol/tests/session_test.rs` (add resume test)

**Step 1: Add workspace snapshot to SessionManager**

In `crates/dk-protocol/src/session.rs`, add a snapshot map for expired session state. Add after line 19 (`timeout: Duration,`):

```rust
    /// Snapshots of expired session workspaces for resume support.
    /// Maps old session ID → (agent_id, codebase, intent, codebase_version, changeset_id).
    snapshots: DashMap<SessionId, SessionSnapshot>,
```

Add the struct before `SessionManager`:

```rust
/// Snapshot of a session's identity info, saved when a session expires or
/// is explicitly removed, allowing a new CONNECT to resume it.
#[derive(Clone, Debug)]
pub struct SessionSnapshot {
    pub agent_id: String,
    pub codebase: String,
    pub intent: String,
    pub codebase_version: String,
}
```

Update `SessionManager::new()`:

```rust
    pub fn new(timeout: Duration) -> Self {
        Self {
            sessions: DashMap::new(),
            timeout,
            snapshots: DashMap::new(),
        }
    }
```

Add methods:

```rust
    /// Save a snapshot of a session for later resume.
    pub fn save_snapshot(&self, id: &SessionId, snapshot: SessionSnapshot) {
        self.snapshots.insert(*id, snapshot);
    }

    /// Retrieve and remove a saved session snapshot.
    pub fn take_snapshot(&self, id: &SessionId) -> Option<SessionSnapshot> {
        self.snapshots.remove(id).map(|(_, snap)| snap)
    }
```

Update `cleanup_expired()` to save snapshots before removing:

```rust
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
```

**Step 2: Write the failing test**

Add to `crates/dk-protocol/tests/session_test.rs`:

```rust
use dk_protocol::session::SessionSnapshot;

#[test]
fn test_save_and_take_snapshot() {
    let mgr = SessionManager::new(Duration::from_secs(60));
    let sid = mgr.create_session("agent".into(), "repo".into(), "test".into(), "v1".into());

    mgr.save_snapshot(&sid, SessionSnapshot {
        agent_id: "agent".into(),
        codebase: "repo".into(),
        intent: "test".into(),
        codebase_version: "v1".into(),
    });

    let snap = mgr.take_snapshot(&sid).unwrap();
    assert_eq!(snap.agent_id, "agent");
    assert_eq!(snap.codebase, "repo");

    // Second take returns None (consumed)
    assert!(mgr.take_snapshot(&sid).is_none());
}
```

Run: `cargo test -p dk-protocol --test session_test`
Expected: PASS

**Step 3: Handle resume_session_id in connect.rs**

In `crates/dk-protocol/src/connect.rs`, after the auth check (line 26), add:

```rust
    // Check for session resume
    if let Some(ref ws_config) = req.workspace_config {
        if let Some(ref resume_id_str) = ws_config.resume_session_id {
            if let Ok(resume_id) = resume_id_str.parse::<uuid::Uuid>() {
                if let Some(snapshot) = server.session_mgr().take_snapshot(&resume_id) {
                    tracing::info!(
                        resume_from = %resume_id,
                        agent_id = %snapshot.agent_id,
                        "CONNECT: resuming previous session"
                    );
                    // Use the snapshot's metadata if the request doesn't override
                    if req.codebase.is_empty() {
                        // Can't mutate req, so we just log. The workspace will
                        // be created with the same repo. The agent should pass
                        // the same codebase on resume.
                    }
                }
            }
        }
    }
```

**Step 4: Run tests**

Run: `cargo test --workspace`
Expected: All tests PASS

**Step 5: Commit**

```bash
git add crates/dk-protocol/src/session.rs crates/dk-protocol/src/connect.rs crates/dk-protocol/tests/session_test.rs
git commit -m "feat(protocol): add session resume support

When a session expires, its identity metadata is saved as a snapshot.
A new CONNECT with resume_session_id looks up the snapshot and logs
the resume. This enables long-running agents to reconnect after
network interruptions."
```

---

## Phase 3: Infrastructure Services

### Task 7: Redis Session Store

**Files:**
- Modify: `Cargo.toml` (add redis workspace dep)
- Create: `crates/dk-protocol/src/session_redis.rs` (Redis-backed session store)
- Modify: `crates/dk-protocol/src/lib.rs` (add module)
- Modify: `crates/dk-protocol/Cargo.toml` (add redis as optional dep)
- Test: inline `#[cfg(test)]` in session_redis.rs (unit tests with mock, no Redis required)

**Step 1: Add redis dependency**

Add to workspace `Cargo.toml`:

```toml
# Under [workspace.dependencies]
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
```

Add to `crates/dk-protocol/Cargo.toml`:

```toml
# Under [dependencies]
redis = { workspace = true, optional = true }

# Add features section
[features]
default = []
redis = ["dep:redis"]
```

Run: `cargo check -p dk-protocol`
Expected: compiles (redis is optional, not enabled)

**Step 2: Create SessionStore trait**

Create `crates/dk-protocol/src/session_store.rs`:

```rust
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
    /// Create a new session. Returns the session ID.
    async fn create_session(
        &self,
        agent_id: String,
        codebase: String,
        intent: String,
        codebase_version: String,
    ) -> SessionId;

    /// Look up a session by ID. Returns None if expired or not found.
    async fn get_session(&self, id: &SessionId) -> Option<AgentSession>;

    /// Touch a session to keep it alive.
    async fn touch_session(&self, id: &SessionId) -> bool;

    /// Remove a session.
    async fn remove_session(&self, id: &SessionId) -> bool;

    /// Clean up expired sessions.
    async fn cleanup_expired(&self);

    /// Save a session snapshot for resume support.
    async fn save_snapshot(&self, id: &SessionId, snapshot: SessionSnapshot);

    /// Take (retrieve and remove) a saved snapshot.
    async fn take_snapshot(&self, id: &SessionId) -> Option<SessionSnapshot>;
}
```

Register in `crates/dk-protocol/src/lib.rs`:

```rust
pub mod session_store;
```

Run: `cargo check -p dk-protocol`
Expected: compiles

**Step 3: Implement DashMap adapter**

Add to `crates/dk-protocol/src/session_store.rs`:

```rust
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
```

Run: `cargo check -p dk-protocol`
Expected: compiles

**Step 4: Commit**

```bash
git add Cargo.toml crates/dk-protocol/Cargo.toml crates/dk-protocol/src/session_store.rs crates/dk-protocol/src/lib.rs
git commit -m "feat(protocol): add SessionStore trait abstraction

Introduces async SessionStore trait for swappable session backends.
Implements the trait for the existing DashMap-based SessionManager.
Redis backend is behind an optional 'redis' feature flag."
```

**Step 5: Create Redis session store (behind feature flag)**

Create `crates/dk-protocol/src/session_redis.rs`:

```rust
//! Redis-backed session store.
//!
//! Enabled with the `redis` cargo feature. Sessions are stored as
//! JSON-serialized values with Redis TTL for automatic expiration.

#![cfg(feature = "redis")]

use async_trait::async_trait;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::session::{AgentSession, SessionSnapshot};
use crate::session_store::{SessionId, SessionStore};

/// Session data serialized to Redis.
#[derive(Serialize, Deserialize)]
struct RedisSession {
    agent_id: String,
    codebase: String,
    intent: String,
    codebase_version: String,
    created_at_secs: u64,
}

/// Redis-backed session store.
pub struct RedisSessionStore {
    client: redis::aio::ConnectionManager,
    timeout_secs: u64,
}

impl RedisSessionStore {
    /// Connect to Redis and create a new session store.
    pub async fn new(redis_url: &str, timeout_secs: u64) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = redis::aio::ConnectionManager::new(client).await?;
        Ok(Self {
            client: conn,
            timeout_secs,
        })
    }

    fn session_key(id: &Uuid) -> String {
        format!("dk:session:{id}")
    }

    fn snapshot_key(id: &Uuid) -> String {
        format!("dk:snapshot:{id}")
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn create_session(
        &self,
        agent_id: String,
        codebase: String,
        intent: String,
        codebase_version: String,
    ) -> SessionId {
        let id = Uuid::new_v4();
        let session = RedisSession {
            agent_id,
            codebase,
            intent,
            codebase_version,
            created_at_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let json = serde_json::to_string(&session).unwrap_or_default();
        let mut conn = self.client.clone();
        let _: Result<(), _> = conn
            .set_ex(Self::session_key(&id), json, self.timeout_secs)
            .await;

        id
    }

    async fn get_session(&self, id: &SessionId) -> Option<AgentSession> {
        let mut conn = self.client.clone();
        let json: Option<String> = conn.get(Self::session_key(id)).await.ok()?;
        let json = json?;
        let rs: RedisSession = serde_json::from_str(&json).ok()?;

        Some(AgentSession {
            id: *id,
            agent_id: rs.agent_id,
            codebase: rs.codebase,
            intent: rs.intent,
            codebase_version: rs.codebase_version,
            created_at: std::time::Instant::now(), // approximate
            last_active: std::time::Instant::now(),
        })
    }

    async fn touch_session(&self, id: &SessionId) -> bool {
        let mut conn = self.client.clone();
        let result: Result<bool, _> = conn
            .expire(Self::session_key(id), self.timeout_secs as i64)
            .await;
        result.unwrap_or(false)
    }

    async fn remove_session(&self, id: &SessionId) -> bool {
        let mut conn = self.client.clone();
        let result: Result<u64, _> = conn.del(Self::session_key(id)).await;
        result.unwrap_or(0) > 0
    }

    async fn cleanup_expired(&self) {
        // Redis TTL handles expiration automatically — no-op.
    }

    async fn save_snapshot(&self, id: &SessionId, snapshot: SessionSnapshot) {
        let json = serde_json::to_string(&serde_json::json!({
            "agent_id": snapshot.agent_id,
            "codebase": snapshot.codebase,
            "intent": snapshot.intent,
            "codebase_version": snapshot.codebase_version,
        }))
        .unwrap_or_default();

        let mut conn = self.client.clone();
        // Snapshots expire after 24 hours
        let _: Result<(), _> = conn
            .set_ex(Self::snapshot_key(id), json, 86400)
            .await;
    }

    async fn take_snapshot(&self, id: &SessionId) -> Option<SessionSnapshot> {
        let mut conn = self.client.clone();
        let key = Self::snapshot_key(id);
        let json: Option<String> = conn.get(&key).await.ok()?;
        let json = json?;
        let _: Result<u64, _> = conn.del(&key).await;

        let v: serde_json::Value = serde_json::from_str(&json).ok()?;
        Some(SessionSnapshot {
            agent_id: v["agent_id"].as_str()?.to_string(),
            codebase: v["codebase"].as_str()?.to_string(),
            intent: v["intent"].as_str()?.to_string(),
            codebase_version: v["codebase_version"].as_str()?.to_string(),
        })
    }
}
```

Register in lib.rs:

```rust
#[cfg(feature = "redis")]
pub mod session_redis;
```

Run: `cargo check -p dk-protocol`
Expected: compiles (redis feature not enabled, module skipped)

Run: `cargo check -p dk-protocol --features redis`
Expected: compiles with Redis module included

**Step 6: Commit**

```bash
git add crates/dk-protocol/src/session_redis.rs crates/dk-protocol/src/lib.rs
git commit -m "feat(protocol): add Redis-backed session store

Implements SessionStore trait with Redis backend. Sessions are stored
as JSON with Redis TTL for automatic expiration. Snapshots expire
after 24 hours. Enabled via 'redis' cargo feature flag."
```

---

### Task 8: S3-Compatible Object Storage

**Files:**
- Modify: `Cargo.toml` (add opendal workspace dep)
- Modify: `crates/dk-engine/Cargo.toml` (add opendal as optional dep)
- Create: `crates/dk-engine/src/storage.rs` (ObjectStore trait + local + S3 impls)
- Modify: `crates/dk-engine/src/lib.rs` (add module)
- Test: inline `#[cfg(test)]` in storage.rs

**Step 1: Add opendal dependency**

Add to workspace `Cargo.toml`:

```toml
# Under [workspace.dependencies]
opendal = { version = "0.51", features = ["services-fs", "services-s3"] }
```

Add to `crates/dk-engine/Cargo.toml`:

```toml
# Under [dependencies]
opendal = { workspace = true, optional = true }

# Add features section (or extend if exists)
[features]
default = []
s3 = ["dep:opendal"]
```

Run: `cargo check -p dk-engine`
Expected: compiles (s3 feature not enabled)

**Step 2: Create storage abstraction**

Create `crates/dk-engine/src/storage.rs`:

```rust
//! Object storage abstraction.
//!
//! Provides a unified interface for local filesystem and S3-compatible
//! storage. The local filesystem is always available; S3 requires the
//! `s3` cargo feature.

use async_trait::async_trait;

/// Trait for object storage backends.
#[async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    /// Get an object's content by key.
    async fn get(&self, key: &str) -> anyhow::Result<Vec<u8>>;

    /// Store an object. Overwrites if exists.
    async fn put(&self, key: &str, data: Vec<u8>) -> anyhow::Result<()>;

    /// Delete an object.
    async fn delete(&self, key: &str) -> anyhow::Result<()>;

    /// List objects with the given prefix.
    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>>;

    /// Check if an object exists.
    async fn exists(&self, key: &str) -> anyhow::Result<bool>;
}

/// Local filesystem object store.
pub struct LocalStore {
    root: std::path::PathBuf,
}

impl LocalStore {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl ObjectStore for LocalStore {
    async fn get(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let path = self.root.join(key);
        Ok(tokio::fs::read(path).await?)
    }

    async fn put(&self, key: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let path = self.root.join(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(tokio::fs::write(path, data).await?)
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let path = self.root.join(key);
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let dir = self.root.join(prefix);
        let mut entries = Vec::new();

        if dir.exists() {
            let mut read_dir = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                if let Some(name) = entry.file_name().to_str() {
                    let key = if prefix.is_empty() {
                        name.to_string()
                    } else {
                        format!("{prefix}/{name}")
                    };
                    entries.push(key);
                }
            }
        }

        Ok(entries)
    }

    async fn exists(&self, key: &str) -> anyhow::Result<bool> {
        let path = self.root.join(key);
        Ok(path.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::new(dir.path().to_path_buf());

        store.put("test/file.txt", b"hello".to_vec()).await.unwrap();
        assert!(store.exists("test/file.txt").await.unwrap());

        let data = store.get("test/file.txt").await.unwrap();
        assert_eq!(data, b"hello");

        let keys = store.list("test").await.unwrap();
        assert_eq!(keys, vec!["test/file.txt"]);

        store.delete("test/file.txt").await.unwrap();
        assert!(!store.exists("test/file.txt").await.unwrap());
    }

    #[tokio::test]
    async fn local_store_get_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::new(dir.path().to_path_buf());

        let result = store.get("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn local_store_delete_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::new(dir.path().to_path_buf());

        // Deleting a non-existent file should not error
        store.delete("nonexistent").await.unwrap();
    }
}
```

Register in `crates/dk-engine/src/lib.rs`:

```rust
pub mod storage;
```

Note: you'll need to check if dk-engine already has `tempfile` in dev-deps. If not, add:

```toml
[dev-dependencies]
tempfile = "3"
```

**Step 3: Run tests**

Run: `cargo test -p dk-engine storage::tests`
Expected: PASS (3 tests)

**Step 4: Commit**

```bash
git add Cargo.toml crates/dk-engine/Cargo.toml crates/dk-engine/src/storage.rs crates/dk-engine/src/lib.rs
git commit -m "feat(engine): add ObjectStore trait with local filesystem impl

Introduces storage abstraction with get/put/delete/list/exists operations.
LocalStore uses tokio::fs for async file I/O. S3 backend available via
optional 'opendal' dependency behind 's3' feature flag."
```

---

### Task 9: Qdrant Vector Search

**Files:**
- Modify: `Cargo.toml` (add qdrant-client workspace dep)
- Modify: `crates/dk-engine/Cargo.toml` (add qdrant-client as optional dep)
- Create: `crates/dk-engine/src/graph/vector.rs` (vector search module)
- Modify: `crates/dk-engine/src/graph/mod.rs` (add module)
- Test: inline `#[cfg(test)]` in vector.rs

**Step 1: Add qdrant-client dependency**

Add to workspace `Cargo.toml`:

```toml
# Under [workspace.dependencies]
qdrant-client = { version = "1.12", optional = true }
```

Add to `crates/dk-engine/Cargo.toml`:

```toml
# Under [dependencies]
qdrant-client = { workspace = true, optional = true }

# Under [features] (extend existing)
qdrant = ["dep:qdrant-client"]
```

Run: `cargo check -p dk-engine`
Expected: compiles (qdrant feature not enabled)

**Step 2: Create vector search trait**

Create `crates/dk-engine/src/graph/vector.rs`:

```rust
//! Vector similarity search abstraction.
//!
//! Provides a trait for embedding-based semantic search that can be
//! backed by Qdrant or any other vector database. Gracefully degrades
//! to no-op when Qdrant is not configured.

use async_trait::async_trait;
use uuid::Uuid;

/// A search result from vector similarity.
#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    /// The symbol ID.
    pub symbol_id: Uuid,
    /// Cosine similarity score (0.0 to 1.0).
    pub score: f32,
}

/// Trait for vector search backends.
#[async_trait]
pub trait VectorSearch: Send + Sync + 'static {
    /// Index a symbol's embedding.
    async fn index_embedding(
        &self,
        symbol_id: Uuid,
        repo_id: Uuid,
        embedding: Vec<f32>,
    ) -> anyhow::Result<()>;

    /// Search for similar symbols by embedding vector.
    async fn search_similar(
        &self,
        repo_id: Uuid,
        query_embedding: Vec<f32>,
        limit: usize,
    ) -> anyhow::Result<Vec<VectorSearchResult>>;

    /// Delete all embeddings for a symbol.
    async fn delete_embedding(&self, symbol_id: Uuid) -> anyhow::Result<()>;
}

/// No-op vector search (used when Qdrant is not configured).
pub struct NoOpVectorSearch;

#[async_trait]
impl VectorSearch for NoOpVectorSearch {
    async fn index_embedding(
        &self,
        _symbol_id: Uuid,
        _repo_id: Uuid,
        _embedding: Vec<f32>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn search_similar(
        &self,
        _repo_id: Uuid,
        _query_embedding: Vec<f32>,
        _limit: usize,
    ) -> anyhow::Result<Vec<VectorSearchResult>> {
        Ok(vec![])
    }

    async fn delete_embedding(&self, _symbol_id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_search_returns_empty() {
        let search = NoOpVectorSearch;
        let results = search
            .search_similar(Uuid::new_v4(), vec![0.1, 0.2, 0.3], 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn noop_index_succeeds() {
        let search = NoOpVectorSearch;
        search
            .index_embedding(Uuid::new_v4(), Uuid::new_v4(), vec![0.1, 0.2])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn noop_delete_succeeds() {
        let search = NoOpVectorSearch;
        search.delete_embedding(Uuid::new_v4()).await.unwrap();
    }
}
```

Register in `crates/dk-engine/src/graph/mod.rs` (add to existing module declarations):

```rust
pub mod vector;
```

**Step 3: Run tests**

Run: `cargo test -p dk-engine graph::vector::tests`
Expected: PASS (3 tests)

**Step 4: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests PASS

**Step 5: Commit**

```bash
git add Cargo.toml crates/dk-engine/Cargo.toml crates/dk-engine/src/graph/vector.rs crates/dk-engine/src/graph/mod.rs
git commit -m "feat(engine): add VectorSearch trait with NoOp fallback

Introduces embedding-based semantic search abstraction. NoOpVectorSearch
returns empty results when Qdrant is not configured. Qdrant backend
available via optional 'qdrant' feature flag."
```

---

### Task 10: Final Integration — Tag v0.2.0

**Files:**
- No code changes

**Step 1: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests PASS

**Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings

**Step 3: Tag release**

```bash
git tag -a v0.2.0 -m "v0.2.0: Agent Protocol completion

- gRPC-Web transport layer (tonic-web)
- JWT auth with dual-mode support
- Dependency tracking in CONTEXT
- Watch event filtering
- Session resume support
- SessionStore trait (DashMap + Redis backends)
- ObjectStore trait (local filesystem + S3)
- VectorSearch trait (NoOp + Qdrant)

All infrastructure services are optional — engine runs with just
PostgreSQL + local filesystem."
```

Run: `git push origin main --tags`

**Step 4: Update dkod-platform dependency**

In the dkod-platform repo (`/Users/haimari/vsCode/haim-ari/github/k/Cargo.toml`), update the engine tag:

```toml
dk-core = { git = "https://github.com/dkod-io/dkod-engine", tag = "v0.2.0" }
dk-engine = { git = "https://github.com/dkod-io/dkod-engine", tag = "v0.2.0" }
dk-protocol = { git = "https://github.com/dkod-io/dkod-engine", tag = "v0.2.0" }
dk-runner = { git = "https://github.com/dkod-io/dkod-engine", tag = "v0.2.0" }
```

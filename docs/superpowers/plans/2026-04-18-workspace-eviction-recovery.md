# Workspace eviction recovery (Epic B) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Epic B design from `docs/superpowers/specs/2026-04-18-workspace-eviction-recovery-design.md`: pin workspaces with non-terminal changesets, strand+release-locks on unavoidable eviction, resume via `dk_connect{resume_session_id}` backed by overlay rehydration, and auto-abandon at 4 h with `dk_abandon` for explicit early abandon.

**Architecture:** In-process edits to `WorkspaceManager` (pin guard + strand + resume + abandon + startup_reconcile). A new `stranded_at` column on `workspaces` is the single source of truth for stranded-session detection. `dk_connect` gains full overlay-rehydrate resume (extending the existing snapshot-based resume path). A shared `require_live_session` middleware hoist surfaces `SESSION_STRANDED` to callers uniformly. New `dk_abandon` RPC + admin CLI escape hatch. Metric hooks follow PR #74's convention.

**Tech Stack:** Rust (edition 2021), tonic + prost for gRPC, sqlx for Postgres, tokio async runtime, existing `claim_tracker.rs` for lock release, existing `FileOverlay::restore_from_db` for rehydration.

---

## Pre-implementation notes

- **Branching.** This plan should be executed on a fresh branch off `main`, not on `feat/release-locks-default-on`. Recommended: `git checkout main && git pull && git checkout -b feat/eviction-recovery` before Task 1.
- **Tests.** Scope every `cargo test` to `-p dk-engine` / `-p dk-protocol` / `-p dk-cli` as appropriate; never `cargo test --workspace` (memory rule).
- **Lint/format.** `cargo clippy -p <crate> -- -D warnings` and `cargo fmt` before every commit (pre-commit hook auto-formats; treat clippy failures as blocking).
- **Proto sync.** After editing `proto/dkod/v1/agent.proto`, copy verbatim to `crates/dk-protocol/proto/dkod/v1/agent.proto` (CLAUDE.md rule — CI fails otherwise).
- **Code review.** Per global CLAUDE.md, run `/coderabbit:review` (Claude Code plugin) before every commit. Rust-bearing commits must be reviewed clean before push.

---

## File structure

### Created

- `migrations/016_workspaces_stranded_at.sql` — schema additions (`stranded_at`, `stranded_reason`, `abandoned_at`, `abandoned_reason`, `superseded_by`).
- `crates/dk-protocol/src/abandon.rs` — `handle_abandon` RPC handler.
- `crates/dk-protocol/src/require_live_session.rs` — middleware helper used by every RPC.
- `crates/dk-cli/src/commands/admin.rs` — admin subcommand group (starting with `admin abandon`).
- `crates/dk-engine/tests/integration/eviction_recovery_test.rs` — the eight integration test cases from the spec.

### Modified

- `proto/dkod/v1/agent.proto` + `crates/dk-protocol/proto/dkod/v1/agent.proto` — wire `AbandonRequest`/`AbandonResponse`, extend error details, tag existing `resume_session_id` semantics in a comment. **Keep in sync.**
- `crates/dk-engine/src/workspace/session_manager.rs` — new methods (`should_pin`, `strand`, `abandon_stranded`, `resume`, `startup_reconcile`), pin guard in `gc_expired_sessions` + `cleanup_disconnected`.
- `crates/dk-engine/src/workspace/session_workspace.rs` — new fields (`stranded_at`, etc.) on `SessionWorkspace`; DB hydration updates.
- `crates/dk-engine/src/workspace/overlay.rs` — add `FileOverlay::drop_for_workspace`.
- `crates/dk-engine/src/workspace/mod.rs` — re-exports for new types.
- `crates/dk-engine/src/changeset.rs` — expose `is_terminal(&ChangesetState) -> bool` helper.
- `crates/dk-protocol/src/server.rs` — register abandon RPC, plumb middleware.
- `crates/dk-protocol/src/lib.rs` — module registration for `abandon`, `require_live_session`.
- `crates/dk-protocol/src/connect.rs` — extend existing resume branch to handle stranded-workspace rehydration (not just snapshot-based resume).
- `crates/dk-protocol/src/submit.rs`, `merge.rs`, `verify.rs`, `file_read.rs`, `file_write.rs`, `context.rs`, `pre_submit.rs`, `watch.rs`, `push.rs`, `session.rs` — call `require_live_session` at entry.
- `crates/dk-protocol/src/metrics.rs` — new counters/gauges.
- `crates/dk-mcp/src/server.rs` — expose `dk_abandon` MCP tool; pass `resume_session_id` through `dk_connect`.
- `crates/dk-server/src/main.rs` — invoke `startup_reconcile` before `tonic::Server::serve`.
- `crates/dk-cli/src/commands/mod.rs` — wire `admin` subcommand.
- `crates/dk-cli/src/cli.rs` (or wherever the clap tree lives) — register admin group.

---

## Task 1: Schema migration

**Files:**
- Create: `migrations/016_workspaces_stranded_at.sql`

- [ ] **Step 1: Write the migration**

Create `migrations/016_workspaces_stranded_at.sql`:

```sql
-- 016_workspaces_stranded_at.sql
-- Epic B: workspace eviction recovery. Adds lifecycle columns for the
-- strand → resume / abandon flow. All columns are nullable and additive —
-- existing rows are unaffected.

ALTER TABLE session_workspaces
    ADD COLUMN stranded_at       TIMESTAMPTZ,
    ADD COLUMN stranded_reason   TEXT,
    ADD COLUMN abandoned_at      TIMESTAMPTZ,
    ADD COLUMN abandoned_reason  TEXT,
    ADD COLUMN superseded_by     UUID REFERENCES session_workspaces(session_id);

-- Partial index: the stranded_sweep scans only rows where stranded_at IS NOT NULL.
CREATE INDEX idx_session_workspaces_stranded_at
    ON session_workspaces (stranded_at)
    WHERE stranded_at IS NOT NULL;

-- Partial index: startup_reconcile filters on non-abandoned rows missing a live session.
CREATE INDEX idx_session_workspaces_alive
    ON session_workspaces (session_id)
    WHERE stranded_at IS NULL AND abandoned_at IS NULL;
```

- [ ] **Step 2: Apply migration against the local test DB**

Run:

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    sqlx migrate run --source migrations
```

Expected: `Applied 016/migrate workspaces stranded at`.

- [ ] **Step 3: Verify schema**

Run:

```bash
psql "postgres://dkod:dkod@localhost:5432/dkod_test" -c "\d workspaces"
```

Expected: the five new columns listed, plus the two partial indexes in the index section.

- [ ] **Step 4: Commit**

```bash
git add migrations/016_workspaces_stranded_at.sql
git commit -m "feat(engine): add stranded/abandoned lifecycle columns to workspaces"
```

---

## Task 2: `ChangesetState::is_terminal` helper

**Files:**
- Modify: `crates/dk-engine/src/changeset.rs`
- Test: `crates/dk-engine/src/changeset.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Append inside the existing `#[cfg(test)] mod tests { … }` block in `crates/dk-engine/src/changeset.rs`:

```rust
#[test]
fn is_terminal_partitions_changeset_states() {
    assert!(!ChangesetState::Submitted.is_terminal());
    assert!(!ChangesetState::Verifying.is_terminal());
    assert!(!ChangesetState::Approved.is_terminal());
    assert!(ChangesetState::Merged.is_terminal());
    assert!(ChangesetState::Rejected.is_terminal());
    assert!(ChangesetState::Closed.is_terminal());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p dk-engine --lib changeset::tests::is_terminal_partitions_changeset_states -- --nocapture
```

Expected: compile error `no method named 'is_terminal' found for enum ChangesetState`.

- [ ] **Step 3: Implement**

Add near the existing `as_str`/`from_str` impls in `crates/dk-engine/src/changeset.rs`:

```rust
impl ChangesetState {
    /// True when the changeset is in a terminal state and its backing
    /// workspace no longer needs to be preserved.
    ///
    /// Used by the workspace pin guard (see Epic B spec) to decide
    /// whether to evict or skip a candidate workspace.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Merged | Self::Rejected | Self::Closed)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p dk-engine --lib changeset::tests::is_terminal_partitions_changeset_states
```

Expected: `test changeset::tests::is_terminal_partitions_changeset_states ... ok`.

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy -p dk-engine -- -D warnings
git add crates/dk-engine/src/changeset.rs
git commit -m "feat(engine): ChangesetState::is_terminal"
```

---

## Task 3: `WorkspaceManager::should_pin`

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Test: `crates/dk-engine/tests/integration/session_gc_test.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/dk-engine/tests/integration/session_gc_test.rs` (gated on DATABASE_URL like the existing tests there):

```rust
#[sqlx::test]
async fn should_pin_returns_true_for_non_terminal_states(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    assert!(mgr.should_pin(&session_id).await);
}

#[sqlx::test]
async fn should_pin_returns_false_for_terminal_states(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    for state in [ChangesetState::Merged, ChangesetState::Rejected, ChangesetState::Closed] {
        let session_id = insert_workspace_with_changeset(&pool, state).await;
        assert!(!mgr.should_pin(&session_id).await, "state {state:?} should not pin");
    }
}

#[sqlx::test]
async fn should_pin_returns_false_when_session_has_no_changeset(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool);
    let missing = Uuid::new_v4();
    assert!(!mgr.should_pin(&missing).await);
}

// Test helper — insert a workspace + changeset row for testing should_pin.
// The tests that need DB rows share this helper; keep it local.
async fn insert_workspace_with_changeset(pool: &PgPool, state: ChangesetState) -> Uuid {
    let session_id = Uuid::new_v4();
    let changeset_id = Uuid::new_v4();
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workspaces (session_id, repo_id, agent_id, changeset_id, state,
                                 mode, base_commit, created_at, last_active, intent)
         VALUES ($1, $2, 'agent-test', $3, 'active', 'ephemeral', 'initial', now(), now(), 'test')",
    )
    .bind(session_id)
    .bind(repo_id)
    .bind(changeset_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO changesets (id, repo_id, state, intent, created_at, updated_at)
         VALUES ($1, $2, $3, 'test', now(), now())",
    )
    .bind(changeset_id)
    .bind(repo_id)
    .bind(state.as_str())
    .execute(pool)
    .await
    .unwrap();
    session_id
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test should_pin -- --nocapture
```

Expected: compile error `no method named 'should_pin' found for struct WorkspaceManager`.

- [ ] **Step 3: Implement**

Add to `crates/dk-engine/src/workspace/session_manager.rs` (after `touch_in_cache` or near other `&self`-returning helpers):

```rust
/// Return true when this workspace's changeset is in a non-terminal state
/// and the workspace should NOT be evicted by GC. See Epic B spec §Pin rule.
///
/// Uses a single indexed query; returns false on missing workspace/changeset
/// so the caller falls through to the existing eviction path.
pub async fn should_pin(&self, session_id: &SessionId) -> bool {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT c.state
        FROM workspaces w
        JOIN changesets c ON c.id = w.changeset_id
        WHERE w.session_id = $1
        LIMIT 1
        "#,
    )
    .bind(session_id)
    .fetch_optional(&self.db)
    .await
    .ok()
    .flatten();

    match row {
        Some((state,)) => crate::changeset::ChangesetState::from_str(&state)
            .is_some_and(|s| !s.is_terminal()),
        None => false,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test should_pin
```

Expected: three tests pass.

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy -p dk-engine -- -D warnings
git add crates/dk-engine/src/workspace/session_manager.rs crates/dk-engine/tests/integration/session_gc_test.rs
git commit -m "feat(engine): WorkspaceManager::should_pin guards non-terminal changesets"
```

---

## Task 4: `StrandReason` + `strand` method (no behavior wire-in yet)

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Test: `crates/dk-engine/tests/integration/session_gc_test.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[sqlx::test]
async fn strand_sets_stranded_at_and_is_idempotent(pool: PgPool) {
    use dk_engine::workspace::session_manager::StrandReason;
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;

    mgr.strand(&session_id, StrandReason::IdleTtl).await.unwrap();

    let (stranded_at, reason): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) = sqlx::query_as(
        "SELECT stranded_at, stranded_reason FROM workspaces WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(stranded_at.is_some());
    assert_eq!(reason.as_deref(), Some("idle_ttl"));

    // Idempotent: a second call must not error nor change stranded_at.
    let first_ts = stranded_at.unwrap();
    mgr.strand(&session_id, StrandReason::IdleTtl).await.unwrap();
    let (stranded_at2, _): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("SELECT stranded_at, stranded_reason FROM workspaces WHERE session_id = $1")
            .bind(session_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stranded_at2, Some(first_ts));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test strand_sets_stranded_at -- --nocapture
```

Expected: compile error (`StrandReason` / `strand` not defined).

- [ ] **Step 3: Implement**

Add to `crates/dk-engine/src/workspace/session_manager.rs`:

```rust
/// Why a workspace transitioned to stranded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrandReason {
    IdleTtl,
    CleanupDisconnected,
    StartupReconcile,
    Explicit,
}

impl StrandReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdleTtl => "idle_ttl",
            Self::CleanupDisconnected => "cleanup_disconnected",
            Self::StartupReconcile => "startup_reconcile",
            Self::Explicit => "explicit",
        }
    }
}

impl WorkspaceManager {
    /// Mark a workspace as stranded: persist `stranded_at` + `stranded_reason`,
    /// release any symbol locks via the existing `release_locks_and_emit` hook
    /// (called from the caller — this function only mutates DB + in-memory state),
    /// then drop the in-memory entry. Idempotent: a second call on an already-
    /// stranded row is a no-op.
    pub async fn strand(
        &self,
        session_id: &SessionId,
        reason: StrandReason,
    ) -> Result<()> {
        // Idempotent UPDATE: only the first call sets stranded_at.
        sqlx::query(
            r#"
            UPDATE workspaces
               SET stranded_at     = COALESCE(stranded_at, now()),
                   stranded_reason = COALESCE(stranded_reason, $2)
             WHERE session_id = $1
            "#,
        )
        .bind(session_id)
        .bind(reason.as_str())
        .execute(&self.db)
        .await
        .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        // Drop in-memory — lock release and event emission are the caller's
        // responsibility because they sit in a different crate (dk-protocol).
        self.last_touched.remove(session_id);
        self.workspaces.remove(session_id);
        self.evict_from_cache(&[*session_id]);
        Ok(())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test strand_sets_stranded_at
```

Expected: `ok`.

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy -p dk-engine -- -D warnings
git add crates/dk-engine/src/workspace/session_manager.rs crates/dk-engine/tests/integration/session_gc_test.rs
git commit -m "feat(engine): strand() method + StrandReason enum"
```

---

## Task 4b: Inject `ClaimTracker` + event-bus handle into `WorkspaceManager`

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Modify: `crates/dk-engine/src/workspace/mod.rs` (re-exports)
- Modify: wherever `WorkspaceManager::new` / `with_cache` is called in engine construction (search `grep -rn "WorkspaceManager::" crates/`)

**Why:** The spec's strand path must `release_locks_and_emit` — release per-session symbol locks and publish a `session.stranded` event — but the current `WorkspaceManager` only holds a `PgPool` + cache. Inject the existing `Arc<dyn ClaimTracker>` and an `Arc<dyn EventPublisher>` at construction time so `strand`, `sweep_stranded`, and `resume` can reach them without crossing the `dk-engine`/`dk-protocol` boundary.

- [ ] **Step 1: Add fields + constructor**

Extend `WorkspaceManager`:

```rust
pub trait EventPublisher: Send + Sync {
    fn publish_session_event(&self, name: &str, session_id: Uuid, changeset_id: Uuid, reason: &str);
}

pub struct WorkspaceManager {
    workspaces: DashMap<SessionId, SessionWorkspace>,
    agent_counters: DashMap<Uuid, AtomicU32>,
    db: PgPool,
    cache: Arc<dyn WorkspaceCache>,
    last_touched: DashMap<SessionId, Instant>,
    claim_tracker: Arc<dyn crate::conflict::ClaimTracker>,
    events: Arc<dyn EventPublisher>,
}

impl WorkspaceManager {
    pub fn with_deps(
        db: PgPool,
        cache: Arc<dyn WorkspaceCache>,
        claim_tracker: Arc<dyn crate::conflict::ClaimTracker>,
        events: Arc<dyn EventPublisher>,
    ) -> Self { /* … */ }
}
```

Keep the existing `new(db)` / `with_cache` constructors; have them default `claim_tracker` to a no-op (`Arc::new(LocalClaimTracker::new())`) and `events` to a no-op publisher so pre-existing tests still compile.

- [ ] **Step 2: Plumb dk-protocol's event bus into the engine**

In `crates/dk-protocol/src/server.rs` (or wherever `WorkspaceManager` is constructed for the live server), add a thin `EventBusAdapter` that implements `EventPublisher` by forwarding to `server.event_bus().publish(...)`. Pass it into `WorkspaceManager::with_deps` at startup.

- [ ] **Step 3: Update `strand` to release locks + emit**

In Task 4's `strand` body, after the `UPDATE workspaces` query and before the in-memory drop:

```rust
// Fetch repo_id + changeset_id so the release + event call has the right shape.
let row: Option<(Uuid, Uuid)> = sqlx::query_as(
    "SELECT repo_id, changeset_id FROM workspaces WHERE session_id = $1",
).bind(session_id).fetch_optional(&self.db).await.ok().flatten();
if let Some((repo_id, changeset_id)) = row {
    if let Err(e) = self.claim_tracker.release_session_locks(repo_id, *session_id).await {
        tracing::warn!(session_id = %session_id, error = %e, "strand: lock release failed");
    }
    self.events.publish_session_event("session.stranded", *session_id, changeset_id, reason.as_str());
    crate::metrics::incr_workspace_stranded(reason.as_str());
}
```

Mirror the pattern inside `abandon_stranded` (`session.abandoned`) and `resume` success path (`session.resumed`).

- [ ] **Step 4: Run the full GC test suite**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test
cargo clippy -p dk-engine -- -D warnings
```

Expected: all existing tests plus the new pin/strand tests pass; no clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/dk-engine/src/workspace/session_manager.rs crates/dk-engine/src/workspace/mod.rs \
        crates/dk-protocol/src/server.rs
git commit -m "feat(engine): inject ClaimTracker + EventPublisher into WorkspaceManager"
```

---

## Task 5: Pin guard in `gc_expired_sessions` + `cleanup_disconnected`

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Test: `crates/dk-engine/tests/integration/session_gc_test.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[sqlx::test]
async fn gc_skips_pinned_workspace(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    // Build an in-memory workspace whose last_active is old
    let ws = /* construct SessionWorkspace with last_active = Instant::now() - 10min */;
    mgr.insert_test_workspace(ws);

    let evicted = mgr
        .gc_expired_sessions_async(Duration::from_secs(60), Duration::from_secs(3600))
        .await;
    assert!(evicted.is_empty(), "pinned workspace must not be evicted");
    assert!(mgr.get_workspace(&session_id).is_some());
}

#[sqlx::test]
async fn gc_strands_non_pinned_expired_workspace(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Closed).await;
    let ws = /* construct SessionWorkspace with last_active far in past */;
    mgr.insert_test_workspace(ws);

    let evicted = mgr
        .gc_expired_sessions_async(Duration::from_secs(60), Duration::from_secs(3600))
        .await;
    assert_eq!(evicted, vec![session_id]);
    // Terminal-state workspaces are NOT stranded — they're evicted outright.
    let (stranded_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT stranded_at FROM workspaces WHERE session_id = $1")
            .bind(session_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(stranded_at.is_none());
}
```

Note: the existing `gc_expired_sessions` is synchronous — adding the pin check introduces an async boundary. Introduce a new `gc_expired_sessions_async` variant and keep the sync one as a thin wrapper that blocks on the runtime (see Step 3).

- [ ] **Step 2: Run tests to verify they fail**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test gc_skips_pinned -- --nocapture
```

Expected: compile error — `gc_expired_sessions_async` not defined.

- [ ] **Step 3: Implement**

Replace the sync body of `gc_expired_sessions` and add the async variant:

```rust
/// Activity-based GC with pin guard (Epic B). See spec §Pin rule.
pub async fn gc_expired_sessions_async(
    &self,
    idle_ttl: std::time::Duration,
    max_ttl: std::time::Duration,
) -> Vec<SessionId> {
    let now = Instant::now();
    // First pass: collect candidates without holding DashMap guards across awaits.
    let candidates: Vec<SessionId> = self
        .workspaces
        .iter()
        .filter(|entry| {
            let ws = entry.value();
            let idle = now.duration_since(ws.last_active);
            let total = now.duration_since(ws.created_at);
            idle > idle_ttl || total > max_ttl
        })
        .map(|entry| *entry.key())
        .collect();

    let mut evicted = Vec::new();
    for sid in candidates {
        if !pin_flag_enabled() || !self.should_pin(&sid).await {
            // Strand if non-terminal (for defense-in-depth — non-terminal sessions
            // here mean the pin flag is off). Otherwise just evict.
            if self.should_pin(&sid).await {
                // Pin flag off + would-have-pinned → strand instead of hard-delete.
                if let Err(e) = self.strand(&sid, StrandReason::IdleTtl).await {
                    tracing::warn!("strand failed during gc: {e}");
                }
            } else {
                self.last_touched.remove(&sid);
                self.workspaces.remove(&sid);
            }
            evicted.push(sid);
        }
    }
    if !evicted.is_empty() {
        self.evict_from_cache(&evicted);
    }
    evicted
}

fn pin_flag_enabled() -> bool {
    // DKOD_PIN_NONTERMINAL: default on, opt out with "0" / "false" / "off".
    std::env::var("DKOD_PIN_NONTERMINAL")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "0" | "false" | "off" | "no")
        })
        .unwrap_or(true)
}

/// Backwards-compatible synchronous wrapper. Blocks on a Tokio runtime if one
/// is available; falls back to the no-pin path otherwise (tests without a
/// runtime, rare admin scripts).
pub fn gc_expired_sessions(
    &self,
    idle_ttl: std::time::Duration,
    max_ttl: std::time::Duration,
) -> Vec<SessionId> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| {
            handle.block_on(self.gc_expired_sessions_async(idle_ttl, max_ttl))
        })
    } else {
        // No runtime — preserve pre-Epic-B behavior exactly. Only callers
        // that skip Tokio are admin scripts that don't care about pinning.
        self.gc_expired_sessions_legacy(idle_ttl, max_ttl)
    }
}

fn gc_expired_sessions_legacy(
    &self,
    idle_ttl: std::time::Duration,
    max_ttl: std::time::Duration,
) -> Vec<SessionId> {
    // (lift existing body verbatim from current gc_expired_sessions)
    // …
}
```

Do the symmetric change in `cleanup_disconnected`:

```rust
pub async fn cleanup_disconnected_async(&self, active_session_ids: &[uuid::Uuid]) {
    let candidates: Vec<uuid::Uuid> = self
        .workspaces
        .iter()
        .filter(|entry| !active_session_ids.contains(entry.key()))
        .map(|entry| *entry.key())
        .collect();
    for sid in candidates {
        if pin_flag_enabled() && self.should_pin(&sid).await {
            continue; // pinned, keep alive
        }
        // Otherwise strand (non-terminal flag-off case covered by should_pin returning false for terminals)
        if self.should_pin(&sid).await {
            if let Err(e) = self.strand(&sid, StrandReason::CleanupDisconnected).await {
                tracing::warn!("strand failed during cleanup_disconnected: {e}");
            }
        } else {
            self.last_touched.remove(&sid);
            self.workspaces.remove(&sid);
            self.evict_from_cache(&[sid]);
        }
    }
}
```

Keep the sync `cleanup_disconnected` as a thin wrapper identical in shape to the GC wrapper.

- [ ] **Step 4: Update existing GC caller**

Find and update the GC invocation in the engine (likely `crates/dk-engine/src/workspace/mod.rs` or wherever the periodic GC loop lives) to call the `_async` variant directly from async context.

- [ ] **Step 5: Run tests to verify they pass**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test
```

Expected: pre-existing tests still pass; new pin/strand tests pass.

- [ ] **Step 6: Clippy + commit**

```bash
cargo clippy -p dk-engine -- -D warnings
git add crates/dk-engine/src/workspace/session_manager.rs crates/dk-engine/tests/integration/session_gc_test.rs
git commit -m "feat(engine): pin non-terminal workspaces during GC + cleanup_disconnected"
```

---

## Task 6: `WorkspaceManager::startup_reconcile`

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Test: `crates/dk-engine/tests/integration/session_gc_test.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[sqlx::test]
async fn startup_reconcile_strands_orphaned_nonterminal_workspaces(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    let mgr = WorkspaceManager::new(pool.clone());
    // Orphan: workspace row exists, no in-memory workspace, changeset is Submitted.
    let orphan = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    // Control: same shape but terminal — should NOT be stranded.
    let terminal = insert_workspace_with_changeset(&pool, ChangesetState::Merged).await;
    // Control: already stranded — must be left alone.
    let already = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    sqlx::query("UPDATE workspaces SET stranded_at = now() - interval '1 hour', stranded_reason = 'idle_ttl' WHERE session_id = $1")
        .bind(already).execute(&pool).await.unwrap();

    let stranded_count = mgr.startup_reconcile().await.unwrap();
    assert_eq!(stranded_count, 1);

    let check = |sid: Uuid| async move {
        let (at, reason): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
            sqlx::query_as("SELECT stranded_at, stranded_reason FROM workspaces WHERE session_id = $1")
                .bind(sid).fetch_one(&pool).await.unwrap();
        (at, reason)
    };
    let (at_orphan, r_orphan) = check(orphan).await;
    assert!(at_orphan.is_some() && r_orphan.as_deref() == Some("startup_reconcile"));
    let (at_terminal, _) = check(terminal).await;
    assert!(at_terminal.is_none());
    // already-stranded: timestamp unchanged
    let (at_already, _) = check(already).await;
    assert!(at_already.is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test startup_reconcile -- --nocapture
```

Expected: compile error — method missing.

- [ ] **Step 3: Implement**

```rust
/// One-shot sweep at server boot: find orphaned non-terminal workspaces
/// (rows with no live in-memory workspace, changeset non-terminal,
/// stranded_at IS NULL) and mark them stranded so callers surface
/// SESSION_STRANDED and can resume. Returns the count of rows updated.
pub async fn startup_reconcile(&self) -> Result<usize> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT w.session_id
          FROM workspaces w
          JOIN changesets c ON c.id = w.changeset_id
         WHERE w.stranded_at IS NULL
           AND w.abandoned_at IS NULL
           AND c.state NOT IN ('merged', 'rejected', 'closed')
        "#,
    )
    .fetch_all(&self.db)
    .await
    .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

    let mut count = 0;
    for (sid,) in rows {
        // In-memory workspaces map will be empty at boot; this condition is
        // a safety belt in case startup_reconcile is invoked later (admin).
        if self.workspaces.contains_key(&sid) {
            continue;
        }
        self.strand(&sid, StrandReason::StartupReconcile).await?;
        count += 1;
    }
    Ok(count)
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test startup_reconcile
```

Expected: `ok`.

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy -p dk-engine -- -D warnings
git add crates/dk-engine/src/workspace/session_manager.rs crates/dk-engine/tests/integration/session_gc_test.rs
git commit -m "feat(engine): startup_reconcile strands orphaned non-terminal workspaces"
```

---

## Task 7: `FileOverlay::drop_for_workspace`

**Files:**
- Modify: `crates/dk-engine/src/workspace/overlay.rs`
- Test: `crates/dk-engine/src/workspace/overlay.rs` (inline `#[cfg(test)]`) + integration test

- [ ] **Step 1: Write the failing test**

Append to `crates/dk-engine/src/workspace/overlay.rs` tests:

```rust
#[sqlx::test]
async fn drop_for_workspace_removes_all_overlay_rows(pool: PgPool) {
    let workspace_id = Uuid::new_v4();
    // Insert two overlay rows
    for p in ["a.rs", "b.rs"] {
        sqlx::query(
            "INSERT INTO session_overlay_files (workspace_id, file_path, content, content_hash, change_type)
             VALUES ($1, $2, $3, 'h', 'modified')"
        ).bind(workspace_id).bind(p).bind(b"c".as_slice()).execute(&pool).await.unwrap();
    }
    FileOverlay::drop_for_workspace(&pool, workspace_id).await.unwrap();
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM session_overlay_files WHERE workspace_id = $1"
    ).bind(workspace_id).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --lib workspace::overlay::tests::drop_for_workspace -- --nocapture
```

Expected: compile error — method missing.

- [ ] **Step 3: Implement**

Add as an associated function (so it can be called without constructing a `FileOverlay`) in `crates/dk-engine/src/workspace/overlay.rs`:

```rust
impl FileOverlay {
    /// Delete every `session_overlay_files` row for a given workspace.
    /// Used by abandon_stranded (Epic B) to release persisted overlay bytes.
    pub async fn drop_for_workspace(db: &PgPool, workspace_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM session_overlay_files WHERE workspace_id = $1")
            .bind(workspace_id)
            .execute(db)
            .await?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --lib workspace::overlay::tests::drop_for_workspace
```

Expected: `ok`.

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy -p dk-engine -- -D warnings
git add crates/dk-engine/src/workspace/overlay.rs
git commit -m "feat(engine): FileOverlay::drop_for_workspace"
```

---

## Task 8: `abandon_stranded` with tombstone + changeset rejection

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Test: `crates/dk-engine/tests/integration/session_gc_test.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[sqlx::test]
async fn abandon_stranded_tombstones_and_rejects_changeset(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    use dk_engine::workspace::session_manager::{AbandonReason, StrandReason};
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    // Insert an overlay row so we can verify deletion.
    let workspace_id: Uuid = sqlx::query_scalar("SELECT workspace_id FROM workspaces WHERE session_id = $1")
        .bind(session_id).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO session_overlay_files(workspace_id, file_path, content, content_hash, change_type)
                 VALUES ($1, 'x.rs', 'c', 'h', 'modified')").bind(workspace_id).execute(&pool).await.unwrap();

    mgr.strand(&session_id, StrandReason::IdleTtl).await.unwrap();
    mgr.abandon_stranded(&session_id, AbandonReason::AutoTtl).await.unwrap();

    let (abandoned_at, reason, changeset_state, overlay_count): (Option<chrono::DateTime<chrono::Utc>>, Option<String>, String, i64) =
        sqlx::query_as(
            "SELECT w.abandoned_at, w.abandoned_reason, c.state,
                    (SELECT COUNT(*) FROM session_overlay_files WHERE workspace_id = w.workspace_id)
               FROM workspaces w
               JOIN changesets c ON c.id = w.changeset_id
              WHERE w.session_id = $1"
        ).bind(session_id).fetch_one(&pool).await.unwrap();
    assert!(abandoned_at.is_some());
    assert_eq!(reason.as_deref(), Some("auto_ttl"));
    assert_eq!(changeset_state, "rejected");
    assert_eq!(overlay_count, 0);
}

#[sqlx::test]
async fn abandon_stranded_is_idempotent(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    use dk_engine::workspace::session_manager::{AbandonReason, StrandReason};
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    mgr.strand(&session_id, StrandReason::IdleTtl).await.unwrap();
    mgr.abandon_stranded(&session_id, AbandonReason::AutoTtl).await.unwrap();
    // Second call — must not error, must not change abandoned_at.
    let first: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar("SELECT abandoned_at FROM workspaces WHERE session_id = $1")
        .bind(session_id).fetch_one(&pool).await.unwrap();
    mgr.abandon_stranded(&session_id, AbandonReason::Explicit { caller: "agent-test".into() }).await.unwrap();
    let second: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar("SELECT abandoned_at FROM workspaces WHERE session_id = $1")
        .bind(session_id).fetch_one(&pool).await.unwrap();
    assert_eq!(first, second);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test abandon_stranded -- --nocapture
```

Expected: compile error (`AbandonReason` / `abandon_stranded` missing).

- [ ] **Step 3: Implement**

Add to `crates/dk-engine/src/workspace/session_manager.rs`:

```rust
/// Why a stranded workspace was abandoned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbandonReason {
    AutoTtl,
    Explicit { caller: String },
    Admin { operator: String },
}

impl AbandonReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AutoTtl => "auto_ttl",
            Self::Explicit { .. } => "explicit",
            Self::Admin { .. } => "admin",
        }
    }
}

impl WorkspaceManager {
    /// Terminal cleanup for a stranded workspace: mark the changeset rejected,
    /// delete overlay rows, tombstone the workspace row. Idempotent.
    pub async fn abandon_stranded(
        &self,
        session_id: &SessionId,
        reason: AbandonReason,
    ) -> Result<()> {
        // Fetch workspace_id + changeset_id in one query; bail if missing.
        let row: Option<(Uuid, Uuid, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            "SELECT workspace_id, changeset_id, abandoned_at FROM workspaces WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        let Some((workspace_id, changeset_id, already_abandoned)) = row else {
            return Ok(()); // nothing to abandon
        };
        if already_abandoned.is_some() {
            return Ok(()); // idempotent
        }

        // 1. Drop overlay rows.
        crate::workspace::overlay::FileOverlay::drop_for_workspace(&self.db, workspace_id).await?;

        // 2. Mark changeset rejected with reject_reason.
        sqlx::query(
            "UPDATE changesets SET state='rejected', reason=$2, updated_at=now()
              WHERE id=$1 AND state NOT IN ('merged','rejected','closed')"
        )
        .bind(changeset_id)
        .bind(format!("session_abandoned:{}", reason.as_str()))
        .execute(&self.db)
        .await
        .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        // 3. Tombstone workspace row.
        sqlx::query(
            "UPDATE workspaces
                SET abandoned_at     = now(),
                    abandoned_reason = $2
              WHERE session_id = $1"
        )
        .bind(session_id)
        .bind(reason.as_str())
        .execute(&self.db)
        .await
        .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        // 4. Ensure in-memory state is gone (strand may have already dropped it).
        self.workspaces.remove(session_id);
        self.last_touched.remove(session_id);
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test abandon_stranded
```

Expected: both tests pass.

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy -p dk-engine -- -D warnings
git add crates/dk-engine/src/workspace/session_manager.rs crates/dk-engine/tests/integration/session_gc_test.rs
git commit -m "feat(engine): abandon_stranded with tombstone + changeset rejection"
```

---

## Task 9: Auto-abandon sweep

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Test: `crates/dk-engine/tests/integration/session_gc_test.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[sqlx::test]
async fn sweep_stranded_auto_abandons_past_ttl(pool: PgPool) {
    use dk_engine::changeset::ChangesetState;
    use dk_engine::workspace::session_manager::StrandReason;
    let mgr = WorkspaceManager::new(pool.clone());
    let young = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    let old   = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    mgr.strand(&young, StrandReason::IdleTtl).await.unwrap();
    mgr.strand(&old,   StrandReason::IdleTtl).await.unwrap();
    // Backdate `old`
    sqlx::query("UPDATE workspaces SET stranded_at = now() - interval '5 hours' WHERE session_id = $1")
        .bind(old).execute(&pool).await.unwrap();

    let n = mgr.sweep_stranded(std::time::Duration::from_secs(4 * 3600)).await.unwrap();
    assert_eq!(n, 1);
    let (old_aband, young_aband): (Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT
            (SELECT abandoned_at FROM workspaces WHERE session_id=$1),
            (SELECT abandoned_at FROM workspaces WHERE session_id=$2)",
    ).bind(old).bind(young).fetch_one(&pool).await.unwrap();
    assert!(old_aband.is_some());
    assert!(young_aband.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test sweep_stranded -- --nocapture
```

Expected: compile error.

- [ ] **Step 3: Implement**

```rust
impl WorkspaceManager {
    /// Auto-abandon stranded workspaces past their TTL. Returns the number
    /// of rows abandoned. Called from the engine's periodic GC loop.
    pub async fn sweep_stranded(&self, ttl: std::time::Duration) -> Result<usize> {
        let ttl_secs = ttl.as_secs() as i64;
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT session_id FROM workspaces
              WHERE stranded_at IS NOT NULL
                AND abandoned_at IS NULL
                AND stranded_at + make_interval(secs => $1::double precision) < now()"
        )
        .bind(ttl_secs as f64)
        .fetch_all(&self.db)
        .await
        .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        let mut count = 0;
        for (sid,) in rows {
            self.abandon_stranded(&sid, AbandonReason::AutoTtl).await?;
            count += 1;
        }
        Ok(count)
    }
}
```

- [ ] **Step 4: Wire into the engine's GC loop**

Locate the periodic GC worker (likely in `crates/dk-engine/src/workspace/mod.rs` or a dedicated GC module) and add a call to `sweep_stranded(Duration::from_secs(4 * 3600))` alongside the existing `gc_expired_sessions_async` call.

- [ ] **Step 5: Run tests + clippy**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test session_gc_test sweep_stranded
cargo clippy -p dk-engine -- -D warnings
```

Expected: pass, no clippy warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/dk-engine/src/workspace/session_manager.rs \
        crates/dk-engine/src/workspace/mod.rs \
        crates/dk-engine/tests/integration/session_gc_test.rs
git commit -m "feat(engine): sweep_stranded auto-abandons past 4h TTL"
```

---

## Task 10: `WorkspaceManager::resume` — transactional overlay + lock rehydrate

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_manager.rs`
- Test: `crates/dk-engine/tests/integration/eviction_recovery_test.rs` (new file)

- [ ] **Step 1: Scaffold the new integration test file**

Create `crates/dk-engine/tests/integration/eviction_recovery_test.rs` with module-level setup helpers:

```rust
//! Integration tests for Epic B — workspace eviction recovery.
//! Requires DATABASE_URL (see CLAUDE.md).

use dk_engine::changeset::ChangesetState;
use dk_engine::workspace::session_manager::{
    AbandonReason, ResumeResult, StrandReason, WorkspaceManager,
};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::{insert_workspace_with_changeset, insert_overlay_row, make_ephemeral_workspace};
```

Create `crates/dk-engine/tests/integration/common/mod.rs` (add to `Cargo.toml` if needed) with the shared insertion helpers — lift `insert_workspace_with_changeset` from Task 3 and add a helper that also inserts overlay rows.

- [ ] **Step 2: Write the happy-path resume test**

```rust
#[sqlx::test]
async fn resume_happy_path_restores_overlay_and_locks(pool: PgPool) {
    let mgr = WorkspaceManager::new(pool.clone());
    let dead = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    insert_overlay_row(&pool, dead, "src/lib.rs", b"fn foo() {}").await;
    mgr.strand(&dead, StrandReason::IdleTtl).await.unwrap();

    let new_session = Uuid::new_v4();
    let result = mgr
        .resume(&dead, new_session, /* agent_creds = same as dead */)
        .await
        .unwrap();

    match result {
        ResumeResult::Ok(ws) => {
            assert_eq!(ws.session_id, new_session);
            // Overlay restored
            assert!(ws.overlay.contains("src/lib.rs"));
        }
        other => panic!("expected Ok, got {:?}", other),
    }

    // DB: stranded_at cleared, superseded_by set, session_id rotated
    let (stranded_at, superseded_by, session_id): (Option<chrono::DateTime<chrono::Utc>>, Option<Uuid>, Uuid) =
        sqlx::query_as("SELECT stranded_at, superseded_by, session_id FROM workspaces WHERE workspace_id = (SELECT workspace_id FROM workspaces WHERE session_id = $1 UNION SELECT workspace_id FROM workspaces WHERE session_id = $2)")
            .bind(dead).bind(new_session).fetch_one(&pool).await.unwrap();
    assert!(stranded_at.is_none());
    assert_eq!(superseded_by, Some(new_session));
    assert_eq!(session_id, new_session);
}
```

- [ ] **Step 3: Run test to verify it fails**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test eviction_recovery_test resume_happy_path -- --nocapture
```

Expected: compile error — `resume`, `ResumeResult` missing.

- [ ] **Step 4: Implement `ResumeResult` + `resume`**

Add to `crates/dk-engine/src/workspace/session_manager.rs`:

```rust
/// Outcome of a resume attempt. The ok variant carries a freshly-constructed
/// SessionWorkspace keyed on the new session id.
#[derive(Debug)]
pub enum ResumeResult {
    Ok(SessionWorkspace),
    Contended(Vec<ConflictingSymbol>),
    AlreadyResumed(SessionId),
    Abandoned,
    NotStranded, // workspace exists but stranded_at IS NULL and not abandoned
}

#[derive(Debug, Clone)]
pub struct ConflictingSymbol {
    pub qualified_name: String,
    pub file_path: String,
    pub claimant_session: SessionId,
    pub claimant_agent: String,
}

impl WorkspaceManager {
    /// Transactionally rehydrate a stranded workspace under a new session id.
    ///
    /// - Preconditions: stranded_at IS NOT NULL, changeset non-terminal.
    /// - Steps: SELECT FOR UPDATE, overlay.restore_from_db, reindex graph,
    ///   atomic claim_tracker.acquire over overlay's changed symbols,
    ///   UPDATE workspaces SET session_id/stranded_at/superseded_by.
    /// - Caller must have already authorized agent_id against the row.
    pub async fn resume(
        &self,
        dead_session: &SessionId,
        new_session: SessionId,
        agent_id: &str,
    ) -> Result<ResumeResult> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        // SELECT ... FOR UPDATE, join changesets for state check.
        let row: Option<(Uuid, Uuid, Uuid, String, String, String, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>, String)> =
            sqlx::query_as(
                r#"
                SELECT w.workspace_id, w.repo_id, w.changeset_id, w.agent_id,
                       w.intent, w.base_commit, w.stranded_at, w.abandoned_at,
                       c.state
                  FROM workspaces w
                  JOIN changesets c ON c.id = w.changeset_id
                 WHERE w.session_id = $1
                 FOR UPDATE OF w
                "#,
            )
            .bind(dead_session)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        let Some((workspace_id, repo_id, changeset_id, orig_agent, intent, base_commit, stranded_at, abandoned_at, changeset_state)) = row else {
            tx.rollback().await.ok();
            return Ok(ResumeResult::NotStranded);
        };

        if abandoned_at.is_some() {
            tx.rollback().await.ok();
            return Ok(ResumeResult::Abandoned);
        }
        if stranded_at.is_none() {
            // Already resumed — look up who took over.
            let new_owner: Option<(Uuid,)> = sqlx::query_as(
                "SELECT superseded_by FROM workspaces WHERE session_id = $1",
            )
            .bind(dead_session)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| dk_core::Error::DbError(e.to_string()))?;
            tx.rollback().await.ok();
            return Ok(match new_owner.and_then(|(v,)| Some(v)) {
                Some(sid) => ResumeResult::AlreadyResumed(sid),
                None => ResumeResult::NotStranded,
            });
        }
        if crate::changeset::ChangesetState::from_str(&changeset_state)
            .is_some_and(|s| s.is_terminal())
        {
            tx.rollback().await.ok();
            return Ok(ResumeResult::Abandoned);
        }
        if orig_agent != agent_id {
            tx.rollback().await.ok();
            return Err(dk_core::Error::Unauthorized(format!(
                "resume requires original agent_id '{orig_agent}'"
            )));
        }

        // Rotate session_id + clear stranded_at.
        sqlx::query(
            r#"
            UPDATE workspaces
               SET session_id       = $2,
                   stranded_at      = NULL,
                   stranded_reason  = NULL,
                   superseded_by    = $2,
                   last_active      = now()
             WHERE session_id = $1
            "#,
        )
        .bind(dead_session)
        .bind(new_session)
        .execute(&mut *tx)
        .await
        .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| dk_core::Error::DbError(e.to_string()))?;

        // OUT OF TRANSACTION: rehydrate overlay + graph + locks.
        // If any step fails, roll back by re-stranding via strand().
        let mut ws = SessionWorkspace::hydrate(
            new_session,
            repo_id,
            orig_agent.clone(),
            changeset_id,
            intent,
            base_commit,
            workspace_id,
            self.db.clone(),
        )
        .await?;
        ws.overlay.restore_from_db().await?;
        ws.reindex_from_overlay().await?; // see Task 11 for this new method

        // Eagerly acquire locks for every changed symbol.
        let claimed: Vec<_> = ws.graph.changed_symbols_with_files();
        let mut conflicts = Vec::new();
        for (qname, path) in &claimed {
            match self
                .engine_claim_tracker()
                .try_acquire(repo_id, path, qname, new_session, &orig_agent)
                .await
            {
                Ok(_) => {}
                Err(info) => conflicts.push(ConflictingSymbol {
                    qualified_name: qname.clone(),
                    file_path: path.clone(),
                    claimant_session: info.session_id,
                    claimant_agent: info.agent_id,
                }),
            }
        }
        if !conflicts.is_empty() {
            // Roll back: release any locks we did acquire, re-strand.
            for (qname, path) in &claimed {
                let _ = self.engine_claim_tracker().release(repo_id, path, qname, new_session).await;
            }
            self.strand(&new_session, StrandReason::Explicit).await?;
            return Ok(ResumeResult::Contended(conflicts));
        }

        self.workspaces.insert(new_session, ws);
        // Return a reference-like copy via get_workspace for the caller.
        Ok(ResumeResult::Ok(
            self.get_workspace(&new_session).unwrap().value().clone_for_return(),
        ))
    }
}
```

Note the two helper accessors this implementation assumes exist — add stubs in Step 5:
- `SessionWorkspace::hydrate(...)` — build a `SessionWorkspace` from DB fields (new; mirrors `new()` but pre-populated).
- `SessionWorkspace::reindex_from_overlay()` — re-run the semantic graph indexer over the rehydrated overlay.
- `SessionWorkspace::clone_for_return()` — cheap clone for passing back through `ResumeResult::Ok`.
- `WorkspaceManager::engine_claim_tracker()` — accessor exposing the existing `ClaimTracker` held by the engine.

Each of these is small (<20 LOC) — implement inline while writing `resume`.

- [ ] **Step 5: Run tests + add contended + double-resume + abandoned cases**

Add three more tests to `eviction_recovery_test.rs`:

```rust
#[sqlx::test]
async fn resume_contended_rolls_back_and_stays_stranded(pool: PgPool) { /* ... */ }

#[sqlx::test]
async fn double_resume_returns_already_resumed(pool: PgPool) { /* ... */ }

#[sqlx::test]
async fn resume_after_abandon_returns_abandoned(pool: PgPool) { /* ... */ }
```

Each follows the established setup pattern. See spec §Testing for the exact scenarios.

Run:

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine --test eviction_recovery_test resume_ double_resume
cargo clippy -p dk-engine -- -D warnings
```

Expected: all four tests pass; no clippy warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/dk-engine/src/workspace/session_manager.rs \
        crates/dk-engine/src/workspace/session_workspace.rs \
        crates/dk-engine/tests/integration/eviction_recovery_test.rs \
        crates/dk-engine/tests/integration/common/mod.rs
git commit -m "feat(engine): WorkspaceManager::resume (overlay + locks, transactional)"
```

---

## Task 11: `SessionWorkspace::reindex_from_overlay` helper

**Files:**
- Modify: `crates/dk-engine/src/workspace/session_workspace.rs`
- Test: inline in same file

- [ ] **Step 1: Failing test**

```rust
#[sqlx::test]
async fn reindex_from_overlay_rebuilds_graph_from_restored_overlay(pool: PgPool) {
    // Build a workspace with a single .rs file in the overlay containing a function.
    // After reindex, the graph must contain that symbol.
    // (Use write_local to avoid DB round-trip for the inserted content.)
    let mut ws = SessionWorkspace::new_test_with_pool(pool.clone());
    ws.overlay.write_local("x.rs", b"fn hello() {}".to_vec(), true);

    ws.reindex_from_overlay().await.unwrap();
    assert!(ws.graph.contains_symbol("hello"));
}
```

- [ ] **Step 2: Run test, confirm fail**

```bash
DATABASE_URL=… cargo test -p dk-engine --lib workspace::session_workspace::tests::reindex_from_overlay -- --nocapture
```

- [ ] **Step 3: Implement**

Add to `SessionWorkspace`:

```rust
/// Re-index the semantic graph from the current overlay contents.
/// Called by WorkspaceManager::resume after restore_from_db.
pub async fn reindex_from_overlay(&mut self) -> Result<()> {
    let parser = self.parser.clone();
    for (path, entry) in self.overlay.list_changes() {
        let content = match entry.content() {
            Some(c) => c,
            None => {
                self.graph.remove_file(&path);
                continue;
            }
        };
        let text = std::str::from_utf8(content).map_err(|e| dk_core::Error::ParseError(e.to_string()))?;
        let symbols = parser.parse_symbols(&path, text)?;
        self.graph.replace_file_symbols(&path, symbols);
    }
    Ok(())
}
```

- [ ] **Step 4: Run test + clippy**

```bash
DATABASE_URL=… cargo test -p dk-engine --lib workspace::session_workspace::tests::reindex_from_overlay
cargo clippy -p dk-engine -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/dk-engine/src/workspace/session_workspace.rs
git commit -m "feat(engine): SessionWorkspace::reindex_from_overlay"
```

---

## Task 12: Proto additions — `AbandonRequest`/`Response`, error details

**Files:**
- Modify: `proto/dkod/v1/agent.proto`
- Modify: `crates/dk-protocol/proto/dkod/v1/agent.proto` (mirror)

- [ ] **Step 1: Edit the canonical proto**

Open `proto/dkod/v1/agent.proto`. Add below the last RPC definition in the `AgentService` service block:

```proto
rpc Abandon(AbandonRequest) returns (AbandonResponse);
```

Add message types near the other request/response pairs:

```proto
message AbandonRequest {
  // Session ID of the stranded workspace. Must be authorized against the
  // original agent credentials that created the workspace.
  string session_id = 1;
}

message AbandonResponse {
  bool success = 1;
  string changeset_id = 2;
  string abandoned_reason = 3;
}

// Error detail for SESSION_STRANDED status responses. Carried in the gRPC
// status `details` field so the harness can pattern-match.
message SessionStrandedDetail {
  string changeset_id = 1;
  string base_commit = 2;
  string stranded_reason = 3;
  string stranded_at_rfc3339 = 4;
}

message ResumeContendedDetail {
  message ConflictingSymbol {
    string qualified_name = 1;
    string file_path = 2;
    string claimant_session = 3;
    string claimant_agent = 4;
  }
  repeated ConflictingSymbol symbols = 1;
}

message AlreadyResumedDetail {
  string new_session_id = 1;
}

message SessionAbandonedDetail {
  string changeset_id = 1;
  string abandoned_reason = 2;
}
```

- [ ] **Step 2: Mirror to the packaging copy**

```bash
cp proto/dkod/v1/agent.proto crates/dk-protocol/proto/dkod/v1/agent.proto
diff -r proto/dkod/v1 crates/dk-protocol/proto/dkod/v1
```

Expected: `diff` produces no output (identical).

- [ ] **Step 3: Regenerate + build**

```bash
cargo build -p dk-protocol
```

Expected: clean build; generated types include `AbandonRequest`, `AbandonResponse`, the detail messages.

- [ ] **Step 4: Commit**

```bash
git add proto/dkod/v1/agent.proto crates/dk-protocol/proto/dkod/v1/agent.proto
git commit -m "feat(proto): add Abandon RPC + stranded/contended/abandoned error details"
```

---

## Task 13: `require_live_session` middleware helper

**Files:**
- Create: `crates/dk-protocol/src/require_live_session.rs`
- Modify: `crates/dk-protocol/src/lib.rs` (register module)

- [ ] **Step 1: Failing test**

Create `crates/dk-protocol/src/require_live_session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn returns_proceed_when_workspace_present() { /* see Step 3 */ }
    #[tokio::test]
    async fn returns_stranded_status_when_workspace_missing_but_stranded() { /* see Step 3 */ }
    #[tokio::test]
    async fn returns_abandoned_status_when_workspace_missing_and_abandoned() { /* see Step 3 */ }
}
```

- [ ] **Step 2: Run, confirm fail**

- [ ] **Step 3: Implement**

```rust
//! Middleware — every dk_* RPC must call this before touching workspace state.
//! If the workspace is live in-memory, returns Ok. Otherwise looks up the
//! persisted workspace row and translates missing/stranded/abandoned into
//! structured gRPC statuses.

use tonic::{Code, Status};
use uuid::Uuid;

use crate::server::ProtocolServer;

pub async fn require_live_session(
    server: &ProtocolServer,
    session_id: &str,
) -> Result<(), Status> {
    let sid = session_id
        .parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;

    if server.engine().workspace_manager().get_workspace(&sid).is_some() {
        return Ok(());
    }

    // Look up persistent state.
    let row: Option<(Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>, Uuid, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            r#"
            SELECT w.stranded_at, w.abandoned_at, w.changeset_id,
                   w.stranded_reason, w.abandoned_reason, w.base_commit
              FROM workspaces w
             WHERE w.session_id = $1
            "#,
        )
        .bind(sid)
        .fetch_optional(server.engine().db())
        .await
        .map_err(|e| Status::internal(format!("workspace lookup failed: {e}")))?;

    let Some((stranded_at, abandoned_at, changeset_id, stranded_reason, abandoned_reason, base_commit)) = row else {
        return Err(Status::not_found("Workspace not found for session"));
    };

    if let Some(at) = abandoned_at {
        // Structured status: use gRPC metadata (not proto status-details). Metadata
        // is the simplest correct transport — tonic serialises each key verbatim
        // and the harness pattern-matches on `dk-error`. Switching to Status::with_details
        // + the proto detail messages is possible later but adds a prost dependency
        // to this module with no functional gain.
        let mut st = Status::failed_precondition("session abandoned");
        st.metadata_mut().insert("dk-error", "SESSION_ABANDONED".parse().unwrap());
        st.metadata_mut().insert("dk-changeset-id", changeset_id.to_string().parse().unwrap());
        if let Some(r) = abandoned_reason {
            st.metadata_mut().insert("dk-abandoned-reason", r.parse().unwrap());
        }
        st.metadata_mut().insert("dk-abandoned-at", at.to_rfc3339().parse().unwrap());
        return Err(st);
    }
    if let Some(at) = stranded_at {
        let mut st = Status::failed_precondition("session stranded");
        st.metadata_mut().insert("dk-error", "SESSION_STRANDED".parse().unwrap());
        st.metadata_mut().insert("dk-changeset-id", changeset_id.to_string().parse().unwrap());
        st.metadata_mut().insert("dk-base-commit", base_commit.parse().unwrap());
        if let Some(r) = stranded_reason {
            st.metadata_mut().insert("dk-stranded-reason", r.parse().unwrap());
        }
        st.metadata_mut().insert("dk-stranded-at", at.to_rfc3339().parse().unwrap());
        return Err(st);
    }

    // Row exists but neither stranded nor abandoned nor in-memory: transient
    // (e.g. cache eviction with no persistence) — treat as not found.
    Err(Status::not_found("Workspace not found for session"))
}
```

Then in `crates/dk-protocol/src/lib.rs`, add:

```rust
pub mod require_live_session;
```

- [ ] **Step 4: Fill in the test bodies from Step 1**

Three tests, each constructs a `ProtocolServer` mock or uses a real one against the test DB.

- [ ] **Step 5: Run tests + clippy + commit**

```bash
DATABASE_URL=… cargo test -p dk-protocol --lib require_live_session
cargo clippy -p dk-protocol -- -D warnings
git add crates/dk-protocol/src/require_live_session.rs crates/dk-protocol/src/lib.rs
git commit -m "feat(protocol): require_live_session middleware surfaces STRANDED/ABANDONED"
```

---

## Task 14: Wire `require_live_session` into every RPC

**Files (modify):**
- `crates/dk-protocol/src/submit.rs`
- `crates/dk-protocol/src/merge.rs`
- `crates/dk-protocol/src/verify.rs`
- `crates/dk-protocol/src/file_read.rs`
- `crates/dk-protocol/src/file_write.rs`
- `crates/dk-protocol/src/file_list.rs`
- `crates/dk-protocol/src/context.rs`
- `crates/dk-protocol/src/pre_submit.rs`
- `crates/dk-protocol/src/push.rs`
- `crates/dk-protocol/src/watch.rs`
- `crates/dk-protocol/src/session.rs`

Exclude: `connect.rs` (no pre-existing session yet), `abandon.rs` (new — handles stranded explicitly).

- [ ] **Step 1: Write a behavior test for one representative RPC**

Pick `submit.rs` as the smoke test. In `crates/dk-engine/tests/integration/eviction_recovery_test.rs`:

```rust
#[sqlx::test]
async fn submit_on_stranded_session_returns_session_stranded(pool: PgPool) {
    // Build a ProtocolServer, create+strand a workspace, call submit with the dead session id.
    // Assert: gRPC status metadata contains dk-error=SESSION_STRANDED + dk-changeset-id.
    ...
}
```

- [ ] **Step 2: Run, confirm fail**

Expected: currently `submit.rs` returns `Status::not_found("Workspace not found…")` — test fails because metadata is missing.

- [ ] **Step 3: Edit each RPC handler**

The mechanical change: at the top of each handler (after `validate_session`), call the middleware. Example from `submit.rs`:

```rust
pub async fn handle_submit(
    server: &ProtocolServer,
    req: SubmitRequest,
) -> Result<Response<SubmitResponse>, Status> {
    let session = server.validate_session(&req.session_id)?;
    crate::require_live_session::require_live_session(server, &req.session_id).await?;
    // ... existing body unchanged ...
}
```

Apply to all 11 files listed above. The `validate_session` call stays (it checks the JWT); `require_live_session` adds the workspace-level check.

- [ ] **Step 4: Remove now-redundant `Workspace not found` branches**

In each handler, the pre-existing `.ok_or_else(|| Status::not_found("Workspace not found…"))` on the `get_workspace` lookup becomes unreachable after the middleware (the middleware would have returned first). You may leave them as defensive belts or delete them — prefer delete, the middleware is the single source of truth.

- [ ] **Step 5: Run full protocol test suite + clippy**

```bash
DATABASE_URL=… cargo test -p dk-protocol
DATABASE_URL=… cargo test -p dk-engine --test eviction_recovery_test submit_on_stranded
cargo clippy -p dk-protocol -- -D warnings
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/dk-protocol/src/{submit,merge,verify,file_read,file_write,file_list,context,pre_submit,push,watch,session}.rs
git commit -m "feat(protocol): hoist require_live_session into all RPC handlers"
```

---

## Task 15: `handle_abandon` RPC

**Files:**
- Create: `crates/dk-protocol/src/abandon.rs`
- Modify: `crates/dk-protocol/src/lib.rs`
- Modify: `crates/dk-protocol/src/server.rs` (register RPC)

- [ ] **Step 1: Failing test**

Append to `eviction_recovery_test.rs`:

```rust
#[sqlx::test]
async fn abandon_rpc_owner_success(pool: PgPool) {
    // Create+strand, then call handle_abandon with the stranded session's agent_id.
    // Assert: AbandonResponse.success=true, DB row has abandoned_at set.
}

#[sqlx::test]
async fn abandon_rpc_non_owner_returns_unauthenticated(pool: PgPool) {
    // Different agent creds → Status::unauthenticated.
}
```

- [ ] **Step 2: Implement**

Create `crates/dk-protocol/src/abandon.rs`:

```rust
use tonic::{Response, Status};
use uuid::Uuid;

use crate::server::ProtocolServer;
use crate::{AbandonRequest, AbandonResponse};
use dk_engine::workspace::session_manager::AbandonReason;

pub async fn handle_abandon(
    server: &ProtocolServer,
    req: AbandonRequest,
) -> Result<Response<AbandonResponse>, Status> {
    // Authorize: caller must be the agent that originally created the stranded workspace.
    let caller_agent = server.authorize_agent_from_context()?;
    let sid = req
        .session_id
        .parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;

    let row: Option<(String, Uuid, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as(
            "SELECT agent_id, changeset_id, stranded_at, abandoned_at
               FROM workspaces WHERE session_id = $1",
        )
        .bind(sid)
        .fetch_optional(server.engine().db())
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let Some((orig_agent, changeset_id, stranded_at, abandoned_at)) = row else {
        return Err(Status::not_found("Workspace not found"));
    };
    if orig_agent != caller_agent {
        return Err(Status::unauthenticated(format!(
            "abandon requires original agent_id '{orig_agent}'"
        )));
    }
    if abandoned_at.is_some() {
        return Ok(Response::new(AbandonResponse {
            success: true,
            changeset_id: changeset_id.to_string(),
            abandoned_reason: "explicit".into(),
        }));
    }
    if stranded_at.is_none() {
        return Err(Status::failed_precondition("session is not stranded"));
    }

    server
        .engine()
        .workspace_manager()
        .abandon_stranded(&sid, AbandonReason::Explicit { caller: caller_agent })
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    Ok(Response::new(AbandonResponse {
        success: true,
        changeset_id: changeset_id.to_string(),
        abandoned_reason: "explicit".into(),
    }))
}
```

Register the module + RPC mapping in `server.rs` (look at how `handle_close` is wired — mirror that).

- [ ] **Step 3: Run tests + clippy + commit**

```bash
DATABASE_URL=… cargo test -p dk-engine --test eviction_recovery_test abandon_rpc
cargo clippy -p dk-protocol -- -D warnings
git add crates/dk-protocol/src/abandon.rs crates/dk-protocol/src/lib.rs crates/dk-protocol/src/server.rs \
        crates/dk-engine/tests/integration/eviction_recovery_test.rs
git commit -m "feat(protocol): handle_abandon RPC (owner-authorized)"
```

---

## Task 16: Extend `handle_connect` for stranded-workspace rehydration

**Files:**
- Modify: `crates/dk-protocol/src/connect.rs`
- Test: `crates/dk-engine/tests/integration/eviction_recovery_test.rs`

Context: `connect.rs` already handles `resume_session_id` for *snapshot-based* resume (in-memory snapshot). Epic B extends this: if the resume target is a stranded workspace (DB `stranded_at IS NOT NULL`) and not found in the snapshot cache, call `WorkspaceManager::resume(...)` to rehydrate from overlay.

- [ ] **Step 1: Failing test**

```rust
#[sqlx::test]
async fn connect_resume_from_stranded_rehydrates_workspace(pool: PgPool) {
    // 1. Strand a workspace via mgr.strand(...).
    // 2. Call handle_connect with workspace_config.resume_session_id = dead_session.
    // 3. Assert: ConnectResponse.session_id != dead_session, new workspace exists
    //    in-memory with restored overlay, old row is tombstoned via superseded_by.
}
```

- [ ] **Step 2: Implement**

In `handle_connect`, add a branch after the existing snapshot-resume block:

```rust
// If snapshot-resume didn't yield a snapshot but resume_session_id was set,
// check whether the target is a stranded workspace and rehydrate.
if resumed_snapshot.is_none() {
    if let Some(ref ws_config) = req.workspace_config {
        if let Some(ref resume_id_str) = ws_config.resume_session_id {
            if let Ok(dead) = resume_id_str.parse::<Uuid>() {
                let mgr = server.engine().workspace_manager();
                match mgr.resume(&dead, new_session_id, &agent_id).await {
                    Ok(ResumeResult::Ok(_ws)) => {
                        info!(resume_from = %dead, new_session_id = %new_session_id, "CONNECT: rehydrated stranded workspace");
                        // Skip normal workspace creation — resume already inserted it.
                        return Ok(Response::new(ConnectResponse {
                            session_id: new_session_id.to_string(),
                            /* fill remaining fields from the hydrated workspace */
                            ..Default::default()
                        }));
                    }
                    Ok(ResumeResult::Contended(symbols)) => {
                        let mut st = Status::failed_precondition("resume contended");
                        st.metadata_mut().insert("dk-error", "RESUME_CONTENDED".parse().unwrap());
                        // Serialize symbols into metadata (JSON) for harness consumption.
                        let json = serde_json::to_string(&symbols).unwrap_or_default();
                        st.metadata_mut().insert("dk-conflicting-symbols", json.parse().unwrap());
                        return Err(st);
                    }
                    Ok(ResumeResult::AlreadyResumed(new_sid)) => {
                        let mut st = Status::already_exists("already resumed");
                        st.metadata_mut().insert("dk-error", "ALREADY_RESUMED".parse().unwrap());
                        st.metadata_mut().insert("dk-new-session-id", new_sid.to_string().parse().unwrap());
                        return Err(st);
                    }
                    Ok(ResumeResult::Abandoned) => {
                        let mut st = Status::failed_precondition("session abandoned");
                        st.metadata_mut().insert("dk-error", "SESSION_ABANDONED".parse().unwrap());
                        return Err(st);
                    }
                    Ok(ResumeResult::NotStranded) => {
                        warn!(resume_session_id = %dead, "CONNECT: resume requested but workspace not stranded");
                    }
                    Err(e) => return Err(Status::internal(e.to_string())),
                }
            }
        }
    }
}
```

Insert before the normal "create new workspace" path.

- [ ] **Step 3: Run tests + clippy + commit**

```bash
DATABASE_URL=… cargo test -p dk-engine --test eviction_recovery_test connect_resume_from_stranded
cargo clippy -p dk-protocol -- -D warnings
git add crates/dk-protocol/src/connect.rs crates/dk-engine/tests/integration/eviction_recovery_test.rs
git commit -m "feat(protocol): connect resumes stranded workspaces via WorkspaceManager::resume"
```

---

## Task 17: MCP surface — `dk_abandon` tool + `dk_connect` resume arg

**Files:**
- Modify: `crates/dk-mcp/src/server.rs`

- [ ] **Step 1: Locate `dk_close` definition**

```bash
grep -n "fn handle_tool_close\|\"dk_close\"" /Users/haimari/vsCode/haim-ari/github/dkod-engine/crates/dk-mcp/src/server.rs
```

Expected: a `handle_tool_close`-style function and a dispatch entry.

- [ ] **Step 2: Add `dk_abandon`**

Mirror `dk_close`'s shape (inputs: `session_id`; calls through to the gRPC `Abandon` RPC). Register in the tool dispatch map.

- [ ] **Step 3: Add `resume_session_id` to `dk_connect` MCP args**

Extend the tool's schema JSON to declare `resume_session_id` as optional string, pass through to the ConnectRequest.workspace_config.

- [ ] **Step 4: Build + commit**

```bash
cargo build -p dk-mcp
cargo clippy -p dk-mcp -- -D warnings
git add crates/dk-mcp/src/server.rs
git commit -m "feat(mcp): dk_abandon tool + dk_connect resume_session_id"
```

---

## Task 18: `dk-server` calls `startup_reconcile` on boot

**Files:**
- Modify: `crates/dk-server/src/main.rs`

- [ ] **Step 1: Locate DB init + Server::serve**

```bash
grep -n "PgPool\|Server::builder\|serve" /Users/haimari/vsCode/haim-ari/github/dkod-engine/crates/dk-server/src/main.rs
```

- [ ] **Step 2: Insert the reconcile call**

Between DB-pool init and `Server::builder()`, add:

```rust
match engine.workspace_manager().startup_reconcile().await {
    Ok(n) => tracing::info!(stranded = n, "startup_reconcile complete"),
    Err(e) => {
        tracing::error!(error = %e, "startup_reconcile failed — refusing to start");
        std::process::exit(1);
    }
}
```

(Fail-fast on reconcile error: a bad DB is not something we want to paper over at boot.)

- [ ] **Step 3: Build + smoke test**

```bash
cargo build -p dk-server
DATABASE_URL=… cargo run -p dk-server -- --help
```

- [ ] **Step 4: Commit**

```bash
git add crates/dk-server/src/main.rs
git commit -m "feat(server): run startup_reconcile before accepting RPCs"
```

---

## Task 19: `dk-cli admin abandon` subcommand

**Files:**
- Create: `crates/dk-cli/src/commands/admin.rs`
- Modify: `crates/dk-cli/src/commands/mod.rs`
- Modify: `crates/dk-cli/src/cli.rs` (or wherever the top-level clap tree lives — search with `grep -n "Subcommand" crates/dk-cli/src/**/*.rs`)

- [ ] **Step 1: Define the subcommand shell**

Create `crates/dk-cli/src/commands/admin.rs`:

```rust
use clap::{Args, Subcommand};
use uuid::Uuid;

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Force-abandon a stranded workspace (operator escape hatch).
    Abandon {
        #[arg(long)]
        session_id: Uuid,
        #[arg(long, default_value = "admin-cli")]
        operator: String,
    },
}

pub async fn run(args: AdminArgs) -> anyhow::Result<()> {
    match args.command {
        AdminCommand::Abandon { session_id, operator } => {
            // Call gRPC or direct engine path. For admin CLI we typically
            // talk to the local engine through an admin JWT; the exact wire
            // depends on how other admin commands auth.
            admin_abandon(&session_id, &operator).await
        }
    }
}

async fn admin_abandon(session_id: &Uuid, operator: &str) -> anyhow::Result<()> {
    // Load the standard CLI client config (server endpoint + token file), same
    // path as `dk status` uses. Admin JWT must have the "admin" scope — the
    // server-side handle_abandon special-cases admin tokens below (Task 15b).
    let mut client = crate::client::connect_with_admin_jwt().await?;
    let mut req = tonic::Request::new(dk_protocol::AbandonRequest {
        session_id: session_id.to_string(),
    });
    req.metadata_mut().insert("dk-admin-operator", operator.parse()?);
    let resp = client.abandon(req).await?.into_inner();
    println!(
        "Abandoned session {} (changeset {}, reason={})",
        session_id, resp.changeset_id, resp.abandoned_reason
    );
    Ok(())
}
```

- [ ] **Step 2: Wire into the CLI's top-level subcommand enum**

Find the top-level `Commands` enum in the CLI and add:

```rust
/// Administrative commands (require admin JWT).
Admin(crate::commands::admin::AdminArgs),
```

Dispatch to `admin::run(args).await` in the runner.

- [ ] **Step 3: Add `connect_with_admin_jwt` client helper**

If it doesn't already exist, add a small helper (e.g. `crates/dk-cli/src/client.rs`) that loads the standard CLI token file, verifies the "admin" scope, and returns a connected `AgentServiceClient`. Pattern-match `dk status`'s client construction.

- [ ] **Step 4: Extend `handle_abandon` for admin callers (server side)**

Edit `crates/dk-protocol/src/abandon.rs` (from Task 15) to honor an admin branch:

```rust
let is_admin = server
    .authorize_admin_from_context()
    .map(|_| true)
    .unwrap_or(false);
let operator = req_metadata_get(&req, "dk-admin-operator").unwrap_or_default();

// Skip owner check when admin token is present.
if !is_admin && orig_agent != caller_agent {
    return Err(Status::unauthenticated(format!(
        "abandon requires original agent_id '{orig_agent}'"
    )));
}

let reason = if is_admin {
    AbandonReason::Admin { operator: operator.to_string() }
} else {
    AbandonReason::Explicit { caller: caller_agent }
};
```

Mirror whatever admin-JWT extraction the server already has (search `authorize_admin` — if none exists today, add a thin check that verifies a scope claim in the JWT).

- [ ] **Step 4: Build + smoke test + commit**

```bash
cargo build -p dk-cli
cargo run -p dk-cli -- admin abandon --help
cargo clippy -p dk-cli -p dk-protocol -- -D warnings
git add crates/dk-cli/src/commands/admin.rs crates/dk-cli/src/commands/mod.rs crates/dk-cli/src/cli.rs \
        crates/dk-protocol/src/abandon.rs
git commit -m "feat(cli): dk admin abandon — operator escape hatch for stranded workspaces"
```

---

## Task 20: Metrics

**Files:**
- Modify: `crates/dk-protocol/src/metrics.rs`

- [ ] **Step 1: Add new metric constants + increment helpers**

Following PR #74's pattern (`incr_locks_released_on_submit`), add:

```rust
static WORKSPACE_PINNED: once_cell::sync::Lazy<IntCounterVec> = once_cell::sync::Lazy::new(|| {
    register_int_counter_vec!(
        "dkod_workspace_pinned_total",
        "Workspaces skipped by GC due to pin rule",
        &["reason"]
    ).unwrap()
});

static WORKSPACE_STRANDED: once_cell::sync::Lazy<IntCounterVec> = once_cell::sync::Lazy::new(|| {
    register_int_counter_vec!(
        "dkod_workspace_stranded_total",
        "Workspaces transitioned to stranded",
        &["reason"]
    ).unwrap()
});

static WORKSPACE_RESUMED: once_cell::sync::Lazy<IntCounterVec> = once_cell::sync::Lazy::new(|| {
    register_int_counter_vec!(
        "dkod_workspace_resumed_total",
        "Resume attempts by outcome",
        &["outcome"]
    ).unwrap()
});

static WORKSPACE_ABANDONED: once_cell::sync::Lazy<IntCounterVec> = once_cell::sync::Lazy::new(|| {
    register_int_counter_vec!(
        "dkod_workspace_abandoned_total",
        "Workspaces abandoned",
        &["reason"]
    ).unwrap()
});

static WORKSPACE_STRANDED_ACTIVE: once_cell::sync::Lazy<IntGauge> = once_cell::sync::Lazy::new(|| {
    register_int_gauge!("dkod_workspace_stranded_active", "Rows where stranded_at IS NOT NULL AND abandoned_at IS NULL").unwrap()
});

pub fn incr_workspace_pinned(reason: &str) { WORKSPACE_PINNED.with_label_values(&[reason]).inc(); }
pub fn incr_workspace_stranded(reason: &str) { WORKSPACE_STRANDED.with_label_values(&[reason]).inc(); }
pub fn incr_workspace_resumed(outcome: &str) { WORKSPACE_RESUMED.with_label_values(&[outcome]).inc(); }
pub fn incr_workspace_abandoned(reason: &str) { WORKSPACE_ABANDONED.with_label_values(&[reason]).inc(); }
pub fn set_workspace_stranded_active(n: i64) { WORKSPACE_STRANDED_ACTIVE.set(n); }
```

- [ ] **Step 2: Wire increment calls**

Call `incr_workspace_stranded(reason.as_str())` inside `strand` just before the `self.workspaces.remove(session_id)`. Call `incr_workspace_resumed("ok"|"contended"|"already_resumed"|"abandoned")` at each return path of `resume`. Call `incr_workspace_abandoned(reason.as_str())` inside `abandon_stranded`. Call `incr_workspace_pinned(...)` inside `should_pin` when it returns true.

- [ ] **Step 3: Wire the `stranded_active` gauge**

Update the gauge from the GC loop after each `sweep_stranded` call:

```rust
let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspaces WHERE stranded_at IS NOT NULL AND abandoned_at IS NULL")
    .fetch_one(pool).await.unwrap_or(0);
crate::metrics::set_workspace_stranded_active(active);
```

- [ ] **Step 4: Build + commit**

```bash
cargo build -p dk-protocol -p dk-engine
cargo clippy -p dk-protocol -p dk-engine -- -D warnings
git add crates/dk-protocol/src/metrics.rs crates/dk-engine/src/workspace/session_manager.rs
git commit -m "feat(metrics): workspace pinned/stranded/resumed/abandoned counters + stranded_active gauge"
```

---

## Task 21: Increase idle TTL default to 60 min

**Files:**
- Modify: wherever `idle_ttl` is passed to `gc_expired_sessions_async` (search with `grep -rn "idle_ttl\|gc_expired_sessions" crates/`)

- [ ] **Step 1: Locate the caller**

Typically `crates/dk-engine/src/workspace/mod.rs` or a GC worker module holds the periodic task.

- [ ] **Step 2: Change constant from 30 min → 60 min**

```rust
const IDLE_TTL: Duration = Duration::from_secs(60 * 60);   // was 30 * 60
const MAX_TTL:  Duration = Duration::from_secs(4 * 3600);  // unchanged
```

- [ ] **Step 3: Sanity-run existing session_gc tests**

```bash
DATABASE_URL=… cargo test -p dk-engine --test session_gc_test
```

- [ ] **Step 4: Commit**

```bash
git add crates/dk-engine/src/workspace/mod.rs
git commit -m "chore(engine): raise idle_ttl 30m→60m to cover slow LLM turns"
```

---

## Task 22: Rollout flag coverage

**Files:**
- Covered by Task 5 (`pin_flag_enabled`). Verify here.

- [ ] **Step 1: Add integration test asserting flag-off preserves legacy behavior**

```rust
#[sqlx::test]
async fn flag_off_evicts_even_non_terminal_workspaces(pool: PgPool) {
    std::env::set_var("DKOD_PIN_NONTERMINAL", "0");
    let mgr = WorkspaceManager::new(pool.clone());
    let session_id = insert_workspace_with_changeset(&pool, ChangesetState::Submitted).await;
    // Populate in-memory with old last_active
    mgr.insert_test_workspace(/* ws with last_active in past */);
    let evicted = mgr.gc_expired_sessions_async(Duration::from_secs(60), Duration::from_secs(3600)).await;
    assert_eq!(evicted, vec![session_id]);
    std::env::remove_var("DKOD_PIN_NONTERMINAL");
}
```

- [ ] **Step 2: Run + commit**

```bash
DATABASE_URL=… cargo test -p dk-engine --test session_gc_test flag_off_evicts
git add crates/dk-engine/tests/integration/session_gc_test.rs
git commit -m "test(engine): verify DKOD_PIN_NONTERMINAL=0 preserves legacy eviction"
```

---

## Task 23: Final check + PR

- [ ] **Step 1: Run full targeted test suite**

```bash
DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test \
    cargo test -p dk-engine -p dk-protocol -p dk-mcp -p dk-cli
```

Expected: all green.

- [ ] **Step 2: Run clippy across affected crates**

```bash
cargo clippy -p dk-engine -p dk-protocol -p dk-mcp -p dk-cli -p dk-server -- -D warnings
```

Expected: no warnings.

- [ ] **Step 3: `/coderabbit:review`**

Run via the Claude Code plugin:

```bash
/coderabbit:review --base main
```

Expected: 0 findings, or address each until clean. Do not skip this step — this is a Rust-heavy change and CodeRabbit will exercise it fully (unlike the earlier doc-only review).

- [ ] **Step 4: Verify CI gates locally**

```bash
diff -r proto/dkod/v1 crates/dk-protocol/proto/dkod/v1
cargo fmt --all -- --check
```

Expected: no diff; fmt clean.

- [ ] **Step 5: Push + open PR**

```bash
git push -u origin feat/eviction-recovery
gh pr create --title "feat(engine): Epic B — workspace eviction recovery" --body "$(cat <<'EOF'
## Summary
- Pins workspaces with non-terminal changesets (prevents WU-01/WU-02-class zombie-lock incidents).
- Releases locks + marks stranded_at on unavoidable evictions (pod restart, crash).
- Adds `dk_connect{resume_session_id}` overlay rehydration + `dk_abandon` RPC.
- Auto-abandons stranded workspaces at 4 h; admin CLI escape hatch.
- Closes the operational `DKOD_API_KEY`-required lock-clearing gap.

Implements: docs/superpowers/specs/2026-04-18-workspace-eviction-recovery-design.md

## Test plan
- [ ] DATABASE_URL=... cargo test -p dk-engine --test eviction_recovery_test
- [ ] DATABASE_URL=... cargo test -p dk-engine --test session_gc_test
- [ ] cargo clippy -p dk-engine -p dk-protocol -p dk-mcp -p dk-cli -p dk-server -- -D warnings
- [ ] /coderabbit:review --base main → 0 findings
- [ ] Manual: simulate eviction with DKOD_IDLE_TTL=1s; verify stranded row + resume flow
- [ ] Manual: pod restart (kill dk-server mid-session) → startup_reconcile strands → resume works

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 6: Address PR-side CodeRabbit review**

Wait for CodeRabbit's automated PR review. Fix every finding, push, wait for next review, iterate until clean. Only merge when CodeRabbit has no remaining issues (global CLAUDE.md rule).

---

## Out of scope for this plan (tracked separately)

- **Epic A — platform schema repair.** `migrations/015_changeset_parent_changeset_id.sql` is present in this repo but did not run on the hosted dkod.io platform. The platform owner must apply the migration; no engine code can fix it.
- **Epic C — UI for distinguishing stranded vs AST conflict.** Once this plan ships, the platform and dkod-app can consume the new `session.stranded` / `session.resumed` / `session.abandoned` events and surface a distinct badge. Separate spec + plan.
- **30-day tombstone prune sweep.** Deferred per spec §Rollout "until bloat is measured."

## References

- Spec: `docs/superpowers/specs/2026-04-18-workspace-eviction-recovery-design.md`
- PR #74 pattern (release-locks-at-submit): `crates/dk-protocol/src/submit.rs:363-381`
- Feb 2026 resume declaration: `docs/plans/2026-02-28-protocol-v01-completion-design.md:66-70`
- Existing GC harness: `crates/dk-engine/src/workspace/session_manager.rs:290-315`

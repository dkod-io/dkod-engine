# Workspace eviction recovery (Epic B)

**Status:** design approved, pending implementation plan
**Date:** 2026-04-18
**Scope:** `dkod-engine` only
**Related work:**
- PR #74 — release-locks-at-submit (symmetric lock release is reused here)
- `docs/plans/2026-02-28-protocol-v01-completion-design.md` §Session Resume (declares `resume_session_id`, never wired)

## Problem

When a session workspace is evicted in the middle of in-flight work, the engine silently drops the in-memory state. The persisted overlay in `session_overlay_files` is orphaned, symbol locks are leaked, and downstream consumers (platform state machine, UI, merge gates) have no signal to distinguish "real AST conflict" from "backing session died."

Observed failure, 2026-04-18 `/dkh` session on `dkod-io/project-management-demo`:
- WU-01 and WU-02 workspaces were evicted mid-flight; locks leaked ("zombie locks").
- WU-02's changeset surfaced as `conflicted:awaiting_user` in the platform UI with no `conflict_details`, even though no same-symbol AST overlap existed (investigation: only eviction + HEAD advancement by sibling WUs).
- Recovery required `DKOD_API_KEY`-gated admin tooling that the harness didn't have.

The failure pattern reproduces whenever (a) GC's `idle_ttl` (30 min) fires during a slow LLM turn, (b) `cleanup_disconnected` runs between agent reconnects, or (c) the pod restarts without a graceful teardown.

## Non-goals

- **Platform DB schema repair** (Epic A — `parent_changeset_id` drift). Separate work, hosted-platform repo.
- **Conflict-UX redesign** (Epic C). Downstream of this spec: once Epic B emits a distinct stranded/resumable signal, the UI can map it to a different badge than AST conflicts.
- **Replacing the AST merge pipeline.** `MergeAnalysis::{AutoMerge, Conflict}` stays as-is.
- **Changeset state machine changes.** The existing states (`submitted → verifying → approved → merged / rejected / closed`, per `crates/dk-engine/src/changeset.rs`) are unchanged. Abandon reuses `rejected` with a `reject_reason` tag — no new state is introduced.

## Policy decisions (locked)

### 1. Hybrid prevent-and-strand

Prevent the easy evictions with a pin rule. For the irreducible cases (pod restart, true crash), produce an explicit stranded state with a caller-driven recovery RPC. No transparent auto-resume — failures must be observable.

### 2. Pin rule

A workspace is **pinned** (exempted from `cleanup_disconnected` and `gc_expired_sessions`) iff its associated changeset's `state` is in a **non-terminal** set: `state NOT IN {merged, rejected, closed}`. Non-terminal states pinned: `submitted`, `verifying`, `approved`.

Rationale: the in-memory state is load-bearing for any changeset that hasn't reached a terminal state — exactly the window where eviction hurts. Using changeset state as the criterion survives session reconnects and avoids fragile signals like overlay emptiness or `last_active`.

`idle_ttl` is raised from 30 min → **60 min** as a safety margin for slow LLM turns.

### 3. Eviction path (rare cases where the pin doesn't apply)

When an eviction does fire — because the changeset reached a terminal state just before GC, or because of a controlled shutdown, or because `startup_reconcile` found an orphan after a crash — the workspace is **stranded**, not deleted:

1. `UPDATE workspaces SET stranded_at = NOW(), stranded_reason = $1 WHERE session_id = $2`.
2. Call `release_locks_and_emit` (existing helper from PR #74) so sibling agents unblock immediately.
3. Emit `session.stranded` watch event.
4. Drop the in-memory workspace entry.

### 4. Startup reconciliation

On `dk-server` boot, before accepting RPCs, run `startup_reconcile`: find workspaces whose session has no in-memory counterpart (empty set at boot), whose changeset is non-terminal, and whose `stranded_at IS NULL`. Strand them. This covers the crash-before-teardown path that the in-process eviction hook can't handle.

### 5. Resume

Resume is triggered by `dk_connect { resume_session_id }` — a field already declared in the protocol (Feb 2026 spec) but never wired end-to-end. Caller-driven; the platform does NOT auto-resume on behalf of the caller (no credential ownership).

Behavior:
- `SELECT … FROM workspaces WHERE session_id = $1 FOR UPDATE` + precondition `stranded_at IS NOT NULL` + changeset non-terminal.
- `base_commit` is **preserved** — no rebase to current HEAD on resume. Divergence is surfaced through the normal `PreSubmitCheck` / merge-time AST path.
- Overlay restored via the existing `FileOverlay::restore_from_db`.
- Semantic graph re-indexed from the restored overlay.
- Locks **eagerly re-acquired** for every symbol in the restored changed-set via `claim_tracker.acquire`. Any contention → transaction rolls back, old row stays stranded, caller gets `RESUME_CONTENDED { conflicting_symbols, claimants }`.
- Second resumer finds `stranded_at IS NULL` under `FOR UPDATE` and gets `ALREADY_RESUMED { new_session_id }`.
- Authorization: the original agent's credentials (verified against the stranded session's `agent_id`). **Not admin-gated** — this is the explicit fix for the current `DKOD_API_KEY` operational gap.

### 6. Abandon

Two paths, identical cleanup, different triggers:

- **Auto-abandon** after `stranded_ttl = 4 hours` (matches current `max_ttl`). Ridden on the existing GC sweep. Reason: `ttl`.
- **Explicit `dk_abandon { session_id }`** RPC — owner-authorized, not admin-only. Reason: `caller`.
- **Admin override** — `dk-cli admin abandon --session-id`, gated by admin JWT scope. The only admin surface in the design; escape hatch for bugs.

`abandon_stranded` cleanup sequence (idempotent):
1. `release_locks_and_emit` (no-op if already released).
2. `UPDATE changesets SET state='rejected', reject_reason='session_abandoned'`.
3. `DELETE FROM session_overlay_files WHERE workspace_id = ?`.
4. Emit `session.abandoned` watch event.
5. Tombstone the workspace row: `UPDATE workspaces SET abandoned_at = NOW(), abandoned_reason = ?`. Row retained for audit; pruned by a separate 30-day sweep if bloat materializes.

## Data model

### Migration

```sql
ALTER TABLE workspaces
    ADD COLUMN stranded_at       TIMESTAMPTZ,
    ADD COLUMN stranded_reason   TEXT,
    ADD COLUMN abandoned_at      TIMESTAMPTZ,
    ADD COLUMN abandoned_reason  TEXT,
    ADD COLUMN superseded_by     UUID REFERENCES workspaces(session_id);

CREATE INDEX idx_workspaces_stranded_at
    ON workspaces (stranded_at) WHERE stranded_at IS NOT NULL;
```

`stranded_at` + `superseded_by` together encode the full lifecycle: `live → stranded → (resumed|abandoned)`.

### Watch events

- `session.stranded { session_id, changeset_id, base_commit, stranded_reason }`.
- `session.resumed { old_session_id, new_session_id, changeset_id }`.
- `session.abandoned { session_id, changeset_id, abandoned_reason }`.

All consumed by the platform UI and by sibling-agent `dk_watch` subscribers that need to know a lock was released.

## Components (file-level)

All changes in `dkod-engine`. Proto updates must stay synced between `proto/dkod/v1/` and `crates/dk-protocol/proto/dkod/v1/` per CLAUDE.md.

### `crates/dk-engine/src/workspace/session_manager.rs` (edit)

New methods:
- `should_pin(&self, session_id) -> bool` — indexed `changesets.state` lookup.
- `strand(&self, session_id, reason: StrandReason) -> Result<()>` — the rare-path replacement for `.remove()`. Idempotent.
- `abandon_stranded(&self, session_id, reason: AbandonReason) -> Result<()>` — sweep + explicit path.
- `resume(&self, dead_session_id, new_session_id, agent_creds) -> Result<ResumeResult>` — transactional.
- `startup_reconcile(&self) -> Result<usize>` — boot-time orphan sweep.

Existing methods edited:
- `gc_expired_sessions`, `cleanup_disconnected` gain a `should_pin` guard: non-pinnable → `strand`; pinnable → skip.

New enum types:
- `StrandReason { IdleTtl, CleanupDisconnected, StartupReconcile, Explicit }`.
- `AbandonReason { AutoTtl, Explicit { caller: AgentId }, Admin { operator: String } }`.
- `ResumeResult::{Ok(SessionWorkspace), Contended(Vec<ConflictingSymbol>), AlreadyResumed(SessionId), Abandoned}`.

### `crates/dk-engine/src/workspace/overlay.rs` (minor edit)

- Add `FileOverlay::drop_for_workspace(db, workspace_id) -> Result<()>` for `abandon_stranded`.
- `restore_from_db` exists — no change.

### `crates/dk-protocol/src/server.rs` + each RPC handler (edit)

Hoist a `require_live_session` middleware check. Every RPC (`dk_submit`, `dk_file_write`, `dk_merge`, `dk_verify`, `dk_context`, `dk_file_read`, `dk_file_list`, `dk_status`, `dk_review`, `dk_approve`, `dk_resolve`, `dk_push`, `dk_close`) calls it first. Returns:
- Workspace in-memory → proceed.
- Workspace missing + `stranded_at IS NOT NULL` → `SESSION_STRANDED { changeset_id, base_commit, stranded_at, reason }`.
- Workspace missing + `abandoned_at IS NOT NULL` → `SESSION_ABANDONED { changeset_id, reject_reason }`.
- Not found (never existed) → existing behavior.

### `crates/dk-protocol/proto/dkod/v1/agent.proto` (edit — keep copy in sync)

- Wire `ConnectRequest.resume_session_id` end-to-end.
- Add `AbandonRequest { session_id }` + `AbandonResponse { success, changeset_id }`.
- Error detail messages: `SessionStranded`, `ResumeContended`, `AlreadyResumed`, `SessionAbandoned`.

### `crates/dk-mcp/src/server.rs` (edit)

- Expose `dk_abandon { session_id }` MCP tool.
- `dk_connect` MCP tool gains optional `resume_session_id` arg.

### `crates/dk-server/src/main.rs` (edit)

- Call `WorkspaceManager::startup_reconcile()` between DB-pool init and `tonic::Server::serve`. Fail boot on error (no partial recovery — operator triage).

### `crates/dk-engine/src/metrics.rs` (edit)

New counters/gauges (following PR #74's convention):
- `dkod_workspace_pinned_total{reason}` counter.
- `dkod_workspace_stranded_total{reason}` counter.
- `dkod_workspace_resumed_total{outcome}` counter.
- `dkod_workspace_abandoned_total{reason}` counter.
- `dkod_workspace_stranded_active` gauge.

### `crates/dk-cli/src/commands/` (new)

- `dk-cli admin abandon --session-id <uuid> [--reason <text>]` — admin escape hatch, admin JWT scope required.

## Data flows

### Flow A — pinned workspace (90% of traffic)

`gc_expired_sessions` / `cleanup_disconnected` iterate candidates. For each:
- `should_pin(session_id)` → `SELECT state FROM changesets WHERE session_id=$1` → non-terminal → skip (pin, keep alive).
- Terminal → evict as today.

### Flow B — graceful eviction → strand → resume

1. `strand(S1, reason)`: set `stranded_at`, release locks, emit `session.stranded`, drop in-memory.
2. Harness next RPC on `S1` hits middleware → `SESSION_STRANDED { … }`.
3. Harness calls `dk_connect { resume_session_id: S1, agent_creds }`:
   - Transaction: `SELECT … FOR UPDATE`, preconditions, rehydrate overlay, re-index graph, atomic `claim_tracker.acquire` loop.
   - On contention → rollback, `RESUME_CONTENDED { conflicting_symbols, claimants }`.
   - On already-resumed → `ALREADY_RESUMED { new_session_id }`.
   - On success: `UPDATE workspaces SET session_id=S2, stranded_at=NULL, superseded_by=S2 WHERE session_id=S1`, emit `session.resumed`.
4. Harness retries the original RPC on `S2`.

### Flow C — crash eviction → startup_reconcile → (Flow B or Flow D)

On boot: `startup_reconcile` finds orphaned workspaces with non-terminal changesets, calls `strand(_, StartupReconcile)` for each. From there the resume/abandon paths are identical to Flow B/D.

### Flow D — abandon

- Auto: sweep reuses GC tick, finds `stranded_at + 4h < NOW()`, calls `abandon_stranded(_, AutoTtl)`.
- Explicit: `dk_abandon { session_id }` authorizes the caller against the stranded session's `agent_id`, calls `abandon_stranded(_, Explicit)`.
- Admin: `dk-cli admin abandon` bypasses agent authorization, calls `abandon_stranded(_, Admin)`.

Cleanup: release locks (idempotent), reject changeset, delete overlay rows, tombstone workspace.

## Error handling

| Code | Trigger | Payload | Caller action |
|---|---|---|---|
| `SESSION_STRANDED` | RPC middleware: workspace missing + `stranded_at IS NOT NULL` | `{changeset_id, base_commit, stranded_at, reason}` | Call `dk_connect{resume_session_id}` or `dk_abandon`. |
| `RESUME_CONTENDED` | Lock re-acquire during `resume` fails | `{conflicting_symbols: [{name, claimant_session, claimant_agent}]}` | Wait on `dk_watch` for release then retry, or abandon. |
| `ALREADY_RESUMED` | Second resumer finds `stranded_at IS NULL` under `FOR UPDATE` | `{new_session_id}` | Adopt new session if owner; else abandon locally. |
| `SESSION_ABANDONED` | Resumer finds changeset `rejected` (sweep raced) | `{changeset_id, reject_reason}` | Start a fresh changeset. |
| `UNAUTHENTICATED` | Resume/abandon caller's `agent_id` ≠ stranded session's `agent_id` | (existing) | Only owner can resume/abandon. Admin override available. |

All four new codes are structured gRPC statuses with proto-defined detail messages — no string parsing required in the harness.

### Edge cases

1. **Race between `dk_abandon`, `stranded_sweep`, and `resume`:** `FOR UPDATE` + precondition checks serialize. First writer wins; losers receive `ALREADY_RESUMED` or `SESSION_ABANDONED`.
2. **Resume succeeds but harness crashes before next RPC:** new session `S2` is pinned (non-terminal changeset). Normal idle-TTL + pin rules apply. No special-casing.
3. **Overlay rehydrates but graph re-index fails** (parser error, stale content): rollback transaction, old row stays stranded with `reason=resume_failed`. Auto-abandon catches it at 4h.
4. **Credentials on resume don't match stranded session's `agent_id`:** standard `UNAUTHENTICATED`.
5. **Admin force-clean:** `dk-cli admin abandon` is the only admin-gated path; exists as a bug escape hatch and does not replace the owner path.

## Metrics

- `dkod_workspace_pinned_total{reason}` — every `should_pin()=true`.
- `dkod_workspace_stranded_total{reason ∈ {idle_ttl, cleanup_disconnected, startup_reconcile, unpinnable}}`.
- `dkod_workspace_resumed_total{outcome ∈ {ok, contended, already_resumed, abandoned}}`.
- `dkod_workspace_abandoned_total{reason ∈ {auto_ttl, explicit, admin}}`.
- `dkod_workspace_stranded_active` — gauge of rows where `stranded_at IS NOT NULL AND abandoned_at IS NULL`.

## Testing

Per memory, scope tests to `-p dk-engine` / targeted `--test` — never `--workspace`.

### Unit

- `session_manager`:
  - `should_pin` truth table across all `changesets.state` values.
  - `strand` idempotency under repeated calls.
  - `abandon_stranded` idempotency under concurrent calls (4 tokio tasks on the same `session_id`).
- `overlay`:
  - `restore_from_db` + `drop_for_workspace` round trip.

### Integration — `tests/integration/eviction_recovery_test.rs` (new, requires `DATABASE_URL`)

- **Pin path:** workspace with submitted changeset survives `gc_expired_sessions` at `idle_ttl=1s`.
- **Strand path:** mark changeset terminal mid-flight → GC evicts → `stranded_at` set → mock RPC returns `SESSION_STRANDED`.
- **Resume happy:** strand → resume → overlay + graph + locks reconstructed; original submit re-tried succeeds.
- **Resume contended:** strand session A; session B acquires one of A's symbols; A's resume returns `RESUME_CONTENDED` and row stays stranded.
- **Double-resume:** two concurrent `dk_connect{resume_session_id}` calls; one succeeds, the other gets `ALREADY_RESUMED`.
- **Auto-abandon:** set `stranded_at = NOW() - 5h`, tick sweep, assert changeset `rejected` and overlay deleted.
- **Explicit abandon:** `dk_abandon` from original agent succeeds; from another agent returns `UNAUTHENTICATED`.
- **Startup reconcile:** insert workspace + non-terminal changeset with no live session; run reconcile; assert stranded.

### Regression

- Existing AST merge tests stay green — this design does not touch `ast_merge` semantics.

## Rollout

One flag for rollback, matching PR #74's pattern:
- `DKOD_PIN_NONTERMINAL` (default `1`). Set to `0` to revert to unconditional eviction. Ops toggle if pin-aggression causes a regression.

No flag for strand/resume/abandon — additive surface (new columns, new RPCs). They only fire when an eviction actually occurs; existing code paths are unaffected when no eviction is happening.

Migration ordering (single deploy):
1. Apply schema migration (additive columns, online).
2. Deploy engine with `DKOD_PIN_NONTERMINAL=1`.
3. Startup reconciliation runs on first boot; existing orphaned workspaces get stranded immediately (they'll hit auto-abandon at 4h if no harness recovers them).

## Open items

- Stored-row bloat: tombstones retained indefinitely. If observed bloat, add a 30-day sweep that `DELETE`s workspaces with `abandoned_at < NOW() - 30d`. Deferred until measured.
- Platform-layer mapping: Epic C will translate `session.stranded` + `SESSION_STRANDED` into a distinct UI badge (not "Conflict"). Out of scope for this spec.

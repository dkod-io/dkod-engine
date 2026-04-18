# Release symbol locks at `dk_submit` + stacked changeset model

**Status**: proposal
**Owner**: engine team
**Date**: 2026-04-17
**Related**: `crates/dk-protocol/src/merge.rs` (`release_locks_and_emit`), `crates/dk-protocol/src/submit.rs` (`handle_submit`), `crates/dk-protocol/src/file_write.rs` (`handle_file_write`), `crates/dk-protocol/src/file_read.rs` (`handle_file_read`)

---

## Problem

Today, symbol locks acquired via `dk_file_write` are released only at merge time:

```rust
// crates/dk-protocol/src/merge.rs
pub async fn handle_merge(...) {
    ...
    release_locks_and_emit(server, repo_id, sid, &req.session_id).await; // lines 96, 131
}
```

In a typical parallel build the merge moment is **far** from the submit moment. A single changeset traverses `submit → verify → dk_review (local, tier=local) → dk_review (deep, tier=deep) → approve → merge`. With deep-LLM review enabled (DKOD_CODE_REVIEW=1) and the LAND-phase fix-loop (up to 3 fix rounds) the end-to-end landing window is **1–5 minutes per changeset** in practice.

Any other agent that tried `dk_file_write` on the same symbol is now sitting in:

```
SYMBOL_LOCKED — write rejected
  1. dk_watch(filter: "symbol.lock.released", wait: true)
```

— but `dk_watch` has a default `timeout_ms = 30_000` and a hard max of `120_000`. The release doesn't come in time; the wait times out; the agent either retries (another `SYMBOL_LOCKED`), gives up and submits a stale overlay (→ `true_conflict` at merge), or both.

Reproduced experimentally with two concurrent sessions on `dkod-io/project-management-demo`:

| Step | Result |
|---|---|
| A `dk_file_write(Foo)` | OK, symbol claimed |
| B `dk_file_write(Foo)` | `SYMBOL_LOCKED`, correct recovery instructions |
| A `dk_submit` | `changeset.submitted` broadcast; **no `symbol.lock.released`**; locks still held |
| B `dk_watch` | `No new watch events` — waiting indefinitely |
| B `dk_file_write(Foo)` retry | still `SYMBOL_LOCKED` |
| A `dk_verify` → `dk_approve` → `dk_merge` | `symbol.lock.released` + `changeset.merged` broadcast |

So the engine works correctly; the *hold window* is the problem.

`dk_verify` is currently a stub (`verification-disabled` step) so releasing at verify is not meaningful.

## Goal

Reduce the lock hold window from "minutes (until merge)" to "seconds (until submit)". Enable true parallelism between agents without breaking merge correctness.

## Design

### Release semantics

Move `release_locks_and_emit()` from `handle_merge` to `handle_submit`. On a successful `dk_submit`:

1. Persist the changeset overlay immutably under `changeset_id`.
2. **Release all symbol locks** held by that session against that changeset.
3. Emit `symbol.lock.released` with `source_session`, `changeset_id`, affected files, affected symbols.
4. Also emit `changeset.submitted` (already done today).

On `dk_merge` (kept for safety + cleanup), emit `symbol.lock.released` only for locks the session *still holds* (i.e., locks from post-submit amendments not yet released). The helper is idempotent.

On `dk_close`, keep the release (already implemented), but additionally emit `symbol.lock.released` — today the release is silent on close, which is also a gap (observed in testing).

### Stacked-changeset semantics

A submitted changeset is now a dependency edge. The next writer stacks on top of it.

Add to the `Changeset` record (whatever table/proto type holds it):

```rust
pub struct Changeset {
    pub id: ChangesetId,
    pub repo_id: RepoId,
    pub session_id: SessionId,
    pub base_version: CommitSha,          // main branch SHA at connect time
    pub parent_changeset_id: Option<ChangesetId>, // NEW — stacks on this
    pub state: ChangesetState,            // Submitted | Verified | Approved | Merged | Closed
    // ...
}
```

When session B's first `dk_submit` happens at a point where session A already has a submitted (but not-yet-merged) changeset `A_cs` that modified symbols B's overlay depends on, the engine sets `B_cs.parent_changeset_id = A_cs.id`. The dependency graph is a forest (usually shallow chains).

`dk_file_read(path, session=B)` resolves to:

```
base_commit_blob(base_version, path)
  + chain_overlay(visible_chain, path)   // submitted ancestors B depends on, in order
  + session_overlay(B, path)             // B's own uncommitted writes
```

Where `visible_chain` is any submitted changeset that B either stacks on (via `parent_changeset_id` walk) OR that modified a file B has read/written since `dk_connect`. "Visibility" is conservative: if in doubt, include it.

### `dk_file_write` semantics (unchanged)

`dk_file_write` already acquires per-symbol locks via the claim tracker. No change needed. Now, because A released locks at submit, the race window between A submitting and B writing becomes milliseconds instead of minutes, and the overwhelming majority of B's writes will simply succeed.

When B *does* collide with a still-in-flight symbol (A is still in the `{writing, not-yet-submitted}` window), B gets `SYMBOL_LOCKED` as today. Recovery instructions stay identical; wait times are now seconds.

### Review-fix without amendment

Current behavior: the generator's review-fix loop calls `dk_file_write` + `dk_submit` on the *same* changeset, replacing its overlay. Under the new model, **amending a submitted changeset is disallowed** (overlay is frozen). The generator instead:

1. Reads current chain state (which may include other agents' submitted work on top of A).
2. Writes the fix as a *new* changeset that stacks on the chain tip.
3. Submits the new changeset.

At engine level this is trivial — each `dk_submit` after the session's first just creates a new changeset whose `parent_changeset_id` is the session's prior submitted changeset. The harness needs a prompt update (see §Harness).

If the generator tries to re-`dk_file_write` against a symbol it already released (at its previous submit), the engine returns `SYMBOL_LOCKED` with the usual recovery instructions. If no other session has claimed it, the generator re-acquires cleanly and proceeds.

### Merge ordering

`dk_merge` now requires `parent_changeset_id`'s state to be `Merged` (or `None`). If the parent isn't merged yet, return:

```
MERGE_BLOCKED — parent changeset <id> has state=<Submitted|Verified|Approved>.
Wait for parent to merge: dk_watch(filter: "changeset.merged", wait: true, timeout_ms: ...)
```

Parents merge first; children follow automatically. This gives the engine a clean linearization without introducing explicit locks on the merge path.

If a parent fails merge (e.g., AST merger can't reconcile against the latest main), the parent is marked `MergeFailed`. The engine emits `changeset.parent_rollback_invalidated` for every direct + transitive child. Child sessions receive it via `dk_watch` and must either:
- `dk_close` and start fresh, or
- Auto-rebase onto the new main and re-submit (if the engine supports it; stretch goal).

### Chained `dk_file_read` — implementation

Most of this is already factored — `dk_file_read` today resolves `base + session_overlay`. Extend the overlay-resolution function to accept a chain:

```rust
fn resolve_overlay(
    base_blob: Option<Bytes>,
    chain: &[ChangesetOverlay],   // NEW — submitted ancestors, ordered parent→child
    session: &SessionOverlay,
    path: &str,
) -> Bytes;
```

Chain resolution is last-write-wins per line/symbol boundary, using the existing AST-merge primitives. Walk the chain in order, applying each overlay's writes to `path`; then apply the session's own overlay on top.

The set of changesets in `chain` for session B is:

```
visible(B) = ancestors(B.parent_changeset_id) ∪ {cs : cs.state ∈ {Submitted, Verified, Approved} ∧ cs.modifies(files_read_or_written_by(B))}
```

Cache aggressively. Invalidate on any `changeset.submitted` / `changeset.amended` event for any changeset in the visible set.

### Event shape (additions)

```
symbol.lock.released               // now fires at submit, merge, AND close
  source_session, session_name, changeset_id, files[], symbols[]

changeset.submitted                // unchanged payload
  changeset_id, author_id, files[], symbols[], parent_changeset_id  // NEW field

changeset.parent_rollback_invalidated   // NEW
  changeset_id, affected_sessions[], reason

// (Not required for phase 1, nice-to-have)
symbol.lock.acquired               // NEW — emitted on successful dk_file_write lock claim
  source_session, session_name, files[], symbols[]
```

### MCP-plugin messaging (dk-mcp)

`crates/dk-mcp/src/server.rs` line 1523 already instructs:

```
1. dk_watch(filter: "symbol.lock.released", wait: true)
```

Keep that. Also update the response the user sees on `SYMBOL_LOCKED` to explicitly call out short waits:

```
Lock typically releases within seconds now (held for the duration of another agent's
`dk_file_write` cycle, released on their next `dk_submit`). Safe to wait with
timeout_ms: 30000.
```

And on `dk_submit`, append to the success response:

```
Locks released: <N> symbols across <M> files. Other sessions watching will unblock.
```

So generators have a clear cue that their submit actually released contention — surfaces the behavior change for debugging.

## Implementation steps

1. **Move `release_locks_and_emit()` call site** — `crates/dk-protocol/src/submit.rs::handle_submit`. On the success path, after persisting the changeset and before returning. Leave the existing call in `handle_merge` idempotent (so post-submit locks from amendments still get cleaned up if a generator managed to acquire more).
2. **Add `symbol.lock.released` emission to `handle_close`** (current: releases silently).
3. **Add `parent_changeset_id: Option<ChangesetId>` to the changeset record** + proto.
4. **Submit handler computes parent** — at submit time, query the session's visible chain tip (most recent ancestor the session has read from). Simplest v1: `parent_changeset_id = session.last_read_from_changeset_id` (tracked on `dk_file_read`). If none, `None`.
5. **Merge handler parent check** — before merging, require `parent.state == Merged || parent_id == None`. If not, return `MERGE_BLOCKED` with watch guidance.
6. **Chained `dk_file_read`** — extend overlay resolution to walk the chain (v1: only direct ancestor chain via `parent_changeset_id`; defer "transitive visibility by file overlap" to v2).
7. **Event emission** — update `symbol.lock.released` to fire on submit; add `changeset.submitted.parent_changeset_id`; add `changeset.parent_rollback_invalidated`.
8. **MCP response text** — update `dk_file_write` `SYMBOL_LOCKED` message + `dk_submit` success message.
9. **Tests**:
   - `crates/dk-engine/tests/integration/submit_releases_lock.rs` — two sessions, A writes + submits, B watches, B's write succeeds without lock wait.
   - `crates/dk-engine/tests/integration/stacked_merge_order.rs` — A + B submit stacked, B merge returns `MERGE_BLOCKED` until A merges.
   - `crates/dk-engine/tests/integration/parent_rollback.rs` — A fails merge, B receives `changeset.parent_rollback_invalidated`.
   - Existing `conflict_claim_test.rs` should still pass.

## Non-goals (v1)

- Transitive visibility by file-overlap (defer to v2).
- Auto-rebase on parent rollback (defer to v2; v1 invalidates and lets the harness decide).
- `symbol.lock.acquired` proactive event (nice-to-have; defer unless trivial).
- Multi-parent DAGs — v1 is strictly linear chains per session.

## Risk & rollback

Feature-flag behind `DKOD_RELEASE_ON_SUBMIT=1`. Default off for first release. Internal testbed sets it on. Once green, flip default.

Rollback is trivial: flip env and `handle_submit`'s release call becomes a no-op, `handle_merge`'s release covers everything again.

## Test plan (pre-release)

1. **Manual**: two MCP sessions on `dkod-io/project-management-demo`; repro the symptoms from the problem section; expect SYMBOL_LOCKED only during the second's `dk_file_write` race (ms), not during A's review pipeline.
2. **Harness smoke**: single-unit build — should be unchanged.
3. **Harness parallel-build**: 3-unit build that shares an aggregation file (the WU-01/02/03 repro). With flag off: reproduces today's `true_conflict` cascade. With flag on: all three land cleanly via stacked submits.

## Harness-side follow-up (tracked separately)

Not an engine change, but the flow only works end-to-end if the harness generator prompt:

- Drops "amend the same changeset" language from the review-fix loop.
- Explicitly calls out "review-fix = new stacked changeset, not amendment".
- Passes `timeout_ms: 60_000` (or 30s) to `dk_watch(filter: "symbol.lock.released", wait: true)` — much shorter waits are now realistic.
- Handles the new `MERGE_BLOCKED` response by watching for `changeset.merged` on the parent.
- Handles `changeset.parent_rollback_invalidated` by closing + re-planning the unit.

## Observability

Add two log lines (INFO) in the engine:

- `lock released on submit: session=<id> changeset=<id> symbols=<N>`
- `merge blocked: session=<id> changeset=<id> parent=<id> parent_state=<state>`

Add counter metrics:

- `dkod_engine_locks_released_on_submit_total`
- `dkod_engine_merge_blocked_on_parent_total`
- `dkod_engine_stacked_submit_depth` (histogram)

These make it easy to verify the flag is doing its job in production.

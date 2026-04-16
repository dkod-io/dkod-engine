# MCP-layer Code Review Gate — Design

**Date:** 2026-04-16
**Scope:** `dkod-engine` (gRPC + MCP), `dkod-plugin` (`/dkod:land`), `dkod-harness` (`/dkh`)
**Status:** Design approved, not yet implemented

---

## Problem

Today the dkod platform runs deep code review for submitted changesets (when an LLM key is configured via web Settings) and exposes results via `dk_review`. However, `dk_approve` is a thin gRPC forwarder with no review enforcement. The `/dkod:land` plugin command gates at score≥3, but the harness `/dkh` and other callers can skip land entirely, reaching `dk_merge` at 1/5 or 2/5 — observed repeatedly in production harness runs (see `project_merged_with_low_review_score`, `project_deep_review_not_gated` in auto-memory).

The gap: the *platform primitive* allows unreviewed approvals, and *enforcement is only at the skill/prompt layer*, which agents can drift past.

## Goals

1. Enforce a minimum deep-review score before `dk_approve` succeeds, without changing default behavior for users who don't opt in.
2. Support BYOK (bring your own key) at the client level — LLM calls happen on the user's machine, not the platform.
3. Work uniformly for self-hosted and cloud engines.
4. Give `/dkh` a deterministic backstop without duplicating enforcement logic in the harness.
5. Provide an auditable escape hatch for false-positive findings and LLM outages.

## Non-goals

1. Replacing the existing web-Settings-based review flow. Both coexist.
2. Reselling LLM minutes through the platform — BYOK stays strictly BYOK.
3. Per-seat licensing changes. Does not conflict with the current Engine (MIT) / Platform (BSL) / Managed ($9.99/mo) model because the value of the managed platform (GitHub integration, dashboard, team, hosting) is unaffected.
4. Agent-review-by-another-agent workflows (too complex for v1; GitHub PR review already fills this role).

---

## Key decisions

| # | Decision | Choice |
|---|----------|--------|
| 1 | Gate location | MCP server layer (client-side), conditional on env |
| 2 | Provider precedence | Explicit opt-in flag + provider keys; OpenRouter wins if both set; fail-closed if flag set with no key |
| 3 | Threshold | `DKOD_REVIEW_MIN_SCORE`, default 4, env-configurable |
| 4 | Approve timing on async review | Reject-if-missing; caller polls/retries |
| 5 | `/dkh` integration | Hybrid: harness owns review-fix loop + MCP gate as backstop |
| 6 | Review execution site | B1 — MCP drives the LLM call, platform stores results via new RPC |
| 7 | Failure response shape | Structured rejection with score, threshold, findings inline, next-action hint |
| 8 | Background run timing | Hybrid — fire on `dk_submit`, read/reject on `dk_approve` |
| 9 | Override path | `dk_approve(force: true, override_reason)` with audit trail |

---

## Architecture

The feature adds a **client-side code-review gate** at the MCP layer. It's entirely off by default; setting `DKOD_CODE_REVIEW=1` opts in. When enabled:

1. **MCP becomes the review driver.** After `dk_submit` succeeds, the MCP server (`dk-mcp` crate in `dkod-engine`) spawns a background `tokio::spawn` task that calls the user's configured LLM (Anthropic or OpenRouter) with the changeset diff + file context.
2. **Results flow back to the platform** via a new gRPC `RecordReview(session_id, changeset_id, tier="deep", score, findings, provider, model, duration_ms)`. The engine's existing review storage table is the sink.
3. **`dk_approve` gates on stored review.** MCP fetches via `dk_review`. If the deep tier is missing, score too low, or a provider error is recorded, MCP returns a structured rejection. Only if score ≥ threshold does it forward the `Approve` RPC to the engine.

**Key architectural property:** the engine gRPC surface is unchanged except for one new RPC (`RecordReview`). The engine's `Approve` RPC stays a dumb forwarder — no LLM dependency, no review policy. The gate is a pure MCP-client concern. This preserves BSL-platform licensing (no LLM logic server-side) and keeps non-MCP callers (web dashboard, `dk` CLI) unaffected.

**Secondary property:** `/dkh` does not need to know about the gate. It already runs a review-fix loop when it can; the MCP gate is a backstop that catches harness bugs. `/dkh`'s only change is setting the env when it launches sub-agents.

---

## Environment variable contract

Nine vars total, all prefixed `DKOD_` for consistency with existing engine conventions.

### Gate activation
- `DKOD_CODE_REVIEW` — set to `1` to enable the gate. Any other value or unset → gate off, `dk_approve` forwards straight through.

### Provider keys
- `DKOD_ANTHROPIC_API_KEY` — Anthropic API key.
- `DKOD_OPENROUTER_API_KEY` — OpenRouter API key. Takes precedence when both are set.

### Provider tuning (optional)
- `DKOD_REVIEW_MODEL` — override the model. Defaults: `claude-sonnet-4-6` for Anthropic, `anthropic/claude-sonnet-4` for OpenRouter.
- `DKOD_REVIEW_MAX_TOKENS` — response size cap. Default `4096`.
- `DKOD_OPENROUTER_BASE_URL` — override for enterprise proxies. Default `https://openrouter.ai/api/v1`.

### Gate policy
- `DKOD_REVIEW_MIN_SCORE` — minimum score to approve. Default `4`. Valid range `1-5`.
- `DKOD_REVIEW_TIMEOUT_SECS` — cap on the background review call. Default `180`.
- `DKOD_REVIEW_BACKOFF_POLICY` — behavior when LLM call fails: `strict` (gate stays closed → `dk_approve` rejects) or `degraded` (MCP records score `null` with a `provider-error` finding → gate uses local review score if ≥ threshold). Default `strict`.

### Precedence & validation (enforced at MCP startup)

1. If `DKOD_CODE_REVIEW=1` and neither provider key is set → MCP emits a startup warning to stderr; every `dk_approve` call rejects with `"code review enabled but no provider key — set DKOD_ANTHROPIC_API_KEY or DKOD_OPENROUTER_API_KEY"`. Fail-closed.
2. If both provider keys are set → OpenRouter is selected, Anthropic ignored. Logged at info.
3. Env vars are read once at MCP server startup and cached. Changing them requires restarting MCP (which Claude Code does on plugin reload).

### Compatibility

`DKOD_REVIEW_API_KEY` and `ANTHROPIC_API_KEY` stay as-is — they are engine-side and control the *platform's* server-run review for cloud-configured users. Setting the new `DKOD_ANTHROPIC_API_KEY` does not disturb them. Zero conflict because they're read in different processes.

---

## `dk_submit` background review flow

When `DKOD_CODE_REVIEW=1` and a provider key is configured, `dk_submit` kicks off a background review immediately after the engine acknowledges the submit. Lives in `dk-mcp/src/server.rs` inside the existing `dk_submit` tool handler.

### Flow

1. Agent calls `dk_submit`. MCP forwards to engine gRPC as today; engine returns `SubmitResponse { changeset_id, state, review_summary: local }`.
2. Before returning to the agent, MCP checks the feature flag. If off → return as today. If on → spawn a `tokio::spawn` background task capturing `(session_id, changeset_id, diff, context)`.
3. MCP returns the submit response **immediately** — the agent is never blocked. The submit response gets one appended hint line:
   `Deep code review started in background (provider: openrouter, model: anthropic/claude-sonnet-4). Poll dk_review or retry dk_approve to check status.`
4. Background task builds the `ReviewRequest` using existing `ReviewRequest` / `FileContext` / `ReviewProvider` types already in `dk-runner/src/steps/agent_review/`. No duplication — reuse the `ReviewProvider` trait.
5. Provider selection: factory function reads env, picks `OpenRouterReviewProvider` if `DKOD_OPENROUTER_API_KEY` is set, else `ClaudeReviewProvider`. Both implement `ReviewProvider`.
6. Provider call with `DKOD_REVIEW_TIMEOUT_SECS` timeout. Response parsed via existing `parse_review_response` helper.
7. Score derivation from `ReviewVerdict` + finding severities:

   | Verdict | Errors | Warnings | Score |
   |---------|--------|----------|-------|
   | Approve | 0 | 0 | 5 |
   | Approve | 0 | ≥1 | 4 |
   | Comment | — | — | 3 |
   | RequestChanges | 0 | ≥1 | 2 |
   | RequestChanges | ≥1 | — | 1 |

8. Task calls `RecordReview` gRPC: `{ session_id, changeset_id, tier: "deep", score, findings, summary, provider_name, model, duration_ms }`. Engine writes to existing review storage.
9. On LLM failure with `DKOD_REVIEW_BACKOFF_POLICY=strict`, task calls `RecordReview` with `score: null` and a `provider-error` finding. If `degraded`, task skips `RecordReview` (gate falls back to local-review score at submit time).

### Concurrency model

Each submit spawns its own task. For `/dkh` with 8 parallel generators, 8 background reviews run concurrently. No shared state, no mutex. Task handles are not retained — results are durably stored via `RecordReview`, so context loss across agent restarts doesn't matter.

### Caveat

The task runs in MCP-server scope, not agent scope. If the user quits Claude Code before a review finishes, the MCP process dies and the task is lost. For autonomous harness runs this is fine (harness holds the session). Phase 2 could persist task state to disk if this ever becomes a problem — not for v1.

---

## `dk_approve` gate flow

Lives in `dk-mcp/src/server.rs` inside the existing `dk_approve` tool handler, wrapping the current `ApproveRequest` forward.

### Flow when `DKOD_CODE_REVIEW=1`

1. Agent calls `dk_approve(session_id?, force?: bool, override_reason?: string)`. The `force` and `override_reason` fields are new to `ApproveParams`.
2. MCP resolves session and changeset as today.
3. **Force path** — if `force: true`:
   - Validate `override_reason` is non-empty and ≥ 20 chars (else reject).
   - Call engine `Approve` RPC with new `override_reason` + `review_snapshot` fields on `ApproveRequest`.
   - Engine stamps on changeset audit log.
   - Return success with `⚠ force-approved: <reason>` suffix. No review lookup.
4. **Gate path** — call engine `Review` RPC to fetch review tiers for the changeset.
5. **Pending check** — if no review with `tier="deep"` exists:

   ```json
   {
     "error": "deep_review_pending",
     "message": "Deep code review has not completed yet. Current tier(s): [local]. Retry dk_approve in ~15s, or poll dk_review.",
     "next_action": { "kind": "wait_and_retry", "retry_after_secs": 15, "can_fix": false }
   }
   ```

6. **Score check** — if deep tier exists but `score < DKOD_REVIEW_MIN_SCORE`:

   ```json
   {
     "error": "review_score_below_threshold",
     "message": "Deep review score 2/5 is below required 4/5. Fix the findings below and resubmit.",
     "score": 2,
     "threshold": 4,
     "findings": [ /* full list, same shape as dk_review output */ ],
     "next_action": {
       "kind": "fix_and_resubmit",
       "can_fix": true,
       "can_override": true,
       "override_hint": "If the findings are false positives, call dk_approve(force: true, override_reason: '...')."
     }
   }
   ```

7. **Provider-error check** (only with `BACKOFF_POLICY=strict`) — if deep tier exists with `score: null` and a `provider-error` finding → reject with `error: "review_provider_error"`, similar shape, `can_override: true`.
8. **Pass** — score ≥ threshold → forward to engine `Approve` RPC, return normal success plus quiet prefix `✓ deep review: 4/5 (openrouter).`

### Shape note

Rejections are returned as `CallToolResult::error` with `Content::text` containing the JSON payload pretty-printed — humans reading a `/dkh` log see a clean error, LLM agents can parse structured fields. Pattern already used in codebase (see `MergeConflict` responses).

---

## `/dkh` harness integration

Per decision #5 (A+B hybrid), `/dkh` owns its own review-fix loop and the MCP gate is the backstop.

### Changes to `harness/skills/dkh/agents/orchestrator.md`

1. **PRE-FLIGHT check** — detect MCP gate state. Read `DKOD_CODE_REVIEW`, `DKOD_ANTHROPIC_API_KEY`, `DKOD_OPENROUTER_API_KEY`. Log one of:
   - `code_review: disabled` — gate off, land pipeline uses today's score≥3 in `/dkod:land`.
   - `code_review: enabled (provider=openrouter|anthropic, min_score=4)` — orchestrator enforces the stricter flow below.
   - `code_review: misconfigured (flag set but no key)` — **abort** the harness run before any generators launch. Prevents wasting 20 minutes and failing at approve.

2. **LAND phase — new step between verify and approve** (when enabled):
   - Call `dk_review` and check for `tier: "deep"` with `score ≥ DKOD_REVIEW_MIN_SCORE`.
   - If deep pending → `dk_watch` for `changeset.review.completed` with 180s timeout. On timeout → fall through; `dk_approve` rejects cleanly, orchestrator re-enters wait loop.
   - If deep score < threshold → dispatch **fix-agent** (reuse existing generator dispatch template) with findings as prompt. Fix agent writes → submits → new deep review fires in background → orchestrator waits and re-checks.
   - Cap at 3 fix rounds (consistent with existing eval-round cap). On exceed → either force-approve with `override_reason: "Exceeded 3 review fix rounds; findings: ..."` or fail the unit.

3. **Force-approve usage is bounded** — only the orchestrator may call `dk_approve(force: true, …)`. Generators never force. Documented as an "ONLY FOR ORCHESTRATOR" section in the orchestrator agent.

4. **`/dkod:land` update** — plugin's land command gets a small addition: check `DKOD_CODE_REVIEW`. If `=1`, use `DKOD_REVIEW_MIN_SCORE` instead of hardcoded `3`. Otherwise no change. Keeps manual (non-harness) users consistent with harness when they opt in.

### What `/dkh` does NOT do

- Does not duplicate gate logic. MCP `dk_approve` gate is the single source of truth for the threshold.
- Does not read `DKOD_*` vars to make policy decisions — only to surface status to the user in the PRE-FLIGHT log.
- No separate `DKOD_HARNESS_REVIEW_MIN_SCORE`. One threshold, one knob.

### Expected behavior change

**Before:** 8 generators submit → orchestrator calls approve → some merge at 2/5 because approve never blocked.

**After:** 8 generators submit → 8 deep reviews fire in background → orchestrator waits-then-approves each → any at 2/5 get dispatched a fix-agent → re-submit → re-review → re-approve → merge. At most 3 fix rounds per changeset before force-approve with documented reason.

---

## Override path & audit trail

Per decision #9, the only way past the gate is `dk_approve(force: true, override_reason: …)`.

### Client-side validation (in `dk_approve` handler)

1. `force: true` and `override_reason` missing/empty/whitespace → reject with `"force requires override_reason (non-empty)"`.
2. `force: true` and `override_reason.len() < 20` → reject with `"override_reason must be at least 20 characters (describe why review is being bypassed)"`. Forces the caller to write something meaningful.
3. `force: true` but `DKOD_CODE_REVIEW` not set → no-op; MCP accepts, logs `"force requested but gate is disabled — proceeding as normal approve"`. No error.

### Engine-side recording

`ApproveRequest` protobuf gets two new optional fields:

```proto
optional string override_reason = 2;
optional ReviewSnapshot review_snapshot = 3;
```

`review_snapshot` captures what the gate saw at force time (current score, threshold, findings count, provider). MCP populates from `dk_review` before forcing. Audit record tells the full story:

> `force-approved at score 2/5 below threshold 4/5, override_reason: "API was wedged for 20 minutes, reviewed manually in chat"`

Engine writes both into changeset audit log. Surfaces in dashboard and `dk_status`:

```
changeset: abc123 [merged]
  review: deep 2/5 (openrouter) — OVERRIDDEN
  override_reason: "API was wedged for 20 minutes, reviewed manually in chat"
```

### Visibility & review discipline

1. Overridden merges are first-class in dashboard (filter: "show only overridden"). Weekly audit helps tune `DKOD_REVIEW_MIN_SCORE`.
2. `dk_push` includes a note at the bottom of the PR body when any merged changeset had `override_reason`:
   ```
   ## Review overrides
   - abc123: "API was wedged for 20 minutes, reviewed manually in chat" (score 2/5, threshold 4/5)
   ```
   GitHub reviewers see it without extra tooling. Cheap to implement; makes silent-skip impossible.
3. No per-changeset override count limit; `/dkh`'s 3-fix-round cap already bounds autonomous force usage. Manual users can force however they want — that's their judgment call.

### Explicitly not part of this design

- No `DKOD_REVIEW_BYPASS` env-level bypass (rejected in Q9 option C).
- No "expire after N hours" on override — audits are permanent.
- No approval-by-another-agent workflow.

---

## Testing & rollout

### Unit tests (`dk-mcp/src/server.rs`, new `dk-mcp/src/review_gate.rs`)

1. Env parsing: table-driven tests for 9 vars (flag off, flag on + each provider, both keys set → OpenRouter, flag on + no key, invalid threshold, etc.).
2. Verdict → score mapping (table-driven, per the table above).
3. Gate decision: given fake `ReviewResponse` + threshold, assert correct rejection shape or pass-through.
4. Force validation: empty reason, 5-char, 20-char, flag-off + force combo.
5. Provider selection: mock env, assert factory returns correct `ReviewProvider` impl.

### Integration tests (`dk-mcp/tests/`)

6. New `review_gate_test.rs`:
   - `DKOD_CODE_REVIEW` unset → `dk_approve` works as today.
   - `=1` + mock provider returning score 5 → submit → wait → approve succeeds.
   - `=1` + mock provider returning score 2 → approve rejects with structured `review_score_below_threshold`.
   - `=1` + approve called before background task completes → rejects with `deep_review_pending`.
   - Force path: score 2 + `force: true` + 30-char reason → approve succeeds, audit contains reason + snapshot.
7. `MockReviewProvider` added alongside Claude/OpenRouter, selected via `DKOD_REVIEW_MODEL=mock-passthrough`. Keeps CI from burning LLM tokens.

### Engine-side tests (`dk-engine/src/changeset.rs` + protocol tests)

8. `RecordReview` RPC: write + read-back via `Review` RPC.
9. `ApproveRequest` with `override_reason` + `review_snapshot`: persisted to audit log, surfaced in `dk_status`.

### Harness tests (`dkod-harness`)

10. Smoke test in e2e suite: small repo, `DKOD_CODE_REVIEW=1` with mock provider returning score 3, 2-generator harness, assert orchestrator dispatches fix-agent and eventually succeeds or hits force-approve with "Exceeded 3 review fix rounds".

### Manual verification before merging

11. `claude mcp add dkod` on self-hosted engine, export `DKOD_CODE_REVIEW=1` + `DKOD_OPENROUTER_API_KEY=…`, submit real changeset, observe end-to-end.
12. Same with `DKOD_ANTHROPIC_API_KEY=…` for the Anthropic path.
13. Confirm that with feature off (no env vars), performance and behavior are bit-for-bit identical to today.

### Rollout plan

Three PRs, each independently shippable:

- **PR 1 — engine:** new `RecordReview` RPC + protobuf changes + two new `ApproveRequest` fields + `OpenRouterReviewProvider` implementation (mirrors `ClaudeReviewProvider`). No user-visible behavior change yet.
- **PR 2 — MCP gate:** env-var parsing, background review spawn in `dk_submit`, gate logic in `dk_approve`, force handling, structured rejections. Feature-flagged by `DKOD_CODE_REVIEW=1`. This is the behavior-shipping PR.
- **PR 3 — harness + `/dkod:land`:** PRE-FLIGHT check, fix-loop integration, land command threshold switch, PR body override note. Polish.

Safe merge checkpoints: PR 1 alone ships dormant server capability. PR 2 alone delivers gate for CLI/manual users. PR 3 completes the harness integration.

---

## Open questions / future work

1. **Task persistence across MCP restarts.** v1 loses background review tasks if MCP dies mid-call. Phase 2: persist `(changeset_id, provider, started_at)` to sqlite; replay on restart.
2. **Multi-provider fan-out.** Some teams may want both Anthropic and OpenRouter reviews for comparison. Out of scope for v1 — one provider per MCP instance.
3. **Cost tracking.** `RecordReview` stores `duration_ms` but not token counts. Could extend to `{input_tokens, output_tokens}` for dashboard cost attribution.
4. **Rate limiting.** High-parallelism harness runs (20+ generators) could hit Anthropic/OpenRouter rate limits. v1 has no queue/throttle — relies on provider retries. Phase 2: add a semaphore per-provider.
5. **Model drift.** If the default `claude-sonnet-4-6` is sunset by Anthropic, reviews silently fail. Long-term: periodic model-availability check in MCP startup, warn if default is deprecated.

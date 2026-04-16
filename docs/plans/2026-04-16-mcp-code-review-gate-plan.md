# MCP Code Review Gate Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a client-side code-review gate at the MCP layer that blocks `dk_approve` when deep-review score is below a configurable threshold, opt-in via `DKOD_CODE_REVIEW=1` with BYOK provider keys.

**Architecture:** MCP server (`dk-mcp` crate) becomes the review driver. On `dk_submit` it spawns a background tokio task that calls Anthropic/OpenRouter with user's local key, then stores the result via a new engine `RecordReview` RPC. On `dk_approve` it fetches via `dk_review` and rejects with structured findings if score < `DKOD_REVIEW_MIN_SCORE` (default 4). A `force` flag with `override_reason` (≥20 chars) bypasses the gate with a permanent audit trail.

**Tech Stack:** Rust (tonic gRPC + rmcp + tokio + reqwest), prost-build for protobuf, async-trait, existing `ReviewProvider` trait from `dk-runner/src/steps/agent_review/`.

**Reference:** Design in `docs/plans/2026-04-16-mcp-code-review-gate-design.md`.

**Repo convention:** All work happens in three repos — `dkod-engine`, `dkod-plugin`, `dkod-harness`. Normal git workflow per `feedback_use_dkod_not_git.md`. Each phase produces one PR; never push to main.

---

## Phase 1 — Engine protocol + OpenRouter provider (PR 1)

Repo: `dkod-engine`
Branch: `feat/record-review-rpc`
All tasks assume `cd /Users/haimari/vsCode/haim-ari/github/dkod-engine`.

### Task 1.1: Add `override_reason` + `ReviewSnapshot` to `ApproveRequest` proto

**Files:**
- Modify: `proto/dkod/v1/agent.proto:479-481` (workspace copy)
- Modify: `crates/dk-protocol/proto/dkod/v1/agent.proto:479-481` (crate copy — CI enforces sync)

**Step 1: Write the failing test**

Create `crates/dk-protocol/tests/approve_proto_test.rs`:

```rust
use dk_protocol::{ApproveRequest, ReviewSnapshot};

#[test]
fn approve_request_has_override_reason_and_snapshot() {
    let req = ApproveRequest {
        session_id: "s1".into(),
        override_reason: Some("Exceeded 3 review fix rounds; findings: X,Y".into()),
        review_snapshot: Some(ReviewSnapshot {
            score: 2,
            threshold: 4,
            findings_count: 3,
            provider: "openrouter".into(),
            model: "anthropic/claude-sonnet-4".into(),
        }),
    };
    assert_eq!(req.override_reason.as_deref(), Some("Exceeded 3 review fix rounds; findings: X,Y"));
    assert_eq!(req.review_snapshot.as_ref().unwrap().score, 2);
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-protocol --test approve_proto_test
```

Expected: FAIL — `ApproveRequest` has no `override_reason` field, `ReviewSnapshot` does not exist.

**Step 3: Edit the proto (both copies — workspace root + dk-protocol crate)**

Modify `proto/dkod/v1/agent.proto` around line 479 AND the same block in `crates/dk-protocol/proto/dkod/v1/agent.proto`:

```proto
// --- APPROVE ---

message ReviewSnapshot {
  int32 score = 1;
  int32 threshold = 2;
  int32 findings_count = 3;
  string provider = 4;       // "anthropic" | "openrouter"
  string model = 5;
}

message ApproveRequest {
  string session_id = 1;
  optional string override_reason = 2;
  optional ReviewSnapshot review_snapshot = 3;
}
```

**Step 4: Run to verify pass**

```bash
cargo build -p dk-protocol && cargo test -p dk-protocol --test approve_proto_test
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "proto: add override_reason + ReviewSnapshot to ApproveRequest"
```

---

### Task 1.2: Add `RecordReview` RPC proto

**Files:**
- Modify: `proto/dkod/v1/agent.proto` (insert after `Review` RPC at line 44, plus new request/response messages after `ReviewResponse` at line 360)

**Step 1: Write the failing test**

Add to `crates/dk-protocol/tests/approve_proto_test.rs`:

```rust
use dk_protocol::{RecordReviewRequest, RecordReviewResponse, ReviewFindingProto};

#[test]
fn record_review_request_shape() {
    let req = RecordReviewRequest {
        session_id: "s1".into(),
        changeset_id: "c1".into(),
        tier: "deep".into(),
        score: Some(4),
        summary: Some("LGTM with minor warnings".into()),
        findings: vec![],
        provider: "anthropic".into(),
        model: "claude-sonnet-4-6".into(),
        duration_ms: 12345,
    };
    assert_eq!(req.tier, "deep");
    assert_eq!(req.score, Some(4));
    assert_eq!(req.duration_ms, 12345);
}

#[test]
fn record_review_response_shape() {
    let resp = RecordReviewResponse {
        review_id: "r1".into(),
        accepted: true,
    };
    assert!(resp.accepted);
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-protocol --test approve_proto_test
```

Expected: FAIL — `RecordReviewRequest`/`RecordReviewResponse` do not exist.

**Step 3: Edit BOTH proto copies (workspace + dk-protocol crate)**

Insert at line 45 (after `rpc Review(...)`) in BOTH `proto/dkod/v1/agent.proto` AND `crates/dk-protocol/proto/dkod/v1/agent.proto`:

```proto
  rpc RecordReview(RecordReviewRequest) returns (RecordReviewResponse);
```

Insert after `ReviewResponse` (line 360) in BOTH files:

```proto
message RecordReviewRequest {
  string session_id = 1;
  string changeset_id = 2;
  string tier = 3;                                    // "deep"
  optional int32 score = 4;                           // null when provider errored under strict policy
  optional string summary = 5;
  repeated ReviewFindingProto findings = 6;
  string provider = 7;
  string model = 8;
  int64 duration_ms = 9;
}

message RecordReviewResponse {
  string review_id = 1;
  bool accepted = 2;
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-protocol --test approve_proto_test
```

Expected: PASS (both tests in this file).

**Step 5: Commit**

```bash
git commit -am "proto: add RecordReview RPC"
```

---

### Task 1.3: Implement engine-side `RecordReview` handler

**Files:**
- Create: `crates/dk-engine/src/review_store.rs` (new module) — or extend existing review storage
- Modify: `crates/dk-mcp/src/grpc.rs` (server handler trait impl)

**Step 1: Locate existing review storage**

```bash
grep -rn "fn review" crates/dk-engine/src/ | head -5
```

Find the existing `fn review(...)` handler and the table/struct it writes to. Add a sibling `fn record_review(...)`.

**Step 2: Write the failing integration test**

Create `crates/dk-engine/tests/record_review_test.rs`:

```rust
use dk_engine::{Engine, ReviewFinding};
use dkod_protocol::dkod::v1::{RecordReviewRequest, ReviewRequest};

#[tokio::test]
async fn record_review_then_fetch_it_back() {
    let engine = Engine::new_in_memory().await;
    let changeset_id = engine.create_test_changeset().await;

    let resp = engine.record_review(RecordReviewRequest {
        session_id: "s1".into(),
        changeset_id: changeset_id.clone(),
        tier: "deep".into(),
        score: Some(4),
        summary: Some("OK".into()),
        findings: vec![],
        provider: "anthropic".into(),
        model: "claude-sonnet-4-6".into(),
        duration_ms: 999,
    }).await.unwrap();
    assert!(resp.accepted);

    let reviews = engine.review(ReviewRequest {
        session_id: "s1".into(),
        changeset_id,
    }).await.unwrap().reviews;

    let deep = reviews.iter().find(|r| r.tier == "deep").unwrap();
    assert_eq!(deep.score, Some(4));
}
```

**Step 3: Run to verify fail**

```bash
cargo test -p dk-engine --test record_review_test
```

Expected: FAIL — `Engine::record_review` doesn't exist.

**Step 4: Implement the handler**

Add `record_review` method to the engine's review store alongside the existing `review` reader. It writes into the same table with `tier = "deep"` and returns an ID.

**Step 5: Wire the RPC handler in the gRPC server**

Implement `async fn record_review(...)` on the `AgentService` impl, delegating to the engine method.

**Step 6: Run to verify pass**

```bash
cargo test -p dk-engine --test record_review_test && cargo build -p dk-mcp
```

Expected: PASS + clean build.

**Step 7: Commit**

```bash
git commit -am "engine: implement RecordReview handler, write to review store"
```

---

### Task 1.4: Persist `override_reason` + `ReviewSnapshot` in `Approve` handler

**Files:**
- Modify: existing `approve` engine handler (find via `grep -rn "fn approve" crates/dk-engine/src/`)
- Modify: audit log write path

**Step 1: Write the failing test**

Create `crates/dk-engine/tests/approve_override_test.rs`:

```rust
use dkod_protocol::dkod::v1::{ApproveRequest, ReviewSnapshot};

#[tokio::test]
async fn approve_persists_override_reason_to_audit_log() {
    let engine = dk_engine::Engine::new_in_memory().await;
    let changeset_id = engine.create_submitted_changeset().await;
    let req = ApproveRequest {
        session_id: "s1".into(),
        override_reason: Some("API wedged for 20 minutes; reviewed manually in chat".into()),
        review_snapshot: Some(ReviewSnapshot {
            score: 2,
            threshold: 4,
            findings_count: 3,
            provider: "openrouter".into(),
            model: "anthropic/claude-sonnet-4".into(),
        }),
    };
    let _ = engine.approve(req).await.unwrap();

    let audit = engine.get_changeset_audit(&changeset_id).await.unwrap();
    assert!(audit.override_reason.as_deref().unwrap().contains("API wedged"));
    assert_eq!(audit.review_snapshot.as_ref().unwrap().score, 2);
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-engine --test approve_override_test
```

Expected: FAIL — audit log has no `override_reason`/`review_snapshot` fields.

**Step 3: Extend the changeset audit schema**

Add two optional columns / struct fields to the audit record. Update the `approve` handler to write them when present.

**Step 4: Run to verify pass**

```bash
cargo test -p dk-engine --test approve_override_test
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "engine: persist override_reason + review_snapshot on approve"
```

---

### Task 1.5: Write `OpenRouterReviewProvider`

**Files:**
- Create: `crates/dk-runner/src/steps/agent_review/openrouter.rs`
- Modify: `crates/dk-runner/src/steps/agent_review/mod.rs` (add `pub mod openrouter;`)

**Step 1: Write the failing test**

Add to `crates/dk-runner/src/steps/agent_review/openrouter.rs` (or separate `openrouter_test.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::OpenRouterReviewProvider;
    use crate::steps::agent_review::provider::{ReviewProvider, ReviewRequest};

    #[tokio::test]
    async fn from_env_returns_none_without_key() {
        std::env::remove_var("DKOD_OPENROUTER_API_KEY");
        assert!(OpenRouterReviewProvider::from_env().is_none());
    }

    #[tokio::test]
    async fn from_env_builds_provider_with_key() {
        std::env::set_var("DKOD_OPENROUTER_API_KEY", "sk-test");
        let p = OpenRouterReviewProvider::from_env().unwrap();
        assert_eq!(p.name(), "openrouter");
        std::env::remove_var("DKOD_OPENROUTER_API_KEY");
    }
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-runner openrouter
```

Expected: FAIL — module doesn't exist.

**Step 3: Implement**

Write `OpenRouterReviewProvider` mirroring `ClaudeReviewProvider` in `claude.rs`. Differences: base URL `https://openrouter.ai/api/v1/chat/completions` (overridable via `DKOD_OPENROUTER_BASE_URL`), default model `anthropic/claude-sonnet-4`, OpenAI-compatible chat/completions shape:

```rust
use std::time::Duration;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use super::parse::parse_review_response;
use super::prompt::build_review_prompt;
use super::provider::{ReviewProvider, ReviewRequest, ReviewResponse};

pub struct OpenRouterReviewProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: usize,
    base_url: String,
}

impl OpenRouterReviewProvider {
    pub fn new(api_key: String, model: Option<String>, max_tokens: Option<usize>, base_url: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(120)).build()?;
        Ok(Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| "anthropic/claude-sonnet-4".to_string()),
            max_tokens: max_tokens.unwrap_or(4096),
            base_url: base_url.unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
        })
    }

    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DKOD_OPENROUTER_API_KEY").ok()?;
        let model = std::env::var("DKOD_REVIEW_MODEL").ok();
        let base_url = std::env::var("DKOD_OPENROUTER_BASE_URL").ok();
        Self::new(api_key, model, None, base_url).ok()
    }
}

#[derive(Serialize)]
struct ChatRequest { model: String, max_tokens: usize, messages: Vec<ChatMessage> }
#[derive(Serialize)]
struct ChatMessage { role: String, content: String }
#[derive(Deserialize)]
struct ChatResponse { choices: Vec<Choice> }
#[derive(Deserialize)]
struct Choice { message: ResponseMessage }
#[derive(Deserialize)]
struct ResponseMessage { content: String }

#[async_trait::async_trait]
impl ReviewProvider for OpenRouterReviewProvider {
    fn name(&self) -> &str { "openrouter" }

    async fn review(&self, request: ReviewRequest) -> Result<ReviewResponse> {
        let prompt = build_review_prompt(&request);
        let resp = self.client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://dkod.io")
            .header("X-Title", "dkod code review")
            .json(&ChatRequest {
                model: self.model.clone(),
                max_tokens: self.max_tokens,
                messages: vec![ChatMessage { role: "user".into(), content: prompt }],
            })
            .send().await.context("Failed to call OpenRouter")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenRouter returned {status}: {body}");
        }
        let api_resp: ChatResponse = resp.json().await.context("Failed to parse OpenRouter response")?;
        let text = api_resp.choices.into_iter().next()
            .map(|c| c.message.content)
            .unwrap_or_default();
        parse_review_response(&text)
    }
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-runner openrouter
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "runner: add OpenRouterReviewProvider"
```

---

### Task 1.6: Provider factory & precedence

**Files:**
- Modify: `crates/dk-runner/src/steps/agent_review/mod.rs`

**Step 1: Write the failing test**

Add to `mod.rs`:

```rust
#[cfg(test)]
mod provider_factory_tests {
    use super::*;

    #[tokio::test]
    async fn openrouter_wins_when_both_keys_set() {
        std::env::set_var("DKOD_ANTHROPIC_API_KEY", "sk-ant");
        std::env::set_var("DKOD_OPENROUTER_API_KEY", "sk-or");
        let p = select_provider_from_env().unwrap();
        assert_eq!(p.name(), "openrouter");
        std::env::remove_var("DKOD_ANTHROPIC_API_KEY");
        std::env::remove_var("DKOD_OPENROUTER_API_KEY");
    }

    #[tokio::test]
    async fn anthropic_selected_when_only_anthropic_set() {
        std::env::remove_var("DKOD_OPENROUTER_API_KEY");
        std::env::set_var("DKOD_ANTHROPIC_API_KEY", "sk-ant");
        let p = select_provider_from_env().unwrap();
        assert_eq!(p.name(), "claude");
        std::env::remove_var("DKOD_ANTHROPIC_API_KEY");
    }

    #[tokio::test]
    async fn none_when_no_keys() {
        std::env::remove_var("DKOD_ANTHROPIC_API_KEY");
        std::env::remove_var("DKOD_OPENROUTER_API_KEY");
        assert!(select_provider_from_env().is_none());
    }
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-runner provider_factory_tests
```

Expected: FAIL — `select_provider_from_env` doesn't exist.

**Step 3: Implement factory**

Add to `mod.rs`:

```rust
pub fn select_provider_from_env() -> Option<Box<dyn provider::ReviewProvider>> {
    use openrouter::OpenRouterReviewProvider;
    use claude::ClaudeReviewProvider;

    // OpenRouter wins when both are set.
    if std::env::var("DKOD_OPENROUTER_API_KEY").is_ok() {
        return OpenRouterReviewProvider::from_env().map(|p| Box::new(p) as _);
    }
    if std::env::var("DKOD_ANTHROPIC_API_KEY").is_ok() {
        // Alias the new var into the existing ClaudeReviewProvider::from_env flow
        let key = std::env::var("DKOD_ANTHROPIC_API_KEY").ok()?;
        let model = std::env::var("DKOD_REVIEW_MODEL").ok();
        return ClaudeReviewProvider::new(key, model, None).ok().map(|p| Box::new(p) as _);
    }
    None
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-runner provider_factory_tests
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "runner: provider factory with OpenRouter-wins precedence"
```

---

### Task 1.7: Open PR 1

```bash
git push -u origin feat/record-review-rpc
gh pr create --title "feat: RecordReview RPC + override audit fields + OpenRouter provider" --body "$(cat <<'EOF'
## Summary
- New RPC `RecordReview(session_id, changeset_id, tier, score, findings, provider, model, duration_ms)` for clients that drive their own review (BYOK-at-client)
- `ApproveRequest` grows optional `override_reason` + `ReviewSnapshot` for force-approve audit trail
- New `OpenRouterReviewProvider` mirroring `ClaudeReviewProvider`, with OpenRouter-wins precedence in `select_provider_from_env`

Design: `docs/plans/2026-04-16-mcp-code-review-gate-design.md`

## Test plan
- [ ] `cargo test -p dkod-protocol` passes
- [ ] `cargo test -p dk-engine` passes (includes RecordReview + approve audit tests)
- [ ] `cargo test -p dk-runner` passes (includes provider factory + openrouter tests)
- [ ] No user-visible behavior change for existing callers

## Scope note
This PR is dormant capability — no gate enforcement yet. PR 2 (dk-mcp gate) turns this on behind `DKOD_CODE_REVIEW=1`.
EOF
)"
```

---

## Phase 2 — MCP gate (PR 2)

Repo: `dkod-engine`
Branch: `feat/mcp-code-review-gate`
Target the same repo since `dk-mcp` lives inside.

### Task 2.1: `review_gate` module — env parsing struct

**Files:**
- Create: `crates/dk-mcp/src/review_gate.rs`
- Modify: `crates/dk-mcp/src/lib.rs` (add `pub mod review_gate;`)

**Step 1: Write the failing test**

Append to `review_gate.rs`:

```rust
#[cfg(test)]
mod env_parsing_tests {
    use super::GateConfig;

    fn clear_all() {
        for k in ["DKOD_CODE_REVIEW", "DKOD_ANTHROPIC_API_KEY", "DKOD_OPENROUTER_API_KEY",
                  "DKOD_REVIEW_MIN_SCORE", "DKOD_REVIEW_TIMEOUT_SECS",
                  "DKOD_REVIEW_BACKOFF_POLICY", "DKOD_REVIEW_MODEL"] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn disabled_when_flag_unset() {
        clear_all();
        assert!(!GateConfig::from_env().enabled);
    }

    #[test]
    fn enabled_with_anthropic_key() {
        clear_all();
        std::env::set_var("DKOD_CODE_REVIEW", "1");
        std::env::set_var("DKOD_ANTHROPIC_API_KEY", "sk-ant");
        let cfg = GateConfig::from_env();
        assert!(cfg.enabled);
        assert_eq!(cfg.provider_name.as_deref(), Some("anthropic"));
        assert_eq!(cfg.min_score, 4);
        clear_all();
    }

    #[test]
    fn misconfigured_when_flag_set_but_no_key() {
        clear_all();
        std::env::set_var("DKOD_CODE_REVIEW", "1");
        let cfg = GateConfig::from_env();
        assert!(cfg.enabled);
        assert!(cfg.provider_name.is_none());
        assert!(cfg.misconfigured());
        clear_all();
    }

    #[test]
    fn openrouter_wins_when_both_set() {
        clear_all();
        std::env::set_var("DKOD_CODE_REVIEW", "1");
        std::env::set_var("DKOD_ANTHROPIC_API_KEY", "sk-ant");
        std::env::set_var("DKOD_OPENROUTER_API_KEY", "sk-or");
        let cfg = GateConfig::from_env();
        assert_eq!(cfg.provider_name.as_deref(), Some("openrouter"));
        clear_all();
    }

    #[test]
    fn min_score_overridable() {
        clear_all();
        std::env::set_var("DKOD_CODE_REVIEW", "1");
        std::env::set_var("DKOD_ANTHROPIC_API_KEY", "sk-ant");
        std::env::set_var("DKOD_REVIEW_MIN_SCORE", "5");
        let cfg = GateConfig::from_env();
        assert_eq!(cfg.min_score, 5);
        clear_all();
    }

    #[test]
    fn min_score_invalid_falls_back_to_default() {
        clear_all();
        std::env::set_var("DKOD_CODE_REVIEW", "1");
        std::env::set_var("DKOD_ANTHROPIC_API_KEY", "sk-ant");
        std::env::set_var("DKOD_REVIEW_MIN_SCORE", "banana");
        let cfg = GateConfig::from_env();
        assert_eq!(cfg.min_score, 4);
        clear_all();
    }
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-mcp env_parsing_tests
```

Expected: FAIL — `GateConfig` doesn't exist.

**Step 3: Implement**

At top of `review_gate.rs`:

```rust
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub enabled: bool,
    pub provider_name: Option<String>,    // None if flag set but no key
    pub min_score: i32,
    pub timeout: Duration,
    pub backoff_policy: BackoffPolicy,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffPolicy { Strict, Degraded }

impl GateConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("DKOD_CODE_REVIEW").map(|v| v == "1").unwrap_or(false);
        let provider_name = if std::env::var("DKOD_OPENROUTER_API_KEY").is_ok() {
            Some("openrouter".to_string())
        } else if std::env::var("DKOD_ANTHROPIC_API_KEY").is_ok() {
            Some("anthropic".to_string())
        } else {
            None
        };
        let min_score = std::env::var("DKOD_REVIEW_MIN_SCORE")
            .ok().and_then(|s| s.parse().ok())
            .filter(|&n: &i32| (1..=5).contains(&n))
            .unwrap_or(4);
        let timeout = std::env::var("DKOD_REVIEW_TIMEOUT_SECS")
            .ok().and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(180));
        let backoff_policy = match std::env::var("DKOD_REVIEW_BACKOFF_POLICY").as_deref() {
            Ok("degraded") => BackoffPolicy::Degraded,
            _ => BackoffPolicy::Strict,
        };
        let model = std::env::var("DKOD_REVIEW_MODEL").ok();
        Self { enabled, provider_name, min_score, timeout, backoff_policy, model }
    }

    pub fn misconfigured(&self) -> bool {
        self.enabled && self.provider_name.is_none()
    }
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-mcp env_parsing_tests
```

Expected: PASS (6 tests).

**Step 5: Commit**

```bash
git commit -am "mcp: add GateConfig env parsing"
```

---

### Task 2.2: Verdict → score mapping

**Files:**
- Modify: `crates/dk-mcp/src/review_gate.rs`

**Step 1: Write the failing test**

Append to `review_gate.rs`:

```rust
#[cfg(test)]
mod verdict_mapping_tests {
    use super::score_from_verdict;
    use dk_runner::steps::agent_review::provider::ReviewVerdict;
    use dk_runner::findings::{Finding, Severity};

    fn f(sev: Severity) -> Finding {
        Finding { severity: sev, check_name: "x".into(), message: "m".into(),
                  file_path: None, line: None, symbol: None }
    }

    #[test]
    fn approve_no_issues_is_5() {
        assert_eq!(score_from_verdict(&ReviewVerdict::Approve, &[]), 5);
    }
    #[test]
    fn approve_with_warnings_is_4() {
        assert_eq!(score_from_verdict(&ReviewVerdict::Approve, &[f(Severity::Warning)]), 4);
    }
    #[test]
    fn comment_is_3() {
        assert_eq!(score_from_verdict(&ReviewVerdict::Comment, &[]), 3);
    }
    #[test]
    fn request_changes_with_only_warnings_is_2() {
        assert_eq!(score_from_verdict(&ReviewVerdict::RequestChanges, &[f(Severity::Warning)]), 2);
    }
    #[test]
    fn request_changes_with_errors_is_1() {
        assert_eq!(score_from_verdict(&ReviewVerdict::RequestChanges, &[f(Severity::Error)]), 1);
    }
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-mcp verdict_mapping_tests
```

Expected: FAIL — `score_from_verdict` doesn't exist.

**Step 3: Implement**

Append to `review_gate.rs`:

```rust
use dk_runner::steps::agent_review::provider::ReviewVerdict;
use dk_runner::findings::{Finding, Severity};

pub fn score_from_verdict(verdict: &ReviewVerdict, findings: &[Finding]) -> i32 {
    let has_error = findings.iter().any(|f| f.severity == Severity::Error);
    let has_warning = findings.iter().any(|f| f.severity == Severity::Warning);
    match (verdict, has_error, has_warning) {
        (ReviewVerdict::Approve, false, false) => 5,
        (ReviewVerdict::Approve, _, _) => 4,
        (ReviewVerdict::Comment, _, _) => 3,
        (ReviewVerdict::RequestChanges, false, _) => 2,
        (ReviewVerdict::RequestChanges, true, _) => 1,
    }
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-mcp verdict_mapping_tests
```

Expected: PASS (5 tests).

**Step 5: Commit**

```bash
git commit -am "mcp: score_from_verdict mapping"
```

---

### Task 2.3: MockReviewProvider for tests

**Files:**
- Create: `crates/dk-mcp/src/review_gate_mock.rs` (gated `#[cfg(any(test, feature = "mock-review"))]`)

**Step 1: Write the failing test**

Add to `review_gate_mock.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::MockReviewProvider;
    use dk_runner::steps::agent_review::provider::{ReviewProvider, ReviewRequest, ReviewVerdict};

    #[tokio::test]
    async fn mock_returns_configured_score() {
        let m = MockReviewProvider::new(ReviewVerdict::Approve, vec![]);
        let resp = m.review(ReviewRequest {
            diff: "".into(), context: vec![], language: "rust".into(), intent: "t".into()
        }).await.unwrap();
        assert!(matches!(resp.verdict, ReviewVerdict::Approve));
    }
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-mcp mock_returns
```

Expected: FAIL.

**Step 3: Implement**

```rust
use async_trait::async_trait;
use anyhow::Result;
use dk_runner::steps::agent_review::provider::{ReviewProvider, ReviewRequest, ReviewResponse, ReviewVerdict};
use dk_runner::findings::{Finding, Suggestion};

pub struct MockReviewProvider {
    verdict: ReviewVerdict,
    findings: Vec<Finding>,
}

impl MockReviewProvider {
    pub fn new(verdict: ReviewVerdict, findings: Vec<Finding>) -> Self {
        Self { verdict, findings }
    }
}

#[async_trait]
impl ReviewProvider for MockReviewProvider {
    fn name(&self) -> &str { "mock" }
    async fn review(&self, _req: ReviewRequest) -> Result<ReviewResponse> {
        Ok(ReviewResponse {
            summary: "mock".into(),
            findings: self.findings.clone(),
            suggestions: Vec::<Suggestion>::new(),
            verdict: self.verdict.clone(),
        })
    }
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-mcp mock_returns
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "mcp: MockReviewProvider for tests"
```

---

### Task 2.4: Background review spawn in `dk_submit`

**Files:**
- Modify: `crates/dk-mcp/src/server.rs` (`dk_submit` tool handler)

**Step 1: Write the failing integration test**

Create `crates/dk-mcp/tests/submit_spawn_test.rs`:

```rust
// Uses the e2e_test scaffolding. Spins up MCP with DKOD_CODE_REVIEW=1 + mock provider.
// After dk_submit, polls for a "deep" review via RecordReview→dk_review.

#[tokio::test]
async fn submit_spawns_background_deep_review() {
    let harness = testhelpers::McpTestHarness::new()
        .with_env("DKOD_CODE_REVIEW", "1")
        .with_env("DKOD_ANTHROPIC_API_KEY", "sk-mock")
        .with_mock_provider_verdict(ReviewVerdict::Approve)
        .start().await;

    harness.connect("demo/hello").await;
    let submit = harness.call_submit("add x").await;
    assert!(submit.message.contains("Deep code review started"));

    // Background task should complete within ~2s with the mock provider.
    let review = harness.wait_for_deep_review(std::time::Duration::from_secs(3)).await;
    assert_eq!(review.score, Some(5));
    assert_eq!(review.tier, "deep");
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-mcp --test submit_spawn_test
```

Expected: FAIL — submit handler doesn't spawn background review.

**Step 3: Implement**

In `server.rs`, after `client.submit(request).await` returns successfully and before the `CallToolResult` is built, when `GateConfig::from_env().enabled && !misconfigured`:

```rust
let cfg = crate::review_gate::GateConfig::from_env();
if cfg.enabled && !cfg.misconfigured() {
    let session_id_clone = session.session_id.clone();
    let changeset_id = response.changeset_id.clone();
    let grpc_addr = self.state.server_addr.clone();
    let auth_token = self.state.auth_token.clone();
    let diff = submit_diff.clone();
    let context = submit_files.clone();
    let cfg_clone = cfg.clone();
    tokio::spawn(async move {
        crate::review_gate::run_background_review(
            grpc_addr, auth_token, session_id_clone, changeset_id, diff, context, cfg_clone
        ).await;
    });
    // Append hint line to the success message.
    text.push_str(&format!(
        "\nDeep code review started in background (provider: {}, min_score: {}). Retry dk_approve to check status.\n",
        cfg.provider_name.as_deref().unwrap_or("?"),
        cfg.min_score
    ));
}
```

Implement `run_background_review` in `review_gate.rs`:

```rust
pub async fn run_background_review(
    grpc_addr: String,
    auth_token: Option<String>,
    session_id: String,
    changeset_id: String,
    diff: String,
    context: Vec<FileContext>,
    cfg: GateConfig,
) {
    let provider = match select_provider() {
        Some(p) => p,
        None => return,
    };
    let start = std::time::Instant::now();
    let review_future = provider.review(ReviewRequest {
        diff, context, language: "rust".into(), intent: "deep review".into()
    });
    let result = tokio::time::timeout(cfg.timeout, review_future).await;

    let mut client = match connect_grpc(grpc_addr, auth_token).await {
        Ok(c) => c,
        Err(_) => return,
    };

    let record = match result {
        Ok(Ok(resp)) => {
            let score = score_from_verdict(&resp.verdict, &resp.findings);
            RecordReviewRequest {
                session_id, changeset_id, tier: "deep".into(),
                score: Some(score),
                summary: Some(resp.summary),
                findings: resp.findings.into_iter().map(into_proto).collect(),
                provider: provider.name().into(),
                model: cfg.model.unwrap_or_default(),
                duration_ms: start.elapsed().as_millis() as i64,
            }
        }
        Ok(Err(e)) | Err(_) if matches!(cfg.backoff_policy, BackoffPolicy::Strict) => {
            RecordReviewRequest {
                session_id, changeset_id, tier: "deep".into(),
                score: None,
                summary: Some("provider error".into()),
                findings: vec![provider_error_finding(format!("{e:?}"))],
                provider: provider.name().into(),
                model: cfg.model.unwrap_or_default(),
                duration_ms: start.elapsed().as_millis() as i64,
            }
        }
        _ => return, // degraded: fall back to local review implicitly
    };
    let _ = client.record_review(record).await;
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-mcp --test submit_spawn_test
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "mcp: spawn background deep review on dk_submit"
```

---

### Task 2.5: `dk_approve` gate — pending rejection

**Files:**
- Modify: `crates/dk-mcp/src/server.rs` (`dk_approve` tool handler)

**Step 1: Write the failing test**

Append to `tests/submit_spawn_test.rs`:

```rust
#[tokio::test]
async fn approve_rejects_when_deep_review_pending() {
    let harness = testhelpers::McpTestHarness::new()
        .with_env("DKOD_CODE_REVIEW", "1")
        .with_env("DKOD_ANTHROPIC_API_KEY", "sk-mock")
        .with_mock_provider_delay(std::time::Duration::from_secs(10)) // never completes in test window
        .start().await;

    harness.connect("demo/hello").await;
    harness.call_submit("x").await;
    let approve_err = harness.call_approve_expect_error().await;
    assert!(approve_err.contains("deep_review_pending"));
    assert!(approve_err.contains("retry_after_secs"));
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-mcp approve_rejects_when_deep_review_pending
```

Expected: FAIL.

**Step 3: Implement gate — pending branch**

In `dk_approve` handler, before calling `client.approve(...)`:

```rust
let cfg = crate::review_gate::GateConfig::from_env();
if cfg.enabled && !params.force.unwrap_or(false) {
    if cfg.misconfigured() {
        return Ok(CallToolResult::error(vec![Content::text(
            r#"{"error":"gate_misconfigured","message":"DKOD_CODE_REVIEW=1 but no provider key (DKOD_ANTHROPIC_API_KEY or DKOD_OPENROUTER_API_KEY)."}"#
        )]));
    }
    let reviews = client.review(ReviewRequest {
        session_id: session_id.clone(),
        changeset_id: changeset_id.clone(),
    }).await?.into_inner().reviews;
    let deep = reviews.iter().find(|r| r.tier == "deep");
    match deep {
        None => {
            return Ok(CallToolResult::error(vec![Content::text(serde_json::json!({
                "error": "deep_review_pending",
                "message": "Deep code review has not completed yet. Retry dk_approve in ~15s, or poll dk_review.",
                "next_action": { "kind": "wait_and_retry", "retry_after_secs": 15, "can_fix": false }
            }).to_string())]));
        }
        Some(r) => { /* score check — next task */ }
    }
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-mcp approve_rejects_when_deep_review_pending
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "mcp: dk_approve rejects with deep_review_pending when no deep tier"
```

---

### Task 2.6: `dk_approve` gate — below-threshold rejection

**Files:**
- Modify: `crates/dk-mcp/src/server.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn approve_rejects_below_threshold() {
    let harness = testhelpers::McpTestHarness::new()
        .with_env("DKOD_CODE_REVIEW", "1")
        .with_env("DKOD_ANTHROPIC_API_KEY", "sk-mock")
        .with_env("DKOD_REVIEW_MIN_SCORE", "4")
        .with_mock_provider_verdict(ReviewVerdict::RequestChanges)
        .with_mock_provider_finding(Severity::Error, "bad") // score will be 1
        .start().await;
    harness.connect("demo/hello").await;
    harness.call_submit("x").await;
    harness.wait_for_deep_review(Duration::from_secs(3)).await;
    let err = harness.call_approve_expect_error().await;
    assert!(err.contains("review_score_below_threshold"));
    assert!(err.contains("\"score\":1"));
    assert!(err.contains("\"threshold\":4"));
    assert!(err.contains("\"findings\""));
    assert!(err.contains("fix_and_resubmit"));
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-mcp approve_rejects_below_threshold
```

Expected: FAIL — gate passes because score check not implemented.

**Step 3: Extend gate — score branch**

Continue from Task 2.5 `Some(r) => { … }`:

```rust
Some(r) => {
    match r.score {
        None => {
            // provider error under strict policy
            return Ok(CallToolResult::error(vec![Content::text(serde_json::json!({
                "error": "review_provider_error",
                "message": "Deep review failed due to provider error. See findings.",
                "findings": r.findings.clone(),
                "next_action": { "kind": "wait_and_retry", "retry_after_secs": 60, "can_fix": false, "can_override": true }
            }).to_string())]));
        }
        Some(score) if score < cfg.min_score => {
            return Ok(CallToolResult::error(vec![Content::text(serde_json::json!({
                "error": "review_score_below_threshold",
                "message": format!("Deep review score {}/5 is below required {}/5. Fix the findings below and resubmit.", score, cfg.min_score),
                "score": score,
                "threshold": cfg.min_score,
                "findings": r.findings.clone(),
                "next_action": {
                    "kind": "fix_and_resubmit",
                    "can_fix": true,
                    "can_override": true,
                    "override_hint": "If the findings are false positives, call dk_approve(force: true, override_reason: '...')."
                }
            }).to_string())]));
        }
        Some(_) => {} // pass-through to normal approve
    }
}
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-mcp approve_rejects_below_threshold
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "mcp: dk_approve rejects below threshold with inline findings"
```

---

### Task 2.7: `dk_approve` pass-through + success prefix

**Files:**
- Modify: `crates/dk-mcp/src/server.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn approve_passes_when_score_meets_threshold() {
    let harness = testhelpers::McpTestHarness::new()
        .with_env("DKOD_CODE_REVIEW", "1")
        .with_env("DKOD_ANTHROPIC_API_KEY", "sk-mock")
        .with_mock_provider_verdict(ReviewVerdict::Approve)
        .start().await;
    harness.connect("demo/hello").await;
    harness.call_submit("x").await;
    harness.wait_for_deep_review(Duration::from_secs(3)).await;
    let ok = harness.call_approve_expect_ok().await;
    assert!(ok.contains("deep review: 5/5"));
    assert!(ok.contains("Changeset approved!"));
}
```

**Step 2: Run to verify fail**

Run the test; expected failure text: missing `deep review: 5/5` prefix.

**Step 3: Add prefix**

After forwarding to `client.approve(...)`, when gate is enabled and passed:

```rust
let prefix = if cfg.enabled {
    format!("✓ deep review: {}/5 ({}).\n", score, cfg.provider_name.as_deref().unwrap_or("?"))
} else {
    String::new()
};
let text = format!("{prefix}{existing_approved_text}");
```

**Step 4: Run to verify pass**

```bash
cargo test -p dk-mcp approve_passes_when_score_meets_threshold
```

Expected: PASS.

**Step 5: Commit**

```bash
git commit -am "mcp: dk_approve success prefix with score + provider"
```

---

### Task 2.8: `dk_approve` force path with validation

**Files:**
- Modify: `crates/dk-mcp/src/server.rs` (`ApproveParams` struct + handler)

**Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn force_rejects_empty_reason() {
    let h = default_gate_harness().await;
    h.connect("demo/hello").await;
    h.call_submit("x").await;
    let err = h.call_approve_force("").await;
    assert!(err.contains("override_reason"));
}

#[tokio::test]
async fn force_rejects_reason_under_20_chars() {
    let h = default_gate_harness().await;
    h.connect("demo/hello").await;
    h.call_submit("x").await;
    let err = h.call_approve_force("short").await;
    assert!(err.contains("at least 20 characters"));
}

#[tokio::test]
async fn force_succeeds_with_reason_and_stamps_audit() {
    let h = default_gate_harness().await;
    h.connect("demo/hello").await;
    h.call_submit("x").await;
    let ok = h.call_approve_force("API was wedged for 20 minutes").await;
    assert!(ok.contains("force-approved"));
    assert!(h.get_audit_override_reason().await.unwrap().contains("API was wedged"));
}
```

**Step 2: Run to verify fail**

```bash
cargo test -p dk-mcp force_
```

Expected: 3 FAIL — `force` field doesn't exist on `ApproveParams`.

**Step 3: Extend `ApproveParams`**

```rust
#[derive(Deserialize, schemars::JsonSchema)]
struct ApproveParams {
    session_id: Option<String>,
    #[serde(default)]
    force: Option<bool>,
    #[serde(default)]
    override_reason: Option<String>,
}
```

Update tool `description` to mention the two new params.

**Step 4: Implement force branch**

Before the gate code added in 2.5, handle force:

```rust
if params.force.unwrap_or(false) {
    let reason = params.override_reason.as_deref().unwrap_or("").trim();
    if reason.is_empty() {
        return Ok(CallToolResult::error(vec![Content::text(
            "force requires override_reason (non-empty)")]));
    }
    if reason.chars().count() < 20 {
        return Ok(CallToolResult::error(vec![Content::text(
            "override_reason must be at least 20 characters (describe why review is being bypassed)")]));
    }
    // Snapshot current review state for the audit record.
    let snap = if cfg.enabled {
        fetch_review_snapshot(&mut client, &session_id, &changeset_id, cfg.min_score).await.ok()
    } else { None };
    let approve_req = ApproveRequest {
        session_id: session_id.clone(),
        override_reason: Some(reason.to_string()),
        review_snapshot: snap,
    };
    let resp = client.approve(approve_req).await?.into_inner();
    let text = format!(
        "Changeset approved!\nchangeset_id: {}\nstate: {}\n⚠ force-approved: {}\n",
        resp.changeset_id, resp.new_state, reason);
    return Ok(CallToolResult::success(vec![Content::text(text)]));
}
```

**Step 5: Run to verify pass**

```bash
cargo test -p dk-mcp force_
```

Expected: 3 PASS.

**Step 6: Commit**

```bash
git commit -am "mcp: dk_approve force path with 20-char reason requirement and snapshot"
```

---

### Task 2.9: `dk_approve` disabled-flag guard for `force`

**Files:**
- Modify: `crates/dk-mcp/src/server.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn force_is_noop_when_gate_disabled() {
    // No DKOD_CODE_REVIEW set
    let h = testhelpers::McpTestHarness::new().start().await;
    h.connect("demo/hello").await;
    h.call_submit("x").await;
    // force: true with no reason should succeed (gate is disabled, force is no-op)
    let ok = h.call_approve_force_no_reason().await;
    assert!(ok.contains("Changeset approved"));
    assert!(!ok.contains("force-approved"));
}
```

**Step 2-3: Implement & run**

When `!cfg.enabled`, skip the `force` validation branch entirely — treat as a normal approve and log once to stderr: `force requested but gate is disabled — proceeding as normal approve`.

**Step 4: Commit**

```bash
git commit -am "mcp: force is no-op when DKOD_CODE_REVIEW unset"
```

---

### Task 2.10: Startup warning for misconfigured gate

**Files:**
- Modify: `crates/dk-mcp/src/main.rs` or wherever MCP startup lives

**Step 1: Manual verification**

Unsetting provider keys and starting MCP with only `DKOD_CODE_REVIEW=1` must emit one stderr warning:

```
[dk-mcp] WARNING: DKOD_CODE_REVIEW=1 but no provider key set. dk_approve will reject with gate_misconfigured until DKOD_ANTHROPIC_API_KEY or DKOD_OPENROUTER_API_KEY is set.
```

**Step 2: Implement**

At startup after `tracing_subscriber::fmt()`:

```rust
let cfg = dk_mcp::review_gate::GateConfig::from_env();
if cfg.misconfigured() {
    eprintln!("[dk-mcp] WARNING: DKOD_CODE_REVIEW=1 but no provider key set. dk_approve will reject with gate_misconfigured until DKOD_ANTHROPIC_API_KEY or DKOD_OPENROUTER_API_KEY is set.");
}
```

**Step 3: Add smoke test** (captures stderr with `assert_cmd`):

```rust
#[test]
fn misconfigured_prints_startup_warning() {
    use assert_cmd::Command;
    let out = Command::cargo_bin("dk-mcp").unwrap()
        .env("DKOD_CODE_REVIEW", "1")
        .env_remove("DKOD_ANTHROPIC_API_KEY")
        .env_remove("DKOD_OPENROUTER_API_KEY")
        .arg("--print-startup-banner-then-exit")  // a new convenience flag for tests
        .output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("DKOD_CODE_REVIEW=1 but no provider key"));
}
```

**Step 4: Commit**

```bash
git commit -am "mcp: startup warning for misconfigured gate"
```

---

### Task 2.11: Open PR 2

```bash
git push -u origin feat/mcp-code-review-gate
gh pr create --title "feat: MCP-layer code review gate (opt-in via DKOD_CODE_REVIEW=1)" --body "$(cat <<'EOF'
## Summary
- Background deep-review task spawned on `dk_submit` when `DKOD_CODE_REVIEW=1` + provider key
- `dk_approve` gate with 3 structured rejections: `deep_review_pending`, `review_score_below_threshold`, `review_provider_error`
- `force: true` + `override_reason` (≥20 chars) bypass with audit snapshot
- Startup warning when flag is set but no key

Depends on: PR 1 (RecordReview RPC + ApproveRequest fields + OpenRouter provider).

Design: `docs/plans/2026-04-16-mcp-code-review-gate-design.md`.

## Test plan
- [ ] `cargo test -p dk-mcp` passes (all new tests)
- [ ] Manual: `DKOD_CODE_REVIEW=1 DKOD_ANTHROPIC_API_KEY=sk-...` → submit → wait → approve succeeds with `✓ deep review: N/5`
- [ ] Manual: force path with 30-char reason → `⚠ force-approved` in output
- [ ] Manual: force with 5-char reason → rejected
- [ ] Gate off (no env): zero behavior change from today

EOF
)"
```

---

## Phase 3 — Plugin + Harness polish (PR 3a, PR 3b)

### Task 3.1: `/dkod:land` threshold from env

Repo: `dkod-plugin`
Branch: `feat/land-respects-min-score`
`cd /Users/haimari/vsCode/haim-ari/github/dkod-plugin`

**Files:**
- Modify: `commands/land.md`

**Step 1: Edit**

Change the hardcoded `3` threshold note to:

```markdown
a. Call `dk_review` to check the code review score and findings
   - If `DKOD_CODE_REVIEW=1` is set, use `DKOD_REVIEW_MIN_SCORE` (default 4) as the threshold
   - Otherwise use the legacy threshold of 3
   - Score >= threshold and no "error" findings -> proceed to approve
   - Score < threshold or "error" findings -> report findings to user, do NOT approve
```

**Step 2: Commit & PR**

```bash
git checkout -b feat/land-respects-min-score
git commit -am "docs(land): respect DKOD_REVIEW_MIN_SCORE when gate enabled"
git push -u origin feat/land-respects-min-score
gh pr create --title "docs(land): respect DKOD_REVIEW_MIN_SCORE when code review gate is enabled"  --body "Small doc update so /dkod:land matches the MCP gate threshold when users opt in via DKOD_CODE_REVIEW=1. See dkod-engine design doc 2026-04-16-mcp-code-review-gate-design.md."
```

---

### Task 3.2: `dk_push` PR-body override note

Repo: `dkod-engine`
Branch: `feat/push-override-note`

**Files:**
- Modify: `crates/dk-engine/src/push.rs` (PR body composer)

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn pr_body_includes_override_section_when_any_changeset_overridden() {
    let engine = Engine::new_in_memory().await;
    let c1 = engine.create_approved_changeset().await;
    engine.set_override_reason(&c1, "API wedged for 20 minutes, reviewed manually").await;
    let body = engine.compose_pr_body(&[c1]).await;
    assert!(body.contains("## Review overrides"));
    assert!(body.contains("API wedged"));
}
```

**Step 2: Implement**

When composing the PR body, append a `## Review overrides` section listing all changesets with `override_reason`. Include `(score X/5, threshold Y/5)` from the `review_snapshot`.

**Step 3: Commit & PR**

```bash
git commit -am "engine: dk_push includes Review overrides section in PR body"
git push -u origin feat/push-override-note
gh pr create --title "feat(push): PR body 'Review overrides' section for force-approved changesets" --body "..."
```

---

### Task 3.3: Harness orchestrator PRE-FLIGHT + fix-loop

Repo: `dkod-harness`
Branch: `feat/orchestrator-review-gate`
`cd /Users/haimari/vsCode/haim-ari/github/dkod-harness`

**Files:**
- Modify: `harness/skills/dkh/agents/orchestrator.md`

**Step 1: Add PRE-FLIGHT section**

Insert in the orchestrator's PRE-FLIGHT section:

```markdown
### Code review gate state

Read env at startup:

- `DKOD_CODE_REVIEW` — if `1`, gate is enabled
- `DKOD_ANTHROPIC_API_KEY` / `DKOD_OPENROUTER_API_KEY` — provider keys
- `DKOD_REVIEW_MIN_SCORE` — threshold (default 4)

Log to the event stream exactly one of:

- `code_review: disabled` — no gate, land pipeline uses legacy threshold 3
- `code_review: enabled (provider=<name>, min_score=<n>)` — gate on, apply LAND-phase rules below
- `code_review: misconfigured (flag set but no key)` — **abort** immediately with a clear message; do not launch generators
```

**Step 2: Add LAND-phase rules**

```markdown
### LAND phase — when code_review is enabled

Between `dk_verify` and `dk_approve`:

1. Call `dk_review` for the changeset; require a `tier: "deep"` result with `score >= DKOD_REVIEW_MIN_SCORE`.
2. If no deep tier → `dk_watch` for `changeset.review.completed`, timeout 180s. On timeout, fall through; `dk_approve` rejects cleanly and you re-enter this step.
3. If `score < min_score` → dispatch a fix-agent (generator template) with the findings as the prompt. Fix agent writes → submits → MCP fires a new deep review. Wait and re-check.
4. Cap at 3 fix rounds per changeset. On exceed:
   - Either **force-approve** with `override_reason: "Exceeded 3 review fix rounds; findings: <short list>"`
   - Or fail the unit and document in the eval report.

### ONLY FOR ORCHESTRATOR — force-approve

Only the orchestrator calls `dk_approve(force: true, override_reason: …)`. Generators never force. The reason must be concrete and ≥20 characters.
```

**Step 3: Commit & PR**

```bash
git checkout -b feat/orchestrator-review-gate
git commit -am "orchestrator: PRE-FLIGHT gate detection + LAND fix-loop + force-approve discipline"
git push -u origin feat/orchestrator-review-gate
gh pr create --title "feat(orchestrator): integrate MCP review gate with PRE-FLIGHT + fix-loop" --body "..."
```

---

### Task 3.4: Harness smoke test

Repo: `dkod-harness`

**Files:**
- Create: `harness/tests/review_gate_e2e.test.ts`

**Step 1: Write**

Integration test that runs the harness end-to-end on a small repo with `DKOD_CODE_REVIEW=1` + mock provider → asserts the orchestrator either succeeds or force-approves with a reason mentioning "Exceeded 3 review fix rounds".

**Step 2-5: TDD cycle + commit**

```bash
git commit -am "test: e2e review-gate + fix-loop + force fallback"
```

---

## Phase 4 — Manual verification (no commit)

Before announcing this feature:

1. `cd dkod-engine && cargo build --release && ./target/release/dk-mcp ...` — self-hosted MCP.
2. `claude mcp add dkod http://localhost:50051` — connect Claude Code to the self-hosted MCP.
3. `export DKOD_CODE_REVIEW=1 DKOD_ANTHROPIC_API_KEY=sk-...` — enable gate with Anthropic.
4. In a test repo: make a trivial change, `dk_submit`, observe hint line about background review.
5. Immediately `dk_approve` → expect `deep_review_pending` JSON.
6. Wait ~30s, `dk_approve` again → expect `✓ deep review: X/5` prefix + success.
7. Induce a low-score change (e.g. add an obvious bug), `dk_submit`, wait, `dk_approve` → expect `review_score_below_threshold` with inline findings.
8. Run `dk_approve(force: true, override_reason: "…30-char reason…")` → expect `⚠ force-approved`, verify dashboard shows OVERRIDDEN.
9. Swap to `DKOD_OPENROUTER_API_KEY=sk-or-...` and repeat step 6 — confirm provider shown is `openrouter`.
10. Unset `DKOD_CODE_REVIEW` — confirm behavior is identical to before this feature.

---

## Execution order summary

1. **PR 1** (dkod-engine) — `feat/record-review-rpc` — dormant capability
2. **PR 2** (dkod-engine) — `feat/mcp-code-review-gate` — ship gate behind flag
3. **PR 3a** (dkod-plugin) — `feat/land-respects-min-score` — doc update
4. **PR 3b** (dkod-engine) — `feat/push-override-note` — PR body polish
5. **PR 3c** (dkod-harness) — `feat/orchestrator-review-gate` — harness integration
6. Manual verification (Phase 4)

PRs 1 and 2 must land in order. PRs 3a/3b/3c are independent and can land in any order once PR 2 is merged.

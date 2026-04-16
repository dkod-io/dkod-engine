//! Environment-derived configuration for the MCP code-review gate.
//!
//! The gate is an opt-in deep code review that runs before `dk_approve` merges
//! a changeset, to catch regressions the generator's local review missed. This
//! module parses the seven `DKOD_*` environment variables into a single
//! [`GateConfig`] value that the server consults at request time.
//!
//! See `docs/plans/2026-04-16-mcp-code-review-gate-design.md` for the broader
//! design, the gate wiring into `server.rs`, and the review-provider contract.

use std::time::Duration;

use dk_runner::steps::agent_review::provider::{
    FileContext, ReviewProvider, ReviewRequest, ReviewResponse, ReviewVerdict,
};
use dk_runner::findings::{Finding, Severity};

/// Map a review verdict + findings list to a 1–5 integer score.
///
/// - `Approve` + no findings → 5
/// - `Approve` + any warning/error → 4
/// - `Comment` → 3
/// - `RequestChanges` with only warnings → 2
/// - `RequestChanges` with any error → 1
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

/// Effective gate settings derived from the environment at a point in time.
#[derive(Debug, Clone)]
pub struct GateConfig {
    /// `true` when `DKOD_CODE_REVIEW=1` — the gate is requested for this process.
    pub enabled: bool,
    /// Name of the selected provider (`"anthropic"` or `"openrouter"`), or
    /// `None` if no provider key is set in the environment.
    pub provider_name: Option<String>,
    /// Minimum review score (1..=5) a changeset must achieve to pass the gate.
    /// Defaults to 4. Out-of-range or unparseable values fall back to the default.
    pub min_score: i32,
    /// Maximum time allowed for a single review call before the backoff policy
    /// takes over. Defaults to 180 seconds.
    pub timeout: Duration,
    /// How provider errors and timeouts are handled — see [`BackoffPolicy`].
    pub backoff_policy: BackoffPolicy,
    /// Optional provider-specific model override (e.g. `anthropic/claude-sonnet-4-5`).
    /// When `None`, the provider implementation picks its default model.
    pub model: Option<String>,
}

/// How the gate reacts when the remote review provider errors or times out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffPolicy {
    /// Provider errors are recorded as score=None and reject on approve.
    Strict,
    /// Falls back to local review silently on provider error.
    Degraded,
}

/// Outcome of evaluating the deep-review gate for a single `dk_approve` call.
pub enum GateOutcome {
    /// Gate disabled or gate passed without a deep-review summary to report —
    /// proceed to forward approve() to the engine with no extra prefix.
    Pass,
    /// Gate passed with a deep-review score. The contained string is a
    /// one-line human-readable prefix (e.g. `"[\u{2713}] deep review: 5/5 (anthropic).\n"`)
    /// to prepend to the approve() success text.
    PassWithPrefix(String),
    /// Reject with a structured JSON payload to send back to the caller.
    Reject(String),
}

/// Outcome of validating an `override_reason` supplied to a force-approve call.
///
/// Used by [`validate_override_reason`] to separate the three shapes the
/// server layer needs to surface distinct error messages for:
/// absent/empty, present-but-too-short, and well-formed.
#[derive(Debug)]
pub enum OverrideReasonValidation {
    /// Trimmed reason is at least 20 characters — carries the trimmed string
    /// ready to forward to the engine.
    Ok(String),
    /// Reason is `None`, empty, or all-whitespace.
    Empty,
    /// Reason is present but trimmed character count is `< 20`; carries the
    /// actual length so the error can include it in the diagnostic.
    TooShort(usize),
}

/// Validate the `override_reason` supplied to a force-approve call.
///
/// Trims leading/trailing whitespace and counts the remaining Unicode scalar
/// values (not bytes) to enforce the 20-character minimum. Unicode-aware so
/// that, e.g., `"🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀"` is rejected as 12 chars, not
/// accepted as ~48 bytes.
pub fn validate_override_reason(reason: Option<&str>) -> OverrideReasonValidation {
    let trimmed = reason.unwrap_or("").trim();
    if trimmed.is_empty() {
        return OverrideReasonValidation::Empty;
    }
    let len = trimmed.chars().count();
    if len < 20 {
        return OverrideReasonValidation::TooShort(len);
    }
    OverrideReasonValidation::Ok(trimmed.to_string())
}

/// Build a [`crate::ReviewSnapshot`] from the current deep review (if any) and
/// gate config, capturing an audit record of the review state at the moment
/// `dk_approve(force: true)` was called.
///
/// When no deep review exists (provider not yet finished, background spawn
/// failed, etc.) the snapshot records `score = Some(0)`, `findings_count = 0`,
/// and the configured threshold so the audit row is still self-describing.
/// The `provider` and `model` fields are taken from [`GateConfig`] rather than
/// the review result because the review result has no provider field.
pub fn build_review_snapshot(
    deep_review: Option<&crate::ReviewResultProto>,
    cfg: &GateConfig,
) -> crate::ReviewSnapshot {
    let provider = cfg.provider_name.clone().unwrap_or_default();
    let model = cfg.model.clone().unwrap_or_default();
    match deep_review {
        Some(r) => crate::ReviewSnapshot {
            score: Some(r.score.unwrap_or(0)),
            threshold: Some(cfg.min_score),
            findings_count: r.findings.len() as u32,
            provider,
            model,
        },
        None => crate::ReviewSnapshot {
            // `None` distinguishes "force-approved with no deep review at all"
            // from `Some(0)` which is reserved for "provider errored mid-review"
            // under strict backoff policy. Audit consumers rely on this.
            score: None,
            threshold: Some(cfg.min_score),
            findings_count: 0,
            provider,
            model,
        },
    }
}

/// Serializable projection of [`crate::ReviewFindingProto`] for embedding in
/// gate rejection payloads. prost-generated types do not derive `Serialize`,
/// so we hand-copy the fields we want to surface.
#[derive(serde::Serialize)]
struct FindingJson<'a> {
    severity: &'a str,
    category: &'a str,
    message: &'a str,
    file_path: &'a str,
    line_start: Option<i32>,
}

fn findings_to_json(findings: &[crate::ReviewFindingProto]) -> Vec<FindingJson<'_>> {
    findings
        .iter()
        .map(|f| FindingJson {
            severity: &f.severity,
            category: &f.category,
            message: &f.message,
            file_path: &f.file_path,
            line_start: f.line_start,
        })
        .collect()
}

/// Evaluate the gate for a `dk_approve` call.
///
/// - `!cfg.enabled` → `Pass` (no gate).
/// - `force` → `Pass` (caller-supplied override; reason validation is handled
///   in the server layer).
/// - `cfg.misconfigured()` → `Reject` with a `gate_misconfigured` payload.
/// - `deep_review` is `None` → `Reject` with a `deep_review_pending` payload
///   that instructs the caller to retry in ~15 seconds.
/// - `deep_review` is `Some(r)` with `r.score` = `None` → `Reject` with a
///   `review_provider_error` payload (retry in 60s, can_override).
/// - `deep_review` is `Some(r)` with `r.score < cfg.min_score` → `Reject` with
///   a `review_score_below_threshold` payload including the inline findings.
/// - `deep_review` is `Some(r)` with `r.score >= cfg.min_score` → `Pass`.
pub fn evaluate_gate(
    cfg: &GateConfig,
    force: bool,
    deep_review: Option<&crate::ReviewResultProto>,
) -> GateOutcome {
    if !cfg.enabled {
        return GateOutcome::Pass;
    }
    if force {
        return GateOutcome::Pass;
    }
    if cfg.misconfigured() {
        let body = serde_json::json!({
            "error": "gate_misconfigured",
            "message": "DKOD_CODE_REVIEW=1 but no provider key (DKOD_ANTHROPIC_API_KEY or DKOD_OPENROUTER_API_KEY).",
        });
        return GateOutcome::Reject(body.to_string());
    }
    let Some(r) = deep_review else {
        let body = serde_json::json!({
            "error": "deep_review_pending",
            "message": "Deep code review has not completed yet. Retry dk_approve in ~15s, or poll dk_review.",
            "next_action": {
                "kind": "wait_and_retry",
                "retry_after_secs": 15,
                "can_fix": false,
            },
        });
        return GateOutcome::Reject(body.to_string());
    };
    let findings_val =
        serde_json::to_value(findings_to_json(&r.findings)).unwrap_or(serde_json::Value::Null);
    match r.score {
        None => {
            let body = serde_json::json!({
                "error": "review_provider_error",
                "message": "Deep review failed due to provider error. See findings.",
                "findings": findings_val,
                "next_action": {
                    "kind": "wait_and_retry",
                    "retry_after_secs": 60,
                    "can_fix": false,
                    "can_override": true,
                },
            });
            GateOutcome::Reject(body.to_string())
        }
        Some(score) if score < cfg.min_score => {
            let body = serde_json::json!({
                "error": "review_score_below_threshold",
                "message": format!(
                    "Deep review score {}/5 is below required {}/5. Fix the findings below and resubmit.",
                    score, cfg.min_score
                ),
                "score": score,
                "threshold": cfg.min_score,
                "findings": findings_val,
                "next_action": {
                    "kind": "fix_and_resubmit",
                    "can_fix": true,
                    "can_override": true,
                    "override_hint": "If the findings are false positives, call dk_approve(force: true, override_reason: '...').",
                },
            });
            GateOutcome::Reject(body.to_string())
        }
        Some(score) => {
            let provider = cfg.provider_name.as_deref().unwrap_or("?");
            let prefix = format!("[\u{2713}] deep review: {score}/5 ({provider}).\n");
            GateOutcome::PassWithPrefix(prefix)
        }
    }
}

/// Collect one-line warning messages to emit at MCP startup based on the
/// effective gate config. Currently yields at most one warning.
pub fn startup_warnings(cfg: &GateConfig) -> Vec<String> {
    let mut out = Vec::new();
    if cfg.misconfigured() {
        out.push(
            "[dk-mcp] WARNING: DKOD_CODE_REVIEW=1 but no provider key set. \
             dk_approve will reject with gate_misconfigured until \
             DKOD_ANTHROPIC_API_KEY or DKOD_OPENROUTER_API_KEY is set.".to_string()
        );
    }
    out
}

impl GateConfig {
    /// Parse the gate configuration from the current process environment.
    ///
    /// Reads seven variables: `DKOD_CODE_REVIEW` (enable flag; only `"1"` enables),
    /// `DKOD_OPENROUTER_API_KEY` and `DKOD_ANTHROPIC_API_KEY` (provider selection —
    /// OpenRouter wins when both are set), `DKOD_REVIEW_MIN_SCORE` (default 4,
    /// valid 1..=5), `DKOD_REVIEW_TIMEOUT_SECS` (default 180),
    /// `DKOD_REVIEW_BACKOFF_POLICY` (`"degraded"` selects [`BackoffPolicy::Degraded`];
    /// anything else — including unset — is [`BackoffPolicy::Strict`]), and
    /// `DKOD_REVIEW_MODEL` (optional model override, forwarded to the provider).
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

    /// Returns `true` when the gate flag is enabled but no provider key is set —
    /// the caller should fail closed.
    pub fn misconfigured(&self) -> bool {
        self.enabled && self.provider_name.is_none()
    }
}

/// Map a [`Finding`] into the wire-level [`ReviewFindingProto`] used by the
/// `RecordReview` RPC. Generates a fresh UUID for the `id` field because
/// [`Finding`] is an in-memory type without a stable identifier.
fn finding_to_proto(finding: &Finding) -> crate::ReviewFindingProto {
    crate::ReviewFindingProto {
        id: uuid::Uuid::new_v4().to_string(),
        file_path: finding.file_path.clone().unwrap_or_default(),
        line_start: finding.line.map(|l| l as i32),
        line_end: None,
        severity: finding.severity.as_str().to_string(),
        category: finding.check_name.clone(),
        message: finding.message.clone(),
        suggestion: None,
        // Providers don't surface per-finding probabilities today; `1.0` treats
        // the finding as fully returned (the LLM did surface it) rather than
        // `0.0` which would mean "no confidence" and is semantically wrong.
        confidence: 1.0,
        dismissed: false,
    }
}

/// Construct a synthetic [`Finding`] describing a provider error (HTTP 5xx,
/// timeout, parse failure). Used by [`build_record_review_request`] under the
/// [`BackoffPolicy::Strict`] policy so the gate can record score=None with a
/// human-readable explanation of the failure.
fn provider_error_finding(err_msg: String) -> Finding {
    Finding {
        severity: Severity::Error,
        check_name: "provider_error".to_string(),
        message: err_msg,
        file_path: None,
        line: None,
        symbol: None,
    }
}

/// Select a [`ReviewProvider`] for the deep-review background task.
///
/// Delegates to `dk_runner::steps::agent_review::select_provider_from_env` so
/// the MCP gate uses the same OpenRouter-over-Anthropic precedence as the
/// generator-side review step. Returns `None` when no provider key is set.
///
/// `_cfg` is accepted for future use (provider-specific options like the model
/// override) but is currently unused — the provider reads its config from the
/// environment directly.
fn select_provider(_cfg: &GateConfig) -> Option<Box<dyn ReviewProvider>> {
    dk_runner::steps::agent_review::select_provider_from_env()
}

/// Connect to the dkod gRPC server with the given bearer token. Returns `None`
/// when no token is available (the server requires authenticated calls) or
/// when the dial fails — the background review task swallows the error
/// silently.
async fn connect_grpc(
    grpc_addr: String,
    auth_token: Option<String>,
) -> Option<crate::grpc::AuthenticatedClient> {
    let token = auth_token?;
    match crate::grpc::connect_with_auth(&grpc_addr, token).await {
        Ok(c) => Some(c),
        Err(err) => {
            tracing::debug!(error = %err, addr = %grpc_addr, "background review: gRPC connect failed");
            None
        }
    }
}

/// Build the `RecordReview` wire message from the provider call result.
///
/// Pure helper extracted so it can be unit-tested without spawning tasks or
/// opening a gRPC channel.
///
/// Returns:
/// - `Some(req)` when the provider succeeded (score set from verdict+findings).
/// - `Some(req)` with `score: None` when the provider errored AND the config
///   uses [`BackoffPolicy::Strict`] — the gate records the failure explicitly.
/// - `None` when the provider errored under [`BackoffPolicy::Degraded`] — the
///   gate falls back silently and does not record a deep review.
pub fn build_record_review_request(
    result: Result<ReviewResponse, anyhow::Error>,
    elapsed: Duration,
    session_id: &str,
    changeset_id: &str,
    provider_name: &str,
    cfg: &GateConfig,
) -> Option<crate::RecordReviewRequest> {
    match result {
        Ok(resp) => {
            let score = score_from_verdict(&resp.verdict, &resp.findings);
            let findings = resp.findings.iter().map(finding_to_proto).collect();
            Some(crate::RecordReviewRequest {
                session_id: session_id.to_string(),
                changeset_id: changeset_id.to_string(),
                tier: "deep".to_string(),
                score: Some(score),
                summary: Some(resp.summary),
                findings,
                provider: provider_name.to_string(),
                model: cfg.model.clone().unwrap_or_default(),
                duration_ms: elapsed.as_millis() as i64,
            })
        }
        Err(err) => match cfg.backoff_policy {
            BackoffPolicy::Strict => {
                let finding = provider_error_finding(err.to_string());
                let findings = vec![finding_to_proto(&finding)];
                Some(crate::RecordReviewRequest {
                    session_id: session_id.to_string(),
                    changeset_id: changeset_id.to_string(),
                    tier: "deep".to_string(),
                    score: None,
                    summary: Some(format!("provider error: {err}")),
                    findings,
                    provider: provider_name.to_string(),
                    model: cfg.model.clone().unwrap_or_default(),
                    duration_ms: elapsed.as_millis() as i64,
                })
            }
            BackoffPolicy::Degraded => None,
        },
    }
}

/// Run a deep code review in the background and record the result via the
/// `RecordReview` gRPC. Fire-and-forget — returns silently on every error
/// path (no provider configured, no auth token, dial failure, RPC failure).
///
/// Diff + context are passed as empty for now; the MCP server does not yet
/// have access to a unified diff without adding new RPCs. The gate design
/// accepts this tradeoff — the gate mechanism is what matters; the review
/// can be enriched in a follow-up PR.
pub async fn run_background_review(
    grpc_addr: String,
    auth_token: Option<String>,
    session_id: String,
    changeset_id: String,
    diff: String,
    context: Vec<FileContext>,
    cfg: GateConfig,
) {
    let provider = match select_provider(&cfg) {
        Some(p) => p,
        None => {
            tracing::debug!("background review: no provider configured");
            return;
        }
    };
    let provider_name = provider.name().to_string();
    let start = std::time::Instant::now();

    let review_future = provider.review(ReviewRequest {
        diff,
        context,
        language: "rust".into(),
        intent: "deep review".into(),
    });

    let timeout_result = tokio::time::timeout(cfg.timeout, review_future).await;
    let elapsed = start.elapsed();

    let call_result: Result<ReviewResponse, anyhow::Error> = match timeout_result {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(anyhow::anyhow!(
            "deep review timed out after {:?}",
            cfg.timeout
        )),
    };

    let record = match build_record_review_request(
        call_result,
        elapsed,
        &session_id,
        &changeset_id,
        &provider_name,
        &cfg,
    ) {
        Some(r) => r,
        None => {
            tracing::debug!(
                session_id = %session_id,
                changeset_id = %changeset_id,
                "background review: provider errored under degraded policy — skipping record"
            );
            return;
        }
    };

    let mut client = match connect_grpc(grpc_addr, auth_token).await {
        Some(c) => c,
        None => return,
    };

    if let Err(e) = client.record_review(record).await {
        tracing::debug!(error = %e, "background review: record_review RPC failed");
    }
}

#[cfg(test)]
mod env_parsing_tests {
    use super::GateConfig;
    use std::sync::Mutex;

    // Tests mutate process-global env vars; serialize to avoid cross-test races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_all() {
        for k in ["DKOD_CODE_REVIEW", "DKOD_ANTHROPIC_API_KEY", "DKOD_OPENROUTER_API_KEY",
                  "DKOD_REVIEW_MIN_SCORE", "DKOD_REVIEW_TIMEOUT_SECS",
                  "DKOD_REVIEW_BACKOFF_POLICY", "DKOD_REVIEW_MODEL"] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn disabled_when_flag_unset() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();
        assert!(!GateConfig::from_env().enabled);
    }

    #[test]
    fn enabled_with_anthropic_key() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_all();
        std::env::set_var("DKOD_CODE_REVIEW", "1");
        std::env::set_var("DKOD_ANTHROPIC_API_KEY", "sk-ant");
        std::env::set_var("DKOD_REVIEW_MIN_SCORE", "banana");
        let cfg = GateConfig::from_env();
        assert_eq!(cfg.min_score, 4);
        clear_all();
    }
}

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

#[cfg(test)]
mod evaluate_gate_tests {
    use super::*;
    use crate::ReviewResultProto;

    fn cfg_off() -> GateConfig {
        GateConfig {
            enabled: false,
            provider_name: None,
            min_score: 4,
            timeout: std::time::Duration::from_secs(180),
            backoff_policy: BackoffPolicy::Strict,
            model: None,
        }
    }
    fn cfg_on() -> GateConfig {
        GateConfig {
            enabled: true,
            provider_name: Some("anthropic".into()),
            min_score: 4,
            timeout: std::time::Duration::from_secs(180),
            backoff_policy: BackoffPolicy::Strict,
            model: None,
        }
    }
    fn cfg_misconfigured() -> GateConfig {
        GateConfig {
            enabled: true,
            provider_name: None,
            min_score: 4,
            timeout: std::time::Duration::from_secs(180),
            backoff_policy: BackoffPolicy::Strict,
            model: None,
        }
    }

    #[test]
    fn pass_when_gate_disabled() {
        assert!(matches!(
            evaluate_gate(&cfg_off(), false, None),
            GateOutcome::Pass
        ));
    }
    #[test]
    fn pass_when_force_even_if_pending() {
        assert!(matches!(
            evaluate_gate(&cfg_on(), true, None),
            GateOutcome::Pass
        ));
    }
    #[test]
    fn reject_misconfigured() {
        let GateOutcome::Reject(body) = evaluate_gate(&cfg_misconfigured(), false, None) else {
            panic!()
        };
        assert!(body.contains("gate_misconfigured"));
        assert!(body.contains("DKOD_CODE_REVIEW"));
    }
    #[test]
    fn reject_deep_review_pending() {
        let GateOutcome::Reject(body) = evaluate_gate(&cfg_on(), false, None) else {
            panic!()
        };
        assert!(body.contains("deep_review_pending"));
        assert!(body.contains("\"retry_after_secs\":15"));
        assert!(body.contains("\"can_fix\":false"));
    }

    fn deep(score: Option<i32>, findings: Vec<crate::ReviewFindingProto>) -> ReviewResultProto {
        ReviewResultProto {
            id: "r1".into(),
            tier: "deep".into(),
            score,
            summary: None,
            findings,
            created_at: "".into(),
        }
    }

    #[test]
    fn reject_below_threshold() {
        let r = deep(Some(2), vec![]);
        let GateOutcome::Reject(body) = evaluate_gate(&cfg_on(), false, Some(&r)) else {
            panic!()
        };
        assert!(body.contains("review_score_below_threshold"));
        assert!(body.contains("\"score\":2"));
        assert!(body.contains("\"threshold\":4"));
        assert!(body.contains("fix_and_resubmit"));
    }
    #[test]
    fn reject_provider_error_when_score_none() {
        let r = deep(None, vec![]);
        let GateOutcome::Reject(body) = evaluate_gate(&cfg_on(), false, Some(&r)) else {
            panic!()
        };
        assert!(body.contains("review_provider_error"));
        assert!(body.contains("\"retry_after_secs\":60"));
    }
    #[test]
    fn pass_at_threshold_exactly() {
        let r = deep(Some(4), vec![]);
        assert!(matches!(
            evaluate_gate(&cfg_on(), false, Some(&r)),
            GateOutcome::PassWithPrefix(_)
        ));
    }
    #[test]
    fn pass_above_threshold() {
        let r = deep(Some(5), vec![]);
        assert!(matches!(
            evaluate_gate(&cfg_on(), false, Some(&r)),
            GateOutcome::PassWithPrefix(_)
        ));
    }

    #[test]
    fn pass_with_prefix_when_above_threshold() {
        let r = deep(Some(5), vec![]);
        let GateOutcome::PassWithPrefix(p) = evaluate_gate(&cfg_on(), false, Some(&r)) else {
            panic!()
        };
        assert!(p.contains("deep review: 5/5"));
        assert!(p.contains("anthropic"));
    }
    #[test]
    fn pass_with_prefix_at_threshold() {
        let r = deep(Some(4), vec![]);
        let GateOutcome::PassWithPrefix(p) = evaluate_gate(&cfg_on(), false, Some(&r)) else {
            panic!()
        };
        assert!(p.contains("deep review: 4/5"));
    }
    #[test]
    fn no_prefix_when_gate_disabled() {
        let r = deep(Some(5), vec![]);
        assert!(matches!(
            evaluate_gate(&cfg_off(), false, Some(&r)),
            GateOutcome::Pass
        ));
    }
}

#[cfg(test)]
mod run_background_review_tests {
    use super::*;
    use dk_runner::findings::{Finding, Severity};
    use dk_runner::steps::agent_review::provider::{ReviewResponse, ReviewVerdict};
    use std::time::Duration;

    fn cfg_strict() -> GateConfig {
        GateConfig {
            enabled: true,
            provider_name: Some("anthropic".into()),
            min_score: 4,
            timeout: Duration::from_secs(180),
            backoff_policy: BackoffPolicy::Strict,
            model: None,
        }
    }

    #[test]
    fn builds_request_with_score_5_on_clean_approve() {
        let resp = ReviewResponse {
            summary: "OK".into(),
            findings: vec![],
            suggestions: vec![],
            verdict: ReviewVerdict::Approve,
        };
        let req = build_record_review_request(
            Ok(resp),
            Duration::from_millis(42),
            "s1",
            "c1",
            "anthropic",
            &cfg_strict(),
        )
        .unwrap();
        assert_eq!(req.session_id, "s1");
        assert_eq!(req.changeset_id, "c1");
        assert_eq!(req.tier, "deep");
        assert_eq!(req.score, Some(5));
        assert_eq!(req.provider, "anthropic");
        assert_eq!(req.duration_ms, 42);
        assert!(req.findings.is_empty());
    }

    #[test]
    fn builds_request_with_score_1_on_request_changes_with_error() {
        let bad = Finding {
            severity: Severity::Error,
            check_name: "x".into(),
            message: "m".into(),
            file_path: None,
            line: None,
            symbol: None,
        };
        let resp = ReviewResponse {
            summary: "bad".into(),
            findings: vec![bad],
            suggestions: vec![],
            verdict: ReviewVerdict::RequestChanges,
        };
        let req = build_record_review_request(
            Ok(resp),
            Duration::from_millis(100),
            "s",
            "c",
            "anthropic",
            &cfg_strict(),
        )
        .unwrap();
        assert_eq!(req.score, Some(1));
        assert_eq!(req.findings.len(), 1);
        assert_eq!(req.findings[0].severity, "error");
    }

    #[test]
    fn builds_error_record_when_strict_and_provider_errored() {
        let req = build_record_review_request(
            Err(anyhow::anyhow!("500 from provider")),
            Duration::from_millis(10),
            "s",
            "c",
            "anthropic",
            &cfg_strict(),
        )
        .unwrap();
        assert_eq!(req.score, None);
        assert_eq!(req.findings.len(), 1);
        assert_eq!(req.findings[0].severity, "error");
        assert!(req.findings[0].message.contains("500 from provider"));
    }

    #[test]
    fn returns_none_when_degraded_and_provider_errored() {
        let mut cfg = cfg_strict();
        cfg.backoff_policy = BackoffPolicy::Degraded;
        let req = build_record_review_request(
            Err(anyhow::anyhow!("timeout")),
            Duration::from_millis(10),
            "s",
            "c",
            "anthropic",
            &cfg,
        );
        assert!(req.is_none());
    }

    #[test]
    fn finding_to_proto_maps_severity_case() {
        let finding = Finding {
            severity: Severity::Warning,
            check_name: "cat".into(),
            message: "msg".into(),
            file_path: Some("f.rs".into()),
            line: Some(7),
            symbol: None,
        };
        let p = finding_to_proto(&finding);
        assert_eq!(p.severity, "warning");
        assert_eq!(p.file_path, "f.rs");
        assert_eq!(p.line_start, Some(7));
        assert_eq!(p.category, "cat");
        assert_eq!(p.message, "msg");
        assert!(!p.id.is_empty()); // UUID generated
    }
}

#[cfg(test)]
mod force_validation_tests {
    use super::{validate_override_reason, OverrideReasonValidation};

    #[test]
    fn empty_is_rejected() {
        assert!(matches!(
            validate_override_reason(None),
            OverrideReasonValidation::Empty
        ));
        assert!(matches!(
            validate_override_reason(Some("")),
            OverrideReasonValidation::Empty
        ));
        assert!(matches!(
            validate_override_reason(Some("   \t\n  ")),
            OverrideReasonValidation::Empty
        ));
    }

    #[test]
    fn short_is_rejected_with_length() {
        let r = validate_override_reason(Some("five"));
        let OverrideReasonValidation::TooShort(n) = r else {
            panic!("expected TooShort, got {r:?}")
        };
        assert_eq!(n, 4);
    }

    #[test]
    fn exactly_20_chars_ok() {
        // exactly 20 chars
        let twenty = "abcdefghijklmnopqrst";
        assert_eq!(twenty.chars().count(), 20);
        let OverrideReasonValidation::Ok(s) = validate_override_reason(Some(twenty)) else {
            panic!("expected Ok");
        };
        assert_eq!(s, twenty);
    }

    #[test]
    fn whitespace_trimmed_before_count() {
        let padded = "   API wedged for 20 minutes; reviewed manually in chat   ";
        let OverrideReasonValidation::Ok(s) = validate_override_reason(Some(padded)) else {
            panic!("expected Ok");
        };
        assert!(!s.starts_with(' '));
        assert!(!s.ends_with(' '));
        assert!(s.chars().count() >= 20);
    }

    #[test]
    fn unicode_counted_by_char_not_byte() {
        // 12 emojis = 12 chars but ~48 bytes — must be rejected.
        let emojis = "🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀";
        assert_eq!(emojis.chars().count(), 12);
        let r = validate_override_reason(Some(emojis));
        let OverrideReasonValidation::TooShort(n) = r else {
            panic!("expected TooShort, got {r:?}")
        };
        assert_eq!(n, 12);
    }
}

#[cfg(test)]
mod startup_warnings_tests {
    use super::*;

    fn cfg(enabled: bool, provider: Option<&str>) -> GateConfig {
        GateConfig {
            enabled,
            provider_name: provider.map(|s| s.to_string()),
            min_score: 4,
            timeout: std::time::Duration::from_secs(180),
            backoff_policy: BackoffPolicy::Strict,
            model: None,
        }
    }

    #[test]
    fn no_warning_when_disabled() {
        let w = startup_warnings(&cfg(false, None));
        assert!(w.is_empty(), "got: {:?}", w);
    }

    #[test]
    fn no_warning_when_enabled_with_key() {
        let w = startup_warnings(&cfg(true, Some("anthropic")));
        assert!(w.is_empty());
    }

    #[test]
    fn warning_when_misconfigured() {
        let w = startup_warnings(&cfg(true, None));
        assert_eq!(w.len(), 1);
        let msg = &w[0];
        assert!(msg.starts_with("[dk-mcp] WARNING:"));
        assert!(msg.contains("DKOD_CODE_REVIEW=1"));
        assert!(msg.contains("DKOD_ANTHROPIC_API_KEY"));
        assert!(msg.contains("DKOD_OPENROUTER_API_KEY"));
        assert!(msg.contains("gate_misconfigured"));
    }
}

#[cfg(test)]
mod review_snapshot_tests {
    use super::*;
    use crate::{ReviewFindingProto, ReviewResultProto};

    fn cfg_with_provider() -> GateConfig {
        GateConfig {
            enabled: true,
            provider_name: Some("anthropic".into()),
            min_score: 4,
            timeout: std::time::Duration::from_secs(180),
            backoff_policy: BackoffPolicy::Strict,
            model: Some("claude-sonnet-4-6".into()),
        }
    }

    #[test]
    fn snapshot_from_deep_review() {
        let r = ReviewResultProto {
            id: "r".into(),
            tier: "deep".into(),
            score: Some(2),
            summary: None,
            findings: vec![ReviewFindingProto::default(); 3],
            created_at: "".into(),
        };
        let s = build_review_snapshot(Some(&r), &cfg_with_provider());
        assert_eq!(s.score, Some(2));
        assert_eq!(s.threshold, Some(4));
        assert_eq!(s.findings_count, 3);
        assert_eq!(s.provider, "anthropic");
        assert_eq!(s.model, "claude-sonnet-4-6");
    }

    #[test]
    fn snapshot_from_none() {
        let s = build_review_snapshot(None, &cfg_with_provider());
        assert_eq!(s.score, None);
        assert_eq!(s.threshold, Some(4));
        assert_eq!(s.findings_count, 0);
    }
}

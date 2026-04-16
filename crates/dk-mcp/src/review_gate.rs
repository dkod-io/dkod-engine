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
        confidence: 0.0,
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
        None => return, // degraded policy — fall back silently
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

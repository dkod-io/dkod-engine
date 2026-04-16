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

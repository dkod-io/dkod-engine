//! HTTP client for pytorch/test-infra services.
//!
//! We intentionally do not pull in pytorch/test-infra as a submodule.
//! The test-determination / flaky-test logic is exposed over HTTP on
//! the PyTorch HUD (`https://hud.pytorch.org/`) — calling that over
//! HTTPS keeps the coupling weak and lets us evolve independently.
//!
//! All methods on [`PytorchClient`] are best-effort: on network errors
//! they return `Err` which the caller is expected to treat as a soft
//! "unknown" rather than a hard failure.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Default HUD base URL. Mirrors what `test-infra`'s own CLI uses.
pub const DEFAULT_HUD_URL: &str = "https://hud.pytorch.org";

/// Configuration consumed by [`PytorchClient`]. Built from env vars by
/// [`PytorchClientConfig::from_env`] but also constructible directly for
/// tests.
#[derive(Clone, Debug)]
pub struct PytorchClientConfig {
    pub base_url: String,
    pub token: Option<String>,
    pub request_timeout: Duration,
}

impl PytorchClientConfig {
    /// Build from environment. Returns `None` when neither the explicit
    /// URL nor the `DKOD_PYTORCH_CI=1` force-enable is set.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("PYTORCH_TEST_INFRA_URL").ok();
        let force = std::env::var("DKOD_PYTORCH_CI")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        if url.is_none() && !force {
            return None;
        }
        Some(Self {
            base_url: url.unwrap_or_else(|| DEFAULT_HUD_URL.to_string()),
            token: std::env::var("PYTORCH_CI_TOKEN").ok(),
            request_timeout: Duration::from_secs(30),
        })
    }
}

pub struct PytorchClient {
    http: reqwest::Client,
    cfg: PytorchClientConfig,
}

impl PytorchClient {
    pub fn new(cfg: PytorchClientConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(cfg.request_timeout)
            .user_agent("dkod-engine/pytorch-ci")
            .build()
            .context("building reqwest client for pytorch-ci")?;
        Ok(Self { http, cfg })
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.cfg.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    /// Fetch the current list of known-flaky test identifiers from the
    /// HUD. On any failure (network, schema drift) an empty list is
    /// returned so the caller can proceed without flake filtering.
    pub async fn fetch_flaky_tests(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/flaky_tests", self.cfg.base_url);
        let resp = self
            .auth(self.http.get(&url))
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            anyhow::bail!("GET {url} -> {}", resp.status());
        }
        let body: FlakyTestsResponse = resp
            .json()
            .await
            .with_context(|| format!("decoding JSON from {url}"))?;
        Ok(body.tests.into_iter().map(|t| t.name).collect())
    }

    /// Query the HUD for the set of workflow runs associated with a
    /// commit SHA. Used by the analytics `pytorch_bridge` to correlate
    /// dkod changesets with pytorch CI activity.
    pub async fn workflow_runs_for_sha(&self, sha: &str) -> Result<Vec<WorkflowRun>> {
        let url = format!("{}/api/hud/{}/commit_status", self.cfg.base_url, sha);
        let resp = self
            .auth(self.http.get(&url))
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            anyhow::bail!("GET {url} -> {}", resp.status());
        }
        let body: WorkflowRunsResponse = resp
            .json()
            .await
            .with_context(|| format!("decoding JSON from {url}"))?;
        Ok(body.runs)
    }
}

#[derive(Debug, Deserialize)]
struct FlakyTestsResponse {
    #[serde(default)]
    tests: Vec<FlakyTest>,
}

#[derive(Debug, Deserialize)]
struct FlakyTest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct WorkflowRunsResponse {
    #[serde(default)]
    runs: Vec<WorkflowRun>,
}

/// One row from the HUD commit-status response. Subset of the full
/// schema — we only deserialise the fields we actually use downstream.
#[derive(Clone, Debug, Deserialize)]
pub struct WorkflowRun {
    pub name: String,
    pub conclusion: Option<String>,
    pub duration_ms: Option<u64>,
    pub html_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_env_returns_none_when_nothing_set() {
        std::env::remove_var("PYTORCH_TEST_INFRA_URL");
        std::env::remove_var("DKOD_PYTORCH_CI");
        assert!(PytorchClientConfig::from_env().is_none());
    }

    #[test]
    fn config_from_env_uses_default_url_when_forced() {
        std::env::remove_var("PYTORCH_TEST_INFRA_URL");
        std::env::set_var("DKOD_PYTORCH_CI", "1");
        let cfg = PytorchClientConfig::from_env().unwrap();
        assert_eq!(cfg.base_url, DEFAULT_HUD_URL);
        std::env::remove_var("DKOD_PYTORCH_CI");
    }
}

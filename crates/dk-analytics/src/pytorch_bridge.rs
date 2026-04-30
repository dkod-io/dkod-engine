//! Bridge pytorch/test-infra CI signals into the dkod ClickHouse schema.
//!
//! pytorch/test-infra exposes several HTTP endpoints (test history, flaky
//! test DB, HUD runs) that we normalise into the [`VerificationRun`]
//! columns. Correlation with a dkod `changeset_id` is by commit SHA: the
//! runner embeds the dkod changeset id as a git note / annotated tag that
//! downstream dashboards can join against.
//!
//! This module is intentionally small — it provides the polling glue and
//! the normalisation function. The actual scheduling (how often to poll,
//! which workflows to watch) is the operator's problem; a common pattern is
//! to run [`poll_once`] from a `dk analytics pytorch-poll` cron.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::events::VerificationRun;

/// Base URL for pytorch/test-infra HUD. Override via `PYTORCH_TEST_INFRA_URL`.
pub const DEFAULT_BASE_URL: &str = "https://hud.pytorch.org/api";

#[derive(Clone, Debug)]
pub struct PytorchBridgeConfig {
    pub base_url: String,
    pub token: Option<String>,
    /// Repository slug to query, e.g. "pytorch/pytorch".
    pub repo: String,
}

impl PytorchBridgeConfig {
    pub fn from_env() -> Option<Self> {
        let repo = std::env::var("PYTORCH_REPO").unwrap_or_else(|_| "pytorch/pytorch".to_string());
        let base_url = std::env::var("PYTORCH_TEST_INFRA_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let token = std::env::var("PYTORCH_CI_TOKEN").ok();
        if std::env::var("PYTORCH_TEST_INFRA_URL").is_err() && token.is_none() {
            // Neither the URL nor a token was explicitly configured — polling
            // pytorch HUD anonymously still works, but the caller probably
            // didn't mean to enable this. Return None so the bridge stays
            // disabled by default.
            return None;
        }
        Some(Self {
            base_url,
            token,
            repo,
        })
    }
}

/// Workflow run returned by the HUD. We only keep the fields we map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PytorchWorkflowRun {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub head_sha: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Map of commit SHA → dkod changeset id. Callers seed this from
/// ClickHouse/Postgres before invoking [`poll_once`] so pytorch rows get
/// the right `changeset_id`.
pub type ShaToChangeset = HashMap<String, Uuid>;

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("pytorch API returned HTTP {0}: {1}")]
    Http(u16, String),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Poll the HUD once and return normalised [`VerificationRun`] rows.
///
/// Runs whose `head_sha` is not in `sha_map` are skipped: the ClickHouse
/// schema requires a non-null `changeset_id`.
pub async fn poll_once(
    client: &reqwest::Client,
    cfg: &PytorchBridgeConfig,
    sha_map: &ShaToChangeset,
) -> Result<Vec<VerificationRun>, BridgeError> {
    let url = format!(
        "{base}/runs/{repo}",
        base = cfg.base_url.trim_end_matches('/'),
        repo = cfg.repo
    );
    let mut req = client.get(&url);
    if let Some(token) = &cfg.token {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(BridgeError::Http(status, body));
    }
    let runs: Vec<PytorchWorkflowRun> = resp
        .json()
        .await
        .context("parsing pytorch HUD response")
        .map_err(BridgeError::Other)?;
    Ok(runs
        .into_iter()
        .filter_map(|run| normalise_run(&run, sha_map))
        .collect())
}

/// Normalise a single workflow run into a [`VerificationRun`] row.
///
/// Exposed for unit tests so recorded fixtures can verify the mapping.
pub fn normalise_run(
    run: &PytorchWorkflowRun,
    sha_map: &ShaToChangeset,
) -> Option<VerificationRun> {
    let changeset_id = sha_map.get(&run.head_sha).copied()?;
    let duration_ms = match (run.started_at, run.completed_at) {
        (Some(s), Some(e)) => (e - s).num_milliseconds().max(0) as u64,
        _ => 0,
    };
    let status = match run.conclusion.as_deref() {
        Some("success") => "pass",
        Some("failure") => "fail",
        Some("skipped") => "skip",
        Some(other) => other,
        None => &run.status,
    }
    .to_string();
    Some(VerificationRun {
        run_id: Uuid::new_v4(),
        changeset_id,
        step_name: format!("pytorch-ci:{}", run.name),
        status,
        duration_ms,
        stdout: String::new(),
        findings_count: 0,
        created_at: run.completed_at.unwrap_or_else(Utc::now),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_drops_runs_with_unknown_sha() {
        let run = PytorchWorkflowRun {
            id: 1,
            name: "lint".into(),
            status: "completed".into(),
            conclusion: Some("success".into()),
            head_sha: "deadbeef".into(),
            started_at: Some(Utc::now()),
            completed_at: Some(Utc::now()),
        };
        assert!(normalise_run(&run, &ShaToChangeset::new()).is_none());
    }

    #[test]
    fn normalise_maps_conclusion_to_status() {
        let cs = Uuid::new_v4();
        let mut sha_map = ShaToChangeset::new();
        sha_map.insert("cafef00d".into(), cs);
        let start = Utc::now();
        let end = start + chrono::Duration::milliseconds(750);
        let run = PytorchWorkflowRun {
            id: 2,
            name: "pytest".into(),
            status: "completed".into(),
            conclusion: Some("failure".into()),
            head_sha: "cafef00d".into(),
            started_at: Some(start),
            completed_at: Some(end),
        };
        let out = normalise_run(&run, &sha_map).expect("should map");
        assert_eq!(out.changeset_id, cs);
        assert_eq!(out.status, "fail");
        assert_eq!(out.step_name, "pytorch-ci:pytest");
        assert_eq!(out.duration_ms, 750);
    }

    #[test]
    fn normalise_falls_back_to_status_when_no_conclusion() {
        let cs = Uuid::new_v4();
        let mut sha_map = ShaToChangeset::new();
        sha_map.insert("abc123".into(), cs);
        let run = PytorchWorkflowRun {
            id: 3,
            name: "build".into(),
            status: "in_progress".into(),
            conclusion: None,
            head_sha: "abc123".into(),
            started_at: None,
            completed_at: None,
        };
        let out = normalise_run(&run, &sha_map).unwrap();
        assert_eq!(out.status, "in_progress");
        assert_eq!(out.duration_ms, 0);
    }
}

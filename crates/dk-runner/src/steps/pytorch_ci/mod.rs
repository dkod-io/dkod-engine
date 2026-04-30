//! PyTorch test-infra verification step.
//!
//! A verification step that consults `pytorch/test-infra` services to
//! (a) figure out which test shards are relevant to the files/symbols
//! touched by a changeset, (b) filter out known-flaky tests, and
//! (c) optionally kick off or poll test runs. The step is intentionally
//! **read-mostly** — it does not execute tests locally; the scheduler
//! already does that via `command`. `pytorch_ci`'s job is to surface
//! signal about *which* tests *should* run and what pytorch's CI has
//! already said about similar commits.
//!
//! Activation:
//! - YAML: `type: pytorch-ci` in a step config.
//! - Env: `DKOD_PYTORCH_CI=1` forces the step to run on every
//!   changeset, useful for ad-hoc experimentation.
//!
//! Env configuration:
//! - `PYTORCH_TEST_INFRA_URL` — base URL, defaults to the public HUD.
//! - `PYTORCH_CI_TOKEN` — optional bearer token for authenticated
//!   endpoints (flaky-test DB, test determination).
//!
//! The step is best-effort: if the remote API is unreachable we return
//! a soft "Skip" with a reason in stdout rather than failing the whole
//! pipeline. This matches how we treat optional analytics emission
//! elsewhere.

pub mod client;
pub mod sharding;

use std::time::Instant;

use crate::executor::{StepOutput, StepStatus};
use crate::findings::{Finding, Severity};

/// Run the pytorch CI step against a list of changed files and symbols.
///
/// Returns a [`StepOutput`] plus a list of [`Finding`]s that describe
/// per-shard results (flaky hits, skipped shards).
pub async fn run_pytorch_ci_step(
    changed_files: &[String],
    changed_symbols: &[String],
) -> (StepOutput, Vec<Finding>) {
    let start = Instant::now();

    let cfg = match client::PytorchClientConfig::from_env() {
        Some(c) => c,
        None => {
            return (
                StepOutput {
                    status: StepStatus::Skip,
                    stdout: "pytorch-ci: PYTORCH_TEST_INFRA_URL not set — skipping".into(),
                    stderr: String::new(),
                    duration: start.elapsed(),
                },
                vec![],
            );
        }
    };

    let cli = match client::PytorchClient::new(cfg) {
        Ok(c) => c,
        Err(e) => {
            return (
                StepOutput {
                    status: StepStatus::Skip,
                    stdout: format!("pytorch-ci: client init failed: {e}"),
                    stderr: String::new(),
                    duration: start.elapsed(),
                },
                vec![],
            );
        }
    };

    let shards = sharding::determine_shards(changed_files, changed_symbols);

    let flaky = match cli.fetch_flaky_tests().await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "pytorch-ci: flaky-test fetch failed");
            vec![]
        }
    };

    let relevant_shards: Vec<&sharding::Shard> = shards
        .iter()
        .filter(|s| !s.is_fully_flaky(&flaky))
        .collect();

    let mut findings = Vec::new();
    for s in &shards {
        if s.is_fully_flaky(&flaky) {
            findings.push(Finding {
                severity: Severity::Warning,
                check_name: format!("pytorch-ci:{}", s.name),
                message: format!(
                    "shard skipped: all tests in {} are on the flaky list",
                    s.name
                ),
                file_path: None,
                line: None,
                symbol: None,
            });
        }
    }

    let stdout = serde_json::to_string_pretty(&serde_json::json!({
        "shards_total": shards.len(),
        "shards_relevant": relevant_shards.len(),
        "flaky_count": flaky.len(),
        "shards": shards,
    }))
    .unwrap_or_default();

    (
        StepOutput {
            status: StepStatus::Pass,
            stdout,
            stderr: String::new(),
            duration: start.elapsed(),
        },
        findings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn step_skips_when_env_missing() {
        // We assume a clean environment — the only reliable way to test
        // a feature-flag gate without a heavy mock is to unset the env
        // var for the duration of the test. std::env::remove_var is
        // process-global; this is a read-only test so it's fine in
        // practice.
        std::env::remove_var("PYTORCH_TEST_INFRA_URL");
        let (out, findings) = run_pytorch_ci_step(&[], &[]).await;
        assert_eq!(out.status, StepStatus::Skip);
        assert!(out.stdout.contains("skipping"));
        assert!(findings.is_empty());
    }
}

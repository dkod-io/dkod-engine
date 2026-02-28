pub mod types;

use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;
use tracing::info;

use dk_engine::repo::Engine;
use crate::executor::{StepOutput, StepStatus};
use crate::findings::{Finding, Severity};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30 * 60); // 30 minutes

/// Run human approval gate with DB polling.
pub async fn run_human_approve_step_with_engine(
    engine: &Arc<Engine>,
    changeset_id: Uuid,
    timeout: Option<Duration>,
) -> (StepOutput, Vec<Finding>) {
    let start = Instant::now();
    let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);

    // Set changeset to awaiting_approval using optimistic locking:
    // only transition if the changeset is currently in 'open' state.
    if let Err(e) = engine.changeset_store().update_status_if(changeset_id, "awaiting_approval", &["open"]).await {
        return (
            StepOutput {
                status: StepStatus::Fail,
                stdout: format!("Failed to set awaiting_approval: {e}"),
                stderr: String::new(),
                duration: start.elapsed(),
            },
            Vec::new(),
        );
    }

    info!(changeset_id = %changeset_id, "awaiting human approval (timeout: {:?})", timeout);

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        if start.elapsed() > timeout {
            let finding = Finding {
                severity: Severity::Warning,
                check_name: "human-approval-timeout".to_string(),
                message: format!("Human approval timed out after {:?}", timeout),
                file_path: None, line: None, symbol: None,
            };
            return (
                StepOutput {
                    status: StepStatus::Fail,
                    stdout: "Human approval: timed out".to_string(),
                    stderr: String::new(),
                    duration: start.elapsed(),
                },
                vec![finding],
            );
        }

        match engine.changeset_store().get(changeset_id).await {
            Ok(changeset) => match changeset.state.as_str() {
                "approved" => return (
                    StepOutput { status: StepStatus::Pass, stdout: "Human approval: approved".to_string(), stderr: String::new(), duration: start.elapsed() },
                    Vec::new(),
                ),
                "rejected" => {
                    let finding = Finding {
                        severity: Severity::Error,
                        check_name: "human-approval-rejected".to_string(),
                        message: "Human reviewer rejected the changeset".to_string(),
                        file_path: None, line: None, symbol: None,
                    };
                    return (
                        StepOutput { status: StepStatus::Fail, stdout: "Human approval: rejected".to_string(), stderr: String::new(), duration: start.elapsed() },
                        vec![finding],
                    );
                }
                "awaiting_approval" => continue,
                other => return (
                    StepOutput { status: StepStatus::Skip, stdout: format!("Human approval: changeset moved to unexpected state '{other}'"), stderr: String::new(), duration: start.elapsed() },
                    Vec::new(),
                ),
            },
            Err(e) => return (
                StepOutput { status: StepStatus::Fail, stdout: format!("Human approval: DB error — {e}"), stderr: String::new(), duration: start.elapsed() },
                Vec::new(),
            ),
        }
    }
}

/// Legacy stub for when no engine is available.
pub async fn run_human_approve_step() -> StepOutput {
    let start = Instant::now();
    StepOutput {
        status: StepStatus::Pass,
        stdout: "human approval: auto-approved (not yet implemented)".to_string(),
        stderr: String::new(),
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_legacy_stub_auto_approves() {
        let output = run_human_approve_step().await;
        assert_eq!(output.status, StepStatus::Pass);
        assert!(output.stdout.contains("auto-approved"));
    }
}

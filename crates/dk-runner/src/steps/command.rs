use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::executor::{Executor, StepOutput};
use crate::workflow::validator::validate_command;

/// Run a command step, validating it first against the allowlist.
pub async fn run_command_step(
    executor: &dyn Executor,
    command: &str,
    work_dir: &Path,
    timeout: Duration,
    env: &HashMap<String, String>,
) -> Result<StepOutput> {
    validate_command(command)?;
    Ok(executor.run_command(command, work_dir, timeout, env).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::process::ProcessExecutor;
    use crate::executor::StepStatus;

    #[tokio::test]
    async fn test_run_allowed_command() {
        let exec = ProcessExecutor::new();
        let dir = std::env::temp_dir();
        let out = run_command_step(&exec, "echo hello", &dir, Duration::from_secs(5), &HashMap::new())
            .await
            .unwrap();
        assert_eq!(out.status, StepStatus::Pass);
    }

    #[tokio::test]
    async fn test_run_disallowed_command_errors() {
        let exec = ProcessExecutor::new();
        let dir = std::env::temp_dir();
        let result = run_command_step(&exec, "rm -rf /", &dir, Duration::from_secs(5), &HashMap::new()).await;
        assert!(result.is_err());
    }
}

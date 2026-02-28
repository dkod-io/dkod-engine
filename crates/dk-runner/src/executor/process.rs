use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use super::{Executor, StepOutput, StepStatus};

pub struct ProcessExecutor;

impl ProcessExecutor {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self { Self }
}

const SAFE_ENV_VARS: &[&str] = &["PATH", "HOME", "LANG", "TERM", "USER", "SHELL"];

#[async_trait::async_trait]
impl Executor for ProcessExecutor {
    async fn run_command(
        &self,
        command: &str,
        work_dir: &Path,
        timeout: Duration,
        env: &HashMap<String, String>,
    ) -> StepOutput {
        let start = Instant::now();
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd.current_dir(work_dir);
        cmd.env_clear();
        for var in SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let result = tokio::time::timeout(timeout, cmd.output()).await;
        let duration = start.elapsed();
        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let status = if output.status.success() { StepStatus::Pass } else { StepStatus::Fail };
                StepOutput { status, stdout, stderr, duration }
            }
            Ok(Err(e)) => StepOutput {
                status: StepStatus::Fail, stdout: String::new(),
                stderr: format!("command error: {e}"), duration,
            },
            Err(_) => StepOutput {
                status: StepStatus::Timeout, stdout: String::new(),
                stderr: format!("command timed out after {}s", timeout.as_secs()), duration,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_passes() {
        let exec = ProcessExecutor::new();
        let dir = std::env::temp_dir();
        let out = exec.run_command("echo hello", &dir, Duration::from_secs(5), &HashMap::new()).await;
        assert_eq!(out.status, StepStatus::Pass);
        assert!(out.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_false_fails() {
        let exec = ProcessExecutor::new();
        let dir = std::env::temp_dir();
        let out = exec.run_command("false", &dir, Duration::from_secs(5), &HashMap::new()).await;
        assert_eq!(out.status, StepStatus::Fail);
    }

    #[tokio::test]
    async fn test_timeout() {
        let exec = ProcessExecutor::new();
        let dir = std::env::temp_dir();
        let out = exec.run_command("sleep 10", &dir, Duration::from_millis(100), &HashMap::new()).await;
        assert_eq!(out.status, StepStatus::Timeout);
    }

    #[tokio::test]
    async fn test_env_injection() {
        let exec = ProcessExecutor::new();
        let dir = std::env::temp_dir();
        let mut env = HashMap::new();
        env.insert("DEKODE_TEST".to_string(), "yes".to_string());
        let out = exec.run_command("echo $DEKODE_TEST", &dir, Duration::from_secs(5), &env).await;
        assert_eq!(out.status, StepStatus::Pass);
        assert!(out.stdout.contains("yes"));
    }
}

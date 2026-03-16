pub mod process;
pub mod container;

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Pass,
    Fail,
    Skip,
    Timeout,
}

impl StepStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StepOutput {
    pub status: StepStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    async fn run_command(
        &self,
        command: &str,
        work_dir: &Path,
        timeout: Duration,
        env: &HashMap<String, String>,
    ) -> StepOutput;
}

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use super::{Executor, StepOutput, StepStatus};

pub struct ContainerExecutor;

impl ContainerExecutor {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl Executor for ContainerExecutor {
    async fn run_command(
        &self, _command: &str, _work_dir: &Path, _timeout: Duration, _env: &HashMap<String, String>,
    ) -> StepOutput {
        StepOutput {
            status: StepStatus::Skip, stdout: String::new(),
            stderr: "container executor not yet implemented".to_string(),
            duration: Duration::ZERO,
        }
    }
}

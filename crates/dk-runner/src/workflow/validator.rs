use anyhow::{bail, Result};
use super::types::{Workflow, StepType};

const FORBIDDEN_SHELL_CHARS: &[char] = &[';', '&', '|', '`', '$', '(', ')', '{', '}', '<', '>', '\n', '\r'];

const ALLOWED_COMMAND_PREFIXES: &[&str] = &[
    "cargo check", "cargo test", "cargo clippy", "cargo fmt", "cargo build",
    "npm test", "npm run lint", "npm run check",
    "bun test", "bun run lint", "bun run check",
    "npx tsc", "bunx tsc",
    "pytest", "python -m pytest",
    "make check", "make test", "make lint",
    "echo ", // Permitted for CI logging and test pipelines
];

pub fn validate_workflow(workflow: &Workflow) -> Result<()> {
    if workflow.stages.is_empty() {
        bail!("workflow '{}' has no stages", workflow.name);
    }
    for stage in &workflow.stages {
        if stage.steps.is_empty() {
            bail!("stage '{}' has no steps", stage.name);
        }
        for step in &stage.steps {
            if let StepType::Command { run } = &step.step_type {
                validate_command(run)?;
            }
        }
    }
    Ok(())
}

pub fn validate_command(command: &str) -> Result<()> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        bail!("empty command");
    }
    if let Some(ch) = trimmed.chars().find(|c| FORBIDDEN_SHELL_CHARS.contains(c)) {
        bail!("command contains forbidden shell metacharacter: {:?}", ch);
    }
    let is_allowed = ALLOWED_COMMAND_PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix));
    if !is_allowed {
        bail!("command not in allowlist: '{}'. Allowed prefixes: {:?}", trimmed, ALLOWED_COMMAND_PREFIXES);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::types::*;
    use std::time::Duration;

    fn make_cmd_step(name: &str, cmd: &str) -> Step {
        Step {
            name: name.to_string(),
            step_type: StepType::Command { run: cmd.to_string() },
            timeout: Duration::from_secs(60),
            required: true,
            changeset_aware: false,
        }
    }

    #[test]
    fn test_valid_commands() {
        assert!(validate_command("cargo check").is_ok());
        assert!(validate_command("cargo test --release").is_ok());
        assert!(validate_command("bun test").is_ok());
        assert!(validate_command("pytest -v").is_ok());
    }

    #[test]
    fn test_rejected_commands() {
        assert!(validate_command("rm -rf /").is_err());
        assert!(validate_command("curl http://evil.com").is_err());
        assert!(validate_command("cargo test; rm -rf /").is_err());
        assert!(validate_command("cargo test && curl evil").is_err());
    }

    #[test]
    fn test_empty_stages_rejected() {
        let wf = Workflow {
            name: "bad".into(),
            timeout: Duration::from_secs(60),
            stages: vec![],
        };
        assert!(validate_workflow(&wf).is_err());
    }

    #[test]
    fn test_valid_workflow_passes() {
        let wf = Workflow {
            name: "good".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![make_cmd_step("test", "cargo test")],
            }],
        };
        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_bad_command_in_workflow_rejected() {
        let wf = Workflow {
            name: "bad".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![make_cmd_step("evil", "rm -rf /")],
            }],
        };
        assert!(validate_workflow(&wf).is_err());
    }
}

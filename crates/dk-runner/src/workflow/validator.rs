use anyhow::{bail, Result};
use super::types::{Workflow, StepType};

const FORBIDDEN_SHELL_CHARS: &[char] = &[';', '&', '|', '`', '$', '(', ')', '{', '}', '<', '>', '\n', '\r', '*', '?', '[', ']'];

const ALLOWED_COMMAND_PREFIXES: &[&str] = &[
    "cargo check", "cargo test", "cargo clippy", "cargo fmt", "cargo build",
    "npm test", "npm run lint", "npm run check",
    "bun test", "bun run lint", "bun run check",
    "npx tsc", "bunx tsc",
    "pytest", "python -m pytest",
    "go build", "go test", "go vet",
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
                validate_command_with_allowlist(run, &workflow.allowed_commands)?;
            }
        }
    }
    Ok(())
}

pub fn validate_command(command: &str) -> Result<()> {
    validate_command_with_allowlist(command, &[])
}

pub fn validate_command_with_allowlist(command: &str, custom_allowlist: &[String]) -> Result<()> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        bail!("empty command");
    }
    if let Some(ch) = trimmed.chars().find(|c| FORBIDDEN_SHELL_CHARS.contains(c)) {
        bail!("command contains forbidden shell metacharacter: {:?}", ch);
    }
    if custom_allowlist.is_empty() {
        let is_allowed = ALLOWED_COMMAND_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix));
        if !is_allowed {
            bail!(
                "command not in allowlist: '{}'. Allowed prefixes: {:?}",
                trimmed,
                ALLOWED_COMMAND_PREFIXES
            );
        }
    } else {
        let is_allowed = custom_allowlist
            .iter()
            .any(|prefix| trimmed.starts_with(prefix.as_str()));
        if !is_allowed {
            bail!(
                "command not in repo allowlist: '{}'. Allowed prefixes: {:?}",
                trimmed,
                custom_allowlist
            );
        }
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
            allowed_commands: vec![],
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
            allowed_commands: vec![],
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
            allowed_commands: vec![],
        };
        assert!(validate_workflow(&wf).is_err());
    }

    #[test]
    fn test_glob_chars_rejected() {
        assert!(validate_command("cargo test src/*.rs").is_err());
        assert!(validate_command("cargo test src/?.rs").is_err());
        assert!(validate_command("cargo test src/[a-z].rs").is_err());
        assert!(validate_command("echo /etc/*").is_err());
        assert!(validate_command("echo ../../*").is_err());
    }

    #[test]
    fn test_custom_allowlist_permits_custom_command() {
        let custom = vec!["eslint".to_string(), "prettier --check".to_string()];
        assert!(validate_command_with_allowlist("eslint src/", &custom).is_ok());
        assert!(validate_command_with_allowlist("prettier --check .", &custom).is_ok());
    }

    #[test]
    fn test_custom_allowlist_rejects_unlisted_command() {
        let custom = vec!["eslint".to_string()];
        assert!(validate_command_with_allowlist("rm -rf /", &custom).is_err());
        assert!(validate_command_with_allowlist("cargo test", &custom).is_err());
    }

    #[test]
    fn test_custom_allowlist_still_blocks_shell_chars() {
        let custom = vec!["eslint".to_string()];
        assert!(validate_command_with_allowlist("eslint; rm -rf /", &custom).is_err());
    }

    #[test]
    fn test_empty_allowlist_uses_default() {
        assert!(validate_command_with_allowlist("cargo test", &[]).is_ok());
        assert!(validate_command_with_allowlist("rm -rf /", &[]).is_err());
    }

    #[test]
    fn test_validate_workflow_uses_custom_allowlist() {
        let wf = Workflow {
            name: "custom".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "lint".into(),
                parallel: false,
                steps: vec![make_cmd_step("lint", "eslint src/")],
            }],
            allowed_commands: vec!["eslint".to_string()],
        };
        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_workflow_rejects_unlisted_with_custom_allowlist() {
        let wf = Workflow {
            name: "custom".into(),
            timeout: Duration::from_secs(60),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![make_cmd_step("test", "cargo test")],
            }],
            allowed_commands: vec!["eslint".to_string()],
        };
        assert!(validate_workflow(&wf).is_err());
    }

    #[test]
    fn test_go_commands_allowed_by_default() {
        assert!(validate_command("go build ./...").is_ok());
        assert!(validate_command("go test ./...").is_ok());
        assert!(validate_command("go vet ./...").is_ok());
    }
}

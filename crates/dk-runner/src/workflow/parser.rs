use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use super::types::*;

pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if let Some(mins) = s.strip_suffix('m') {
        let n: u64 = mins.parse().context("invalid minutes")?;
        return Ok(Duration::from_secs(n * 60));
    }
    if let Some(secs) = s.strip_suffix('s') {
        let n: u64 = secs.parse().context("invalid seconds")?;
        return Ok(Duration::from_secs(n));
    }
    if let Some(hours) = s.strip_suffix('h') {
        let n: u64 = hours.parse().context("invalid hours")?;
        return Ok(Duration::from_secs(n * 3600));
    }
    bail!(
        "unsupported duration format: '{}' (use '5m', '120s', or '2h')",
        s
    )
}

pub fn parse_workflow_file(path: &Path) -> Result<Workflow> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow file: {}", path.display()))?;
    parse_workflow_str(&content)
}

pub fn parse_workflow_str(content: &str) -> Result<Workflow> {
    let file: WorkflowFile = toml::from_str(content).context("failed to parse workflow TOML")?;
    let timeout = parse_duration(&file.pipeline.timeout)?;
    let stages = file
        .stage
        .into_iter()
        .map(resolve_stage)
        .collect::<Result<Vec<_>>>()?;
    Ok(Workflow {
        name: file.pipeline.name,
        timeout,
        stages,
    })
}

fn resolve_stage(sc: StageConfig) -> Result<Stage> {
    let steps = sc
        .step
        .into_iter()
        .map(resolve_step)
        .collect::<Result<Vec<_>>>()?;
    Ok(Stage {
        name: sc.name,
        parallel: sc.parallel,
        steps,
    })
}

fn resolve_step(sc: StepConfig) -> Result<Step> {
    let timeout = match &sc.timeout {
        Some(t) => parse_duration(t)?,
        None => Duration::from_secs(120),
    };
    let step_type = match sc.step_type.as_deref() {
        Some("semantic") => StepType::Semantic { checks: sc.check },
        Some("agent-review") => StepType::AgentReview {
            prompt: sc
                .prompt
                .unwrap_or_else(|| "Review this changeset".to_string()),
        },
        Some("human-approve") => StepType::HumanApprove,
        Some(other) => bail!("unknown step type: '{}'", other),
        None => {
            let run = sc.run.context("step must have either 'run' or 'type'")?;
            StepType::Command { run }
        }
    };
    Ok(Step {
        name: sc.name,
        step_type,
        timeout,
        required: sc.required,
        changeset_aware: sc.changeset_aware,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("120s").unwrap(), Duration::from_secs(120));
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn test_parse_basic_workflow() {
        let toml = r#"
[pipeline]
name = "verify"
timeout = "10m"

[[stage]]
name = "checks"
parallel = true

[[stage.step]]
name = "typecheck"
run = "cargo check"
timeout = "2m"

[[stage.step]]
name = "test"
run = "cargo test"
timeout = "5m"
changeset_aware = true
"#;
        let wf = parse_workflow_str(toml).unwrap();
        assert_eq!(wf.name, "verify");
        assert_eq!(wf.timeout, Duration::from_secs(600));
        assert_eq!(wf.stages.len(), 1);
        assert!(wf.stages[0].parallel);
        assert_eq!(wf.stages[0].steps.len(), 2);
        assert!(wf.stages[0].steps[1].changeset_aware);
    }

    #[test]
    fn test_parse_gates_stage() {
        let toml = r#"
[pipeline]
name = "full"

[[stage]]
name = "gates"

[[stage.step]]
name = "semantic-check"
type = "semantic"
check = ["no-unsafe-added", "types-consistent"]

[[stage.step]]
name = "agent-review"
type = "agent-review"
prompt = "Check for security issues"

[[stage.step]]
name = "human-approval"
type = "human-approve"
"#;
        let wf = parse_workflow_str(toml).unwrap();
        assert_eq!(wf.stages.len(), 1);
        let steps = &wf.stages[0].steps;
        assert_eq!(steps.len(), 3);
        assert!(
            matches!(&steps[0].step_type, StepType::Semantic { checks } if checks.len() == 2)
        );
        assert!(matches!(&steps[1].step_type, StepType::AgentReview { .. }));
        assert!(matches!(&steps[2].step_type, StepType::HumanApprove));
    }

    #[test]
    fn test_step_without_run_or_type_fails() {
        let toml = r#"
[pipeline]
name = "bad"

[[stage]]
name = "s"

[[stage.step]]
name = "missing"
"#;
        assert!(parse_workflow_str(toml).is_err());
    }
}

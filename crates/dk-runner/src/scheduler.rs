use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

use dk_engine::repo::Engine;

use crate::changeset::scope_command_to_changeset;
use crate::executor::{Executor, StepOutput, StepStatus};
use crate::findings::{Finding, Suggestion};
use crate::steps::{agent_review, command, human_approve, semantic};
use crate::workflow::types::{Stage, Step, StepType, Workflow};

/// Result of running a single step, with metadata for streaming.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub stage_name: String,
    pub step_name: String,
    pub status: StepStatus,
    pub output: String,
    pub required: bool,
    pub findings: Vec<Finding>,
    pub suggestions: Vec<Suggestion>,
}

/// Run an entire workflow: stages sequentially, steps within parallel stages concurrently.
/// Sends `StepResult`s to `tx` as each step completes. Returns `true` if all required steps passed.
///
/// `engine` and `repo_id` are optional — when provided, the semantic step uses the full
/// Engine-backed analysis. Pass `None` for both in tests or contexts without an Engine.
pub async fn run_workflow(
    workflow: &Workflow,
    executor: &dyn Executor,
    work_dir: &Path,
    changeset_files: &[String],
    env: &HashMap<String, String>,
    tx: &mpsc::Sender<StepResult>,
    engine: Option<&Arc<Engine>>,
    repo_id: Option<Uuid>,
    changeset_id: Option<Uuid>,
) -> bool {
    let mut all_passed = true;

    for stage in &workflow.stages {
        info!(stage = %stage.name, parallel = stage.parallel, "running stage");

        let results = if stage.parallel {
            run_stage_parallel(stage, executor, work_dir, changeset_files, env, engine, repo_id, changeset_id)
                .await
        } else {
            run_stage_sequential(stage, executor, work_dir, changeset_files, env, engine, repo_id, changeset_id)
                .await
        };

        for result in results {
            if result.status != StepStatus::Pass && result.required {
                all_passed = false;
            }
            let _ = tx.send(result).await;
        }
    }

    all_passed
}

async fn run_stage_parallel(
    stage: &Stage,
    executor: &dyn Executor,
    work_dir: &Path,
    changeset_files: &[String],
    env: &HashMap<String, String>,
    engine: Option<&Arc<Engine>>,
    repo_id: Option<Uuid>,
    changeset_id: Option<Uuid>,
) -> Vec<StepResult> {
    let mut futures = Vec::new();
    for step in &stage.steps {
        futures.push(run_single_step(
            &stage.name,
            step,
            executor,
            work_dir,
            changeset_files,
            env,
            engine,
            repo_id,
            changeset_id,
        ));
    }
    futures::future::join_all(futures).await
}

async fn run_stage_sequential(
    stage: &Stage,
    executor: &dyn Executor,
    work_dir: &Path,
    changeset_files: &[String],
    env: &HashMap<String, String>,
    engine: Option<&Arc<Engine>>,
    repo_id: Option<Uuid>,
    changeset_id: Option<Uuid>,
) -> Vec<StepResult> {
    let mut results = Vec::new();
    for step in &stage.steps {
        let result = run_single_step(
            &stage.name,
            step,
            executor,
            work_dir,
            changeset_files,
            env,
            engine,
            repo_id,
            changeset_id,
        )
        .await;
        results.push(result);
    }
    results
}

async fn run_single_step(
    stage_name: &str,
    step: &Step,
    executor: &dyn Executor,
    work_dir: &Path,
    changeset_files: &[String],
    env: &HashMap<String, String>,
    engine: Option<&Arc<Engine>>,
    repo_id: Option<Uuid>,
    changeset_id: Option<Uuid>,
) -> StepResult {
    info!(step = %step.name, "running step");

    match &step.step_type {
        StepType::Command { run } => {
            let cmd = if step.changeset_aware {
                scope_command_to_changeset(run, changeset_files)
                    .unwrap_or_else(|| run.clone())
            } else {
                run.clone()
            };
            let output =
                match command::run_command_step(executor, &cmd, work_dir, step.timeout, env).await {
                    Ok(out) => out,
                    Err(e) => StepOutput {
                        status: StepStatus::Fail,
                        stdout: String::new(),
                        stderr: e.to_string(),
                        duration: std::time::Duration::ZERO,
                    },
                };

            let combined_output = if output.stderr.is_empty() {
                output.stdout
            } else {
                format!("{}{}", output.stdout, output.stderr)
            };

            StepResult {
                stage_name: stage_name.to_string(),
                step_name: step.name.clone(),
                status: output.status,
                output: combined_output,
                required: step.required,
                findings: Vec::new(),
                suggestions: Vec::new(),
            }
        }
        StepType::Semantic { checks } => {
            if let (Some(eng), Some(rid)) = (engine, repo_id) {
                // Full Engine-backed semantic analysis
                let (output, findings, suggestions) = semantic::run_semantic_step(
                    eng,
                    rid,
                    changeset_files,
                    work_dir,
                    checks,
                )
                .await;

                let combined_output = if output.stderr.is_empty() {
                    output.stdout
                } else {
                    format!("{}{}", output.stdout, output.stderr)
                };

                StepResult {
                    stage_name: stage_name.to_string(),
                    step_name: step.name.clone(),
                    status: output.status,
                    output: combined_output,
                    required: step.required,
                    findings,
                    suggestions,
                }
            } else {
                // Fallback to simple shim (no Engine available)
                let output = semantic::run_semantic_step_simple(checks).await;

                let combined_output = if output.stderr.is_empty() {
                    output.stdout
                } else {
                    format!("{}{}", output.stdout, output.stderr)
                };

                StepResult {
                    stage_name: stage_name.to_string(),
                    step_name: step.name.clone(),
                    status: output.status,
                    output: combined_output,
                    required: step.required,
                    findings: Vec::new(),
                    suggestions: Vec::new(),
                }
            }
        }
        StepType::AgentReview { prompt } => {
            let provider = agent_review::claude::ClaudeReviewProvider::from_env();
            if let Some(provider) = provider {
                let mut diff = String::new();
                let mut files = Vec::new();
                for path in changeset_files {
                    let full_path = work_dir.join(path);
                    if let Ok(content) = tokio::fs::read_to_string(&full_path).await {
                        diff.push_str(&format!("--- {path}\n+++ {path}\n{content}\n"));
                        files.push(agent_review::provider::FileContext {
                            path: path.clone(),
                            content,
                        });
                    }
                }
                let (output, findings, suggestions) =
                    agent_review::run_agent_review_step_with_provider(
                        &provider, &diff, files, prompt,
                    )
                    .await;
                return StepResult {
                    stage_name: stage_name.to_string(),
                    step_name: step.name.clone(),
                    status: output.status,
                    output: if output.stderr.is_empty() {
                        output.stdout
                    } else {
                        format!("{}{}", output.stdout, output.stderr)
                    },
                    required: step.required,
                    findings,
                    suggestions,
                };
            }
            // No provider: use legacy stub
            let output = agent_review::run_agent_review_step(prompt).await;
            StepResult {
                stage_name: stage_name.to_string(),
                step_name: step.name.clone(),
                status: output.status,
                output: if output.stderr.is_empty() {
                    output.stdout
                } else {
                    format!("{}{}", output.stdout, output.stderr)
                },
                required: step.required,
                findings: Vec::new(),
                suggestions: Vec::new(),
            }
        }
        StepType::HumanApprove => {
            if let (Some(eng), Some(cid)) = (engine, changeset_id) {
                let (output, findings) = human_approve::run_human_approve_step_with_engine(
                    eng, cid, Some(step.timeout),
                ).await;
                return StepResult {
                    stage_name: stage_name.to_string(),
                    step_name: step.name.clone(),
                    status: output.status,
                    output: if output.stderr.is_empty() { output.stdout } else { format!("{}{}", output.stdout, output.stderr) },
                    required: step.required,
                    findings,
                    suggestions: Vec::new(),
                };
            }
            let output = human_approve::run_human_approve_step().await;
            StepResult {
                stage_name: stage_name.to_string(),
                step_name: step.name.clone(),
                status: output.status,
                output: if output.stderr.is_empty() { output.stdout } else { format!("{}{}", output.stdout, output.stderr) },
                required: step.required,
                findings: Vec::new(),
                suggestions: Vec::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::process::ProcessExecutor;
    use crate::workflow::types::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_run_workflow_passes() {
        let wf = Workflow {
            name: "test".into(),
            timeout: Duration::from_secs(30),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![Step {
                    name: "echo-test".into(),
                    step_type: StepType::Command {
                        run: "echo hello".into(),
                    },
                    timeout: Duration::from_secs(5),
                    required: true,
                    changeset_aware: false,
                }],
            }],
        };

        let exec = ProcessExecutor::new();
        let (tx, mut rx) = mpsc::channel(32);
        let dir = std::env::temp_dir();

        let passed =
            run_workflow(&wf, &exec, &dir, &[], &HashMap::new(), &tx, None, None, None).await;
        drop(tx);
        assert!(passed);
        let result = rx.recv().await.unwrap();
        assert_eq!(result.status, StepStatus::Pass);
    }

    #[tokio::test]
    async fn test_failing_required_step() {
        let wf = Workflow {
            name: "test".into(),
            timeout: Duration::from_secs(30),
            stages: vec![Stage {
                name: "checks".into(),
                parallel: false,
                steps: vec![Step {
                    name: "disallowed".into(),
                    step_type: StepType::Command {
                        run: "false_cmd_not_in_allowlist".into(),
                    },
                    timeout: Duration::from_secs(5),
                    required: true,
                    changeset_aware: false,
                }],
            }],
        };

        let exec = ProcessExecutor::new();
        let (tx, _rx) = mpsc::channel(32);
        let dir = std::env::temp_dir();

        let passed =
            run_workflow(&wf, &exec, &dir, &[], &HashMap::new(), &tx, None, None, None).await;
        drop(tx);
        assert!(!passed);
    }

    #[tokio::test]
    async fn test_parallel_stage() {
        let wf = Workflow {
            name: "test".into(),
            timeout: Duration::from_secs(30),
            stages: vec![Stage {
                name: "parallel-checks".into(),
                parallel: true,
                steps: vec![
                    Step {
                        name: "echo-a".into(),
                        step_type: StepType::Command {
                            run: "echo a".into(),
                        },
                        timeout: Duration::from_secs(5),
                        required: true,
                        changeset_aware: false,
                    },
                    Step {
                        name: "echo-b".into(),
                        step_type: StepType::Command {
                            run: "echo b".into(),
                        },
                        timeout: Duration::from_secs(5),
                        required: true,
                        changeset_aware: false,
                    },
                ],
            }],
        };

        let exec = ProcessExecutor::new();
        let (tx, mut rx) = mpsc::channel(32);
        let dir = std::env::temp_dir();

        let passed =
            run_workflow(&wf, &exec, &dir, &[], &HashMap::new(), &tx, None, None, None).await;
        drop(tx);
        assert!(passed);

        let mut results = Vec::new();
        while let Some(r) = rx.recv().await {
            results.push(r);
        }
        assert_eq!(results.len(), 2);
    }
}

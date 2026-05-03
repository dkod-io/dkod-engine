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
use crate::steps::{agent_review, command, human_approve, llm_judge, pytorch_ci, semantic};
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
    /// Wall-clock duration of the step in milliseconds. Measured around
    /// `run_single_step` so it includes framework overhead (spawn, await)
    /// as well as the step body itself — this matches what a human would
    /// see in a dashboard.
    pub duration_ms: u64,
}

/// Emit a `verification_runs` row for a finished step.
///
/// A no-op when no analytics sink is installed or when the caller didn't
/// pass a `changeset_id` (the schema requires a non-null `changeset_id`,
/// and rows without one wouldn't roll up to anything useful).
fn emit_verification_analytics(changeset_id: Option<Uuid>, result: &StepResult) {
    let Some(changeset_id) = changeset_id else {
        return;
    };
    dk_analytics::global::emit(dk_analytics::AnalyticsEvent::Verification(
        dk_analytics::VerificationRun {
            run_id: Uuid::new_v4(),
            changeset_id,
            step_name: format!("{}::{}", result.stage_name, result.step_name),
            status: result.status.as_str().to_string(),
            duration_ms: result.duration_ms,
            // `output` already carries stdout+stderr concatenated; the
            // column is named `stdout` in ClickHouse for backwards
            // compatibility, not because of content.
            stdout: result.output.clone(),
            findings_count: result.findings.len() as u32,
            created_at: chrono::Utc::now(),
        },
    ));
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
            run_stage_parallel(
                stage,
                executor,
                work_dir,
                changeset_files,
                env,
                engine,
                repo_id,
                changeset_id,
            )
            .await
        } else {
            run_stage_sequential(
                stage,
                executor,
                work_dir,
                changeset_files,
                env,
                engine,
                repo_id,
                changeset_id,
            )
            .await
        };

        for result in results {
            if result.status != StepStatus::Pass && result.required {
                all_passed = false;
            }
            emit_verification_analytics(changeset_id, &result);
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
        let failed_required = step.required && result.status != StepStatus::Pass;
        results.push(result);
        // Abort early if a required step failed — no point running subsequent
        // steps (e.g., cargo test after cargo check fails with compile errors)
        if failed_required {
            tracing::warn!(
                stage = %stage.name,
                step = %step.name,
                "required step failed — aborting remaining steps in sequential stage"
            );
            break;
        }
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

    let started = std::time::Instant::now();
    let mut result = run_single_step_inner(
        stage_name,
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
    result.duration_ms = started.elapsed().as_millis() as u64;
    result
}

async fn run_single_step_inner(
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
    match &step.step_type {
        StepType::Command { run } => {
            let cmd = if step.changeset_aware {
                let local_files: Vec<String> = if let Some(sub) = &step.work_dir {
                    let prefix = format!("{}/", sub.display());
                    changeset_files
                        .iter()
                        .filter_map(|f| f.strip_prefix(&prefix).map(|s| s.to_string()))
                        .collect()
                } else {
                    changeset_files.to_vec()
                };
                scope_command_to_changeset(run, &local_files).unwrap_or_else(|| run.clone())
            } else {
                run.clone()
            };
            let step_work_dir = match &step.work_dir {
                Some(sub) => work_dir.join(sub),
                None => work_dir.to_path_buf(),
            };
            let output =
                match command::run_command_step(executor, &cmd, &step_work_dir, step.timeout, env)
                    .await
                {
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
                duration_ms: 0,
            }
        }
        StepType::Semantic { checks } => {
            if let (Some(eng), Some(rid)) = (engine, repo_id) {
                // Full Engine-backed semantic analysis
                let (output, findings, suggestions) =
                    semantic::run_semantic_step(eng, rid, changeset_files, work_dir, checks).await;

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
                    duration_ms: 0,
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
                    duration_ms: 0,
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
                    duration_ms: 0,
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
                duration_ms: 0,
            }
        }
        StepType::HumanApprove => {
            if let (Some(eng), Some(cid)) = (engine, changeset_id) {
                let (output, findings) =
                    human_approve::run_human_approve_step_with_engine(eng, cid, Some(step.timeout))
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
                    suggestions: Vec::new(),
                    duration_ms: 0,
                };
            }
            let output = human_approve::run_human_approve_step().await;
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
                duration_ms: 0,
            }
        }
        StepType::LlmJudge {
            criteria,
            max_iterations,
        } => {
            // The judge needs: the diff (built from the changeset files
            // the same way agent_review does), an engine to flip the
            // changeset state, and an LLM provider. If we can't get all
            // three we degrade gracefully — a misconfigured environment
            // shouldn't take down the whole pipeline.
            let provider = llm_judge::AnthropicJudge::from_env();
            match (provider, engine, changeset_id) {
                (Some(provider), Some(eng), Some(cid)) => {
                    let mut diff = String::new();
                    for path in changeset_files {
                        let full_path = work_dir.join(path);
                        if let Ok(content) = tokio::fs::read_to_string(&full_path).await {
                            diff.push_str(&format!("--- {path}\n+++ {path}\n{content}\n"));
                        }
                    }
                    let (output, findings) = llm_judge::run_llm_judge_step_with_engine(
                        &provider,
                        eng,
                        cid,
                        &diff,
                        criteria,
                        *max_iterations,
                    )
                    .await;
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
                        findings,
                        suggestions: Vec::new(),
                        duration_ms: 0,
                    }
                }
                _ => StepResult {
                    stage_name: stage_name.to_string(),
                    step_name: step.name.clone(),
                    status: StepStatus::Skip,
                    output: "llm-judge: no provider / engine / changeset configured — skipping"
                        .to_string(),
                    required: step.required,
                    findings: Vec::new(),
                    suggestions: Vec::new(),
                    duration_ms: 0,
                },
            }
        }
        StepType::PytorchCi => {
            // Symbols are not tracked on the `Step` level — the
            // sharding heuristic can work with just files. When the
            // engine starts exposing changed symbols per step we can
            // thread them through here.
            let files: Vec<String> = changeset_files.iter().map(|s| s.to_string()).collect();
            let (output, findings) = pytorch_ci::run_pytorch_ci_step(&files, &[]).await;
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
                findings,
                suggestions: Vec::new(),
                duration_ms: 0,
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
                    work_dir: None,
                }],
            }],
            allowed_commands: vec![],
        };

        let exec = ProcessExecutor::new();
        let (tx, mut rx) = mpsc::channel(32);
        let dir = std::env::temp_dir();

        let passed = run_workflow(
            &wf,
            &exec,
            &dir,
            &[],
            &HashMap::new(),
            &tx,
            None,
            None,
            None,
        )
        .await;
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
                    work_dir: None,
                }],
            }],
            allowed_commands: vec![],
        };

        let exec = ProcessExecutor::new();
        let (tx, _rx) = mpsc::channel(32);
        let dir = std::env::temp_dir();

        let passed = run_workflow(
            &wf,
            &exec,
            &dir,
            &[],
            &HashMap::new(),
            &tx,
            None,
            None,
            None,
        )
        .await;
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
                        work_dir: None,
                    },
                    Step {
                        name: "echo-b".into(),
                        step_type: StepType::Command {
                            run: "echo b".into(),
                        },
                        timeout: Duration::from_secs(5),
                        required: true,
                        changeset_aware: false,
                        work_dir: None,
                    },
                ],
            }],
            allowed_commands: vec![],
        };

        let exec = ProcessExecutor::new();
        let (tx, mut rx) = mpsc::channel(32);
        let dir = std::env::temp_dir();

        let passed = run_workflow(
            &wf,
            &exec,
            &dir,
            &[],
            &HashMap::new(),
            &tx,
            None,
            None,
            None,
        )
        .await;
        drop(tx);
        assert!(passed);

        let mut results = Vec::new();
        while let Some(r) = rx.recv().await {
            results.push(r);
        }
        assert_eq!(results.len(), 2);
    }
}

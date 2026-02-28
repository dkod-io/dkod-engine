use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

use dk_engine::repo::Engine;

use crate::executor::Executor;
use crate::scheduler::{self, StepResult};
use crate::workflow::parser::parse_workflow_file;
use crate::workflow::types::{Stage, Step, StepType, Workflow};
use crate::workflow::validator::validate_workflow;

/// The top-level runner that loads workflows and executes them.
pub struct Runner {
    engine: Arc<Engine>,
    executor: Box<dyn Executor>,
}

impl Runner {
    pub fn new(engine: Arc<Engine>, executor: Box<dyn Executor>) -> Self {
        Self { engine, executor }
    }

    /// Run a verification pipeline for a changeset.
    pub async fn verify(
        &self,
        changeset_id: Uuid,
        repo_name: &str,
        tx: mpsc::Sender<StepResult>,
    ) -> Result<bool> {
        let (repo_id, repo_dir) = {
            let (repo_id, git_repo) = self.engine.get_repo(repo_name).await?;
            // git_repo.path() returns the .git dir; we want the working tree
            let git_path = git_repo.path().to_path_buf();
            let work_tree = git_path.parent().unwrap_or(&git_path).to_path_buf();
            (repo_id, work_tree)
        };

        // Create a temp directory with the full repo content, then overlay
        // changeset files so that cargo/build tools find Cargo.toml and
        // all workspace metadata alongside the modified source files.
        let changeset_data = self.engine.changeset_store().get_files(changeset_id).await?;
        let temp_dir = tempfile::tempdir().context("failed to create temp dir for verify")?;
        let work_dir = temp_dir.path().to_path_buf();

        // Copy repo working tree into temp dir so Cargo.toml, Cargo.lock,
        // and all other workspace files are present for build tools.
        copy_dir_recursive(&repo_dir, &work_dir).await
            .context("failed to copy repo into temp dir")?;

        // Overlay changeset files on top of the repo copy.
        let mut changeset_paths: Vec<String> = Vec::with_capacity(changeset_data.len());
        for file in &changeset_data {
            changeset_paths.push(file.file_path.clone());
            if let Some(content) = &file.content {
                let dest = work_dir.join(&file.file_path);
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&dest, content).await?;
            }
        }

        info!(
            "copied repo and overlaid {} changeset files into {} for verification",
            changeset_paths.len(),
            work_dir.display()
        );

        let workflow = self.load_workflow(&repo_dir, repo_id).await?;
        validate_workflow(&workflow).context("workflow validation failed")?;

        let mut env = HashMap::new();
        env.insert("DEKODE_CHANGESET_ID".to_string(), changeset_id.to_string());
        env.insert("DEKODE_REPO_ID".to_string(), repo_id.to_string());

        let passed = tokio::time::timeout(
            workflow.timeout,
            scheduler::run_workflow(
                &workflow,
                self.executor.as_ref(),
                &work_dir,
                &changeset_paths,
                &env,
                &tx,
                Some(&self.engine),
                Some(repo_id),
                Some(changeset_id),
            ),
        )
        .await
        .unwrap_or_else(|_| {
            tracing::warn!("workflow '{}' timed out after {:?}", workflow.name, workflow.timeout);
            false
        });

        // temp_dir cleaned up on drop
        Ok(passed)
    }

    async fn load_workflow(&self, work_dir: &Path, repo_id: Uuid) -> Result<Workflow> {
        let pipeline_path = work_dir.join(".dekode/pipeline.toml");
        if pipeline_path.exists() {
            info!("loading workflow from {}", pipeline_path.display());
            return parse_workflow_file(&pipeline_path);
        }

        let db_steps = self.engine
            .pipeline_store()
            .get_pipeline(repo_id)
            .await
            .unwrap_or_default();

        if !db_steps.is_empty() {
            info!(
                "loading workflow from DB pipeline ({} steps)",
                db_steps.len()
            );
            return Ok(db_pipeline_to_workflow(db_steps));
        }

        info!("using default verification workflow");
        Ok(default_workflow())
    }
}

fn db_pipeline_to_workflow(steps: Vec<dk_engine::pipeline::PipelineStep>) -> Workflow {
    let resolved_steps: Vec<Step> = steps
        .into_iter()
        .map(|s| {
            let command = s
                .config
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("echo 'no command configured'")
                .to_string();
            let timeout_secs = s
                .config
                .get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(120);

            let step_type = match s.step_type.as_str() {
                "agent-review" => StepType::AgentReview {
                    prompt: "Review this changeset".to_string(),
                },
                "human-approve" => StepType::HumanApprove,
                _ => StepType::Command { run: command },
            };

            Step {
                name: s.step_type.clone(),
                step_type,
                timeout: Duration::from_secs(timeout_secs),
                required: s.required,
                changeset_aware: false,
            }
        })
        .collect();

    Workflow {
        name: "db-pipeline".to_string(),
        timeout: Duration::from_secs(600),
        stages: vec![Stage {
            name: "pipeline".to_string(),
            parallel: false,
            steps: resolved_steps,
        }],
    }
}

fn default_workflow() -> Workflow {
    Workflow {
        name: "default".to_string(),
        timeout: Duration::from_secs(120),
        stages: vec![Stage {
            name: "checks".to_string(),
            parallel: false,
            steps: vec![
                Step {
                    name: "typecheck".to_string(),
                    step_type: StepType::Command {
                        run: "cargo check".to_string(),
                    },
                    timeout: Duration::from_secs(60),
                    required: true,
                    changeset_aware: true,
                },
                Step {
                    name: "test".to_string(),
                    step_type: StepType::Command {
                        run: "cargo test".to_string(),
                    },
                    timeout: Duration::from_secs(60),
                    required: true,
                    changeset_aware: true,
                },
            ],
        }],
    }
}

/// Recursively copy a directory tree, skipping the `.git` directory.
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name();
        // Skip .git to avoid copying potentially large git objects
        if file_name == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&file_name);
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_workflow_structure() {
        let wf = default_workflow();
        assert_eq!(wf.name, "default");
        assert_eq!(wf.stages.len(), 1);
        assert!(!wf.stages[0].parallel);
        assert_eq!(wf.stages[0].steps.len(), 2);
    }

    #[test]
    fn test_db_pipeline_conversion() {
        let steps = vec![
            dk_engine::pipeline::PipelineStep {
                repo_id: Uuid::new_v4(),
                step_order: 1,
                step_type: "typecheck".to_string(),
                config: serde_json::json!({"command": "cargo check", "timeout_secs": 120}),
                required: true,
            },
            dk_engine::pipeline::PipelineStep {
                repo_id: Uuid::new_v4(),
                step_order: 2,
                step_type: "test".to_string(),
                config: serde_json::json!({"command": "cargo test", "timeout_secs": 300}),
                required: true,
            },
        ];
        let wf = db_pipeline_to_workflow(steps);
        assert_eq!(wf.stages.len(), 1);
        assert_eq!(wf.stages[0].steps.len(), 2);
    }
}

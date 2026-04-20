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
use crate::workflow::parser::parse_yaml_workflow_file;
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
            // GitRepository::path() already returns the working tree directory
            (repo_id, git_repo.path().to_path_buf())
        };

        // Create a temp directory with the full repo content, then overlay
        // changeset files so that cargo/build tools find Cargo.toml and
        // all workspace metadata alongside the modified source files.
        let changeset_data = self
            .engine
            .changeset_store()
            .get_files(changeset_id)
            .await?;
        let temp_dir = tempfile::tempdir().context("failed to create temp dir for verify")?;
        let work_dir = temp_dir.path().to_path_buf();

        // Copy repo working tree into temp dir so Cargo.toml, Cargo.lock,
        // and all other workspace files are present for build tools.
        copy_dir_recursive(&repo_dir, &work_dir)
            .await
            .context("failed to copy repo into temp dir")?;

        // Overlay changeset files on top of the repo copy.
        let mut changeset_paths: Vec<String> = Vec::with_capacity(changeset_data.len());
        for file in &changeset_data {
            changeset_paths.push(file.file_path.clone());
            if let Some(content) = &file.content {
                // Security: reject dangerous paths BEFORE any filesystem operations.
                // 1. Reject traversal components (../)
                if file.file_path.contains("..") {
                    anyhow::bail!(
                        "changeset file path contains traversal component: '{}'",
                        file.file_path
                    );
                }
                // 2. Reject absolute paths (would discard work_dir base in Path::join)
                if file.file_path.starts_with('/') || file.file_path.starts_with('\\') {
                    anyhow::bail!("changeset file path is absolute: '{}'", file.file_path);
                }
                let dest = work_dir.join(&file.file_path);
                // 3. Lexical prefix check: verify joined path stays under work_dir.
                //    This catches any remaining edge cases without touching the filesystem.
                if !dest.starts_with(&work_dir) {
                    anyhow::bail!(
                        "changeset file path escapes sandbox: '{}' resolves outside work_dir",
                        file.file_path
                    );
                }
                // Safe to create directories and write — path is validated
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

        // Intentionally load the pipeline from the canonical repo directory, not from
        // work_dir (the changeset overlay). This prevents a submitted changeset from
        // hijacking its own verification pipeline for security.
        let workflow = self.load_workflow(&repo_dir, repo_id).await?;

        // Auto-none: no pipeline configured, auto-approve with audit trail
        if workflow.stages.is_empty() {
            tracing::warn!(
                changeset_id = %changeset_id,
                repo = %repo_name,
                "auto-approving changeset: no verification pipeline and no recognized project type"
            );
            return Ok(true);
        }

        validate_workflow(&workflow).context("workflow validation failed")?;

        let mut env = HashMap::new();
        env.insert("DKOD_CHANGESET_ID".to_string(), changeset_id.to_string());
        env.insert("DKOD_REPO_ID".to_string(), repo_id.to_string());

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
            tracing::warn!(
                "workflow '{}' timed out after {:?}",
                workflow.name,
                workflow.timeout
            );
            false
        });

        // temp_dir cleaned up on drop
        Ok(passed)
    }

    async fn load_workflow(&self, repo_dir: &Path, repo_id: Uuid) -> Result<Workflow> {
        // Priority 1: .dkod/pipeline.yaml in repo
        let yaml_path = repo_dir.join(".dkod/pipeline.yaml");
        if yaml_path.exists() {
            info!("loading workflow from {}", yaml_path.display());
            let workflow = parse_yaml_workflow_file(&yaml_path).await?;
            if workflow.stages.is_empty() {
                anyhow::bail!(
                    "pipeline.yaml exists but defines no stages — refusing to auto-approve; \
                     add at least one stage or remove the file to use auto-detection"
                );
            }
            return Ok(workflow);
        }

        // Check for legacy .dekode/pipeline.toml and warn about migration
        let legacy_toml = repo_dir.join(".dekode/pipeline.toml");
        if legacy_toml.exists() {
            tracing::warn!(
                path = %legacy_toml.display(),
                "found legacy .dekode/pipeline.toml \u{2014} this format is no longer loaded; please migrate to .dkod/pipeline.yaml"
            );
        }

        // Priority 2: DB-stored pipeline
        let db_steps = self
            .engine
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

        // Priority 3: Auto-detect from project files
        info!("auto-detecting verification workflow from project files");
        Ok(detect_workflow(repo_dir))
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
                work_dir: None,
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
        allowed_commands: vec![],
    }
}

/// Auto-detect verification workflow from project files in the repo.
/// Scans the repo root AND immediate subdirectories for ALL known language
/// markers and creates steps for each.  When a marker is found in a
/// subdirectory, `step.work_dir` is set so the scheduler runs the command
/// in the correct directory.
/// Returns a no-stage workflow (auto-approve) if no known project type found.
pub fn detect_workflow(repo_dir: &Path) -> Workflow {
    use std::path::PathBuf;

    let mut steps: Vec<Step> = Vec::new();

    // Directories to scan: repo root + immediate subdirectories.
    // Each entry is (subdir_name, full_path).  Root uses "" as the name.
    // Skip hidden dirs, node_modules, and target to avoid noise.
    let skip = ["node_modules", "target"];
    let mut scan_dirs: Vec<(String, std::path::PathBuf)> =
        vec![("".to_string(), repo_dir.to_path_buf())];
    if let Ok(entries) = std::fs::read_dir(repo_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if name_str.starts_with('.') || skip.contains(&name_str.as_str()) {
                continue;
            }
            scan_dirs.push((name_str, path));
        }
    }

    // Sort subdirectories alphabetically for deterministic ordering.
    // Keep root (index 0) first, sort only subdirs.
    if scan_dirs.len() > 1 {
        scan_dirs[1..].sort_by(|a, b| a.0.cmp(&b.0));
    }

    // First pass: collect languages present in the root directory so that
    // subdirectory matches of the same language are suppressed (root wins).
    // Sibling subdirectories are NOT deduplicated against each other — e.g.
    // frontend/package.json and backend/package.json both get steps.
    let root_dir = &scan_dirs[0].1;
    let mut root_languages: std::collections::HashSet<&str> = std::collections::HashSet::new();
    if root_dir.join("Cargo.toml").exists() {
        root_languages.insert("rust");
    }
    if root_dir.join("package.json").exists() {
        let is_bun = root_dir.join("bun.lock").exists() || root_dir.join("bun.lockb").exists();
        root_languages.insert(if is_bun { "bun" } else { "node" });
    }
    if root_dir.join("pyproject.toml").exists() || root_dir.join("requirements.txt").exists() {
        root_languages.insert("python");
    }
    if root_dir.join("go.mod").exists() {
        root_languages.insert("go");
    }

    for (subdir_name, dir) in &scan_dirs {
        // Determine work_dir for steps detected in this directory.
        let is_root = subdir_name.is_empty();
        let step_work_dir: Option<PathBuf> = if is_root {
            None
        } else {
            Some(PathBuf::from(subdir_name))
        };

        // ── Rust ──
        if dir.join("Cargo.toml").exists() && (is_root || !root_languages.contains("rust")) {
            let name_prefix = if is_root {
                "rust".to_string()
            } else {
                format!("rust({subdir_name})")
            };
            steps.push(Step {
                name: format!("{name_prefix}:check"),
                step_type: StepType::Command {
                    run: "cargo check".to_string(),
                },
                timeout: Duration::from_secs(60),
                required: true,
                changeset_aware: true,
                work_dir: step_work_dir.clone(),
            });
            steps.push(Step {
                name: format!("{name_prefix}:test"),
                step_type: StepType::Command {
                    run: "cargo test".to_string(),
                },
                timeout: Duration::from_secs(60),
                required: true,
                changeset_aware: true,
                work_dir: step_work_dir.clone(),
            });
        }

        // ── Node / Bun ──
        if dir.join("package.json").exists() {
            let is_bun = dir.join("bun.lock").exists() || dir.join("bun.lockb").exists();
            let lang_key = if is_bun { "bun" } else { "node" };
            if is_root || !root_languages.contains(lang_key) {
                let (label, install_cmd, test_cmd) = if is_bun {
                    ("bun", "bun install --frozen-lockfile", "bun test")
                } else {
                    ("node", "npm ci", "npm test")
                };
                let name_prefix = if is_root {
                    label.to_string()
                } else {
                    format!("{label}({subdir_name})")
                };
                steps.push(Step {
                    name: format!("{name_prefix}:install"),
                    step_type: StepType::Command {
                        run: install_cmd.to_string(),
                    },
                    timeout: Duration::from_secs(120),
                    required: true,
                    changeset_aware: false,
                    work_dir: step_work_dir.clone(),
                });
                steps.push(Step {
                    name: format!("{name_prefix}:test"),
                    step_type: StepType::Command {
                        run: test_cmd.to_string(),
                    },
                    timeout: Duration::from_secs(60),
                    required: true,
                    changeset_aware: true,
                    work_dir: step_work_dir.clone(),
                });
            }
        }

        // ── Python ──
        if (dir.join("pyproject.toml").exists() || dir.join("requirements.txt").exists())
            && (is_root || !root_languages.contains("python"))
        {
            let name_prefix = if is_root {
                "python".to_string()
            } else {
                format!("python({subdir_name})")
            };
            if dir.join("pyproject.toml").exists() {
                steps.push(Step {
                    name: format!("{name_prefix}:install"),
                    step_type: StepType::Command {
                        run: "pip install -e .".to_string(),
                    },
                    timeout: Duration::from_secs(120),
                    required: true,
                    changeset_aware: false,
                    work_dir: step_work_dir.clone(),
                });
            }
            if dir.join("requirements.txt").exists() {
                steps.push(Step {
                    name: format!("{name_prefix}:install-deps"),
                    step_type: StepType::Command {
                        run: "pip install -r requirements.txt".to_string(),
                    },
                    timeout: Duration::from_secs(120),
                    required: true,
                    changeset_aware: false,
                    work_dir: step_work_dir.clone(),
                });
            }
            steps.push(Step {
                name: format!("{name_prefix}:test"),
                step_type: StepType::Command {
                    run: "pytest".to_string(),
                },
                timeout: Duration::from_secs(60),
                required: true,
                changeset_aware: true,
                work_dir: step_work_dir.clone(),
            });
        }

        // ── Go ──
        if dir.join("go.mod").exists() && (is_root || !root_languages.contains("go")) {
            let name_prefix = if is_root {
                "go".to_string()
            } else {
                format!("go({subdir_name})")
            };
            steps.push(Step {
                name: format!("{name_prefix}:build"),
                step_type: StepType::Command {
                    run: "go build ./...".to_string(),
                },
                timeout: Duration::from_secs(60),
                required: true,
                changeset_aware: true,
                work_dir: step_work_dir.clone(),
            });
            steps.push(Step {
                name: format!("{name_prefix}:vet"),
                step_type: StepType::Command {
                    run: "go vet ./...".to_string(),
                },
                timeout: Duration::from_secs(60),
                required: true,
                changeset_aware: true,
                work_dir: step_work_dir.clone(),
            });
            steps.push(Step {
                name: format!("{name_prefix}:test"),
                step_type: StepType::Command {
                    run: "go test ./...".to_string(),
                },
                timeout: Duration::from_secs(60),
                required: true,
                changeset_aware: true,
                work_dir: step_work_dir.clone(),
            });
        }
    }

    if steps.is_empty() {
        return Workflow {
            name: "auto-none".to_string(),
            timeout: Duration::from_secs(30),
            allowed_commands: vec![],
            stages: vec![],
        };
    }

    let unique_langs = steps
        .iter()
        .map(|s| s.name.split(':').next().unwrap_or(""))
        .collect::<std::collections::HashSet<_>>();
    let unique_work_dirs = steps
        .iter()
        .map(|s| s.work_dir.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let name = if unique_langs.len() > 1 || unique_work_dirs.len() > 1 {
        "auto-polyglot".to_string()
    } else {
        format!(
            "auto-{}",
            steps[0].name.split(':').next().unwrap_or("unknown")
        )
    };

    // Derive timeout from the sum of individual step timeouts (with a floor of 60s).
    let total_timeout_secs = steps
        .iter()
        .map(|s| s.timeout.as_secs())
        .sum::<u64>()
        .max(60);

    Workflow {
        name,
        timeout: Duration::from_secs(total_timeout_secs),
        allowed_commands: vec![],
        stages: vec![Stage {
            name: "checks".to_string(),
            parallel: false,
            steps,
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
        } else if file_type.is_symlink() {
            let target = tokio::fs::read_link(&src_path).await?;
            // Security: only recreate relative symlinks whose resolved target
            // stays within the destination tree. This prevents sandbox escapes
            // via crafted symlinks (e.g., link -> /etc/passwd, link -> ../../..).
            let target_str = target.to_string_lossy();
            if target_str.starts_with('/') || target_str.contains("..") {
                tracing::warn!(
                    src = %src_path.display(),
                    target = %target.display(),
                    "skipping symlink that points outside sandbox"
                );
                continue;
            }
            #[cfg(unix)]
            tokio::fs::symlink(target, &dst_path).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_workflow_rust() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("Cargo.toml"), b"[package]\nname = \"test\"")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-rust");
        assert_eq!(wf.stages.len(), 1);
        assert_eq!(wf.stages[0].steps.len(), 2);
    }

    #[tokio::test]
    async fn test_detect_workflow_bun() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("package.json"), b"{}")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("bun.lock"), b"")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-bun");
        assert_eq!(wf.stages[0].steps.len(), 2);
        let cmds: Vec<_> = wf.stages[0]
            .steps
            .iter()
            .filter_map(|s| {
                if let StepType::Command { run } = &s.step_type {
                    Some(run.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(cmds.contains(&"bun install --frozen-lockfile"));
        assert!(cmds.contains(&"bun test"));
    }

    #[tokio::test]
    async fn test_detect_workflow_bun_lockb() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("package.json"), b"{}")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("bun.lockb"), b"\x00")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-bun");
        assert_eq!(wf.stages[0].steps.len(), 2);
        let cmds: Vec<_> = wf.stages[0]
            .steps
            .iter()
            .filter_map(|s| {
                if let StepType::Command { run } = &s.step_type {
                    Some(run.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(cmds.contains(&"bun install --frozen-lockfile"));
        assert!(cmds.contains(&"bun test"));
    }

    #[tokio::test]
    async fn test_detect_workflow_npm() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("package.json"), b"{}")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-node");
        assert_eq!(wf.stages[0].steps.len(), 2);
        let cmds: Vec<_> = wf.stages[0]
            .steps
            .iter()
            .filter_map(|s| {
                if let StepType::Command { run } = &s.step_type {
                    Some(run.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(cmds.contains(&"npm ci"));
        assert!(cmds.contains(&"npm test"));
    }

    #[tokio::test]
    async fn test_detect_workflow_python_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("pyproject.toml"), b"[project]")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-python");
        // pyproject.toml only — install via pip install -e . plus test
        assert_eq!(wf.stages[0].steps.len(), 2);
        let cmds: Vec<_> = wf.stages[0]
            .steps
            .iter()
            .filter_map(|s| {
                if let StepType::Command { run } = &s.step_type {
                    Some(run.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(cmds.contains(&"pip install -e ."));
        assert!(cmds.contains(&"pytest"));
    }

    #[tokio::test]
    async fn test_detect_workflow_python_dual_file() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("pyproject.toml"), b"[project]")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("requirements.txt"), b"pytest\nrequests")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-python");
        // Both files present — install pyproject + requirements + test
        assert_eq!(wf.stages[0].steps.len(), 3);
        let cmds: Vec<_> = wf.stages[0]
            .steps
            .iter()
            .filter_map(|s| {
                if let StepType::Command { run } = &s.step_type {
                    Some(run.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(cmds.contains(&"pip install -e ."));
        assert!(cmds.contains(&"pip install -r requirements.txt"));
        assert!(cmds.contains(&"pytest"));
    }

    #[tokio::test]
    async fn test_detect_workflow_python_requirements() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("requirements.txt"), b"pytest\nrequests")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-python");
        // requirements.txt only — install-deps + test
        assert_eq!(wf.stages[0].steps.len(), 2);
        let cmds: Vec<_> = wf.stages[0]
            .steps
            .iter()
            .filter_map(|s| {
                if let StepType::Command { run } = &s.step_type {
                    Some(run.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(cmds.contains(&"pip install -r requirements.txt"));
        assert!(cmds.contains(&"pytest"));
    }

    #[tokio::test]
    async fn test_detect_workflow_python_both_files() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("pyproject.toml"), b"[project]")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("requirements.txt"), b"pytest\nrequests")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-python");
        // Both files — install-package + install-deps + test
        assert_eq!(wf.stages[0].steps.len(), 3);
        let cmds: Vec<_> = wf.stages[0]
            .steps
            .iter()
            .filter_map(|s| {
                if let StepType::Command { run } = &s.step_type {
                    Some(run.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(cmds.contains(&"pip install -e ."));
        assert!(cmds.contains(&"pip install -r requirements.txt"));
        assert!(cmds.contains(&"pytest"));
    }

    #[tokio::test]
    async fn test_detect_workflow_go() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("go.mod"), b"module example.com/test")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-go");
        assert_eq!(wf.stages[0].steps.len(), 3);
    }

    #[tokio::test]
    async fn test_detect_workflow_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-none");
        assert!(wf.stages.is_empty());
    }

    #[tokio::test]
    async fn test_copy_dir_recursive_copies_files() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        tokio::fs::write(src.path().join("Cargo.toml"), b"[package]\nname = \"test\"")
            .await
            .unwrap();
        tokio::fs::create_dir_all(src.path().join("src"))
            .await
            .unwrap();
        tokio::fs::write(src.path().join("src/main.rs"), b"fn main() {}")
            .await
            .unwrap();

        // .git dir should be skipped
        tokio::fs::create_dir_all(src.path().join(".git/objects"))
            .await
            .unwrap();
        tokio::fs::write(src.path().join(".git/HEAD"), b"ref: refs/heads/main")
            .await
            .unwrap();

        copy_dir_recursive(src.path(), dst.path()).await.unwrap();

        assert!(
            dst.path().join("Cargo.toml").exists(),
            "Cargo.toml must be at dst root"
        );
        assert!(
            dst.path().join("src/main.rs").exists(),
            "src/main.rs must exist"
        );
        assert!(!dst.path().join(".git").exists(), ".git must be skipped");
    }

    #[tokio::test]
    async fn test_copy_dir_recursive_handles_symlinks() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        // Create a regular file and a symlink to it
        tokio::fs::write(src.path().join("real.txt"), b"hello")
            .await
            .unwrap();
        #[cfg(unix)]
        tokio::fs::symlink("real.txt", src.path().join("link.txt"))
            .await
            .unwrap();

        copy_dir_recursive(src.path(), dst.path()).await.unwrap();

        assert!(dst.path().join("real.txt").exists());
        #[cfg(unix)]
        {
            let meta = tokio::fs::symlink_metadata(dst.path().join("link.txt"))
                .await
                .unwrap();
            assert!(meta.file_type().is_symlink(), "symlink should be preserved");
            let target = tokio::fs::read_link(dst.path().join("link.txt"))
                .await
                .unwrap();
            assert_eq!(target.to_str().unwrap(), "real.txt");
        }
    }

    #[tokio::test]
    async fn test_copy_dir_recursive_handles_dir_symlinks() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        // Create a real directory and a symlink to it
        tokio::fs::create_dir_all(src.path().join("real_dir"))
            .await
            .unwrap();
        tokio::fs::write(src.path().join("real_dir/file.txt"), b"content")
            .await
            .unwrap();
        #[cfg(unix)]
        tokio::fs::symlink("real_dir", src.path().join("linked_dir"))
            .await
            .unwrap();

        copy_dir_recursive(src.path(), dst.path()).await.unwrap();

        assert!(dst.path().join("real_dir/file.txt").exists());
        #[cfg(unix)]
        {
            let meta = tokio::fs::symlink_metadata(dst.path().join("linked_dir"))
                .await
                .unwrap();
            assert!(
                meta.file_type().is_symlink(),
                "dir symlink should be preserved"
            );
            let target = tokio::fs::read_link(dst.path().join("linked_dir"))
                .await
                .unwrap();
            assert_eq!(target.to_str().unwrap(), "real_dir");
        }
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

    #[tokio::test]
    async fn test_detect_workflow_polyglot_rust_and_node() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("Cargo.toml"), b"[package]\nname = \"test\"")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("package.json"), b"{}")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-polyglot");
        assert_eq!(wf.stages.len(), 1);
        let step_names: Vec<&str> = wf.stages[0].steps.iter().map(|s| s.name.as_str()).collect();
        assert!(
            step_names.iter().any(|n| n.starts_with("rust:")),
            "missing rust steps"
        );
        assert!(
            step_names.iter().any(|n| n.starts_with("node:")),
            "missing node steps"
        );
    }

    #[tokio::test]
    async fn test_detect_workflow_polyglot_three_languages() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("Cargo.toml"), b"[package]\nname = \"test\"")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("package.json"), b"{}")
            .await
            .unwrap();
        tokio::fs::write(
            dir.path().join("pyproject.toml"),
            b"[project]\nname = \"test\"",
        )
        .await
        .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-polyglot");
        let step_names: Vec<&str> = wf.stages[0].steps.iter().map(|s| s.name.as_str()).collect();
        assert!(
            step_names.iter().any(|n| n.starts_with("rust:")),
            "missing rust"
        );
        assert!(
            step_names.iter().any(|n| n.starts_with("node:")),
            "missing node"
        );
        assert!(
            step_names.iter().any(|n| n.starts_with("python:")),
            "missing python"
        );
    }

    #[tokio::test]
    async fn test_detect_workflow_polyglot_bun_and_go() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("package.json"), b"{}")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("bun.lock"), b"")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("go.mod"), b"module example.com/test")
            .await
            .unwrap();
        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-polyglot");
        let step_names: Vec<&str> = wf.stages[0].steps.iter().map(|s| s.name.as_str()).collect();
        assert!(
            step_names.iter().any(|n| n.starts_with("bun:")),
            "missing bun steps"
        );
        assert!(
            step_names.iter().any(|n| n.starts_with("go:")),
            "missing go steps"
        );
    }

    #[tokio::test]
    async fn test_detect_workflow_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        // Create markers in subdirectories, not root
        std::fs::create_dir_all(dir.path().join("rust")).unwrap();
        std::fs::write(
            dir.path().join("rust/Cargo.toml"),
            "[package]\nname = \"test\"",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("python")).unwrap();
        std::fs::write(dir.path().join("python/requirements.txt"), "flask\n").unwrap();

        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-polyglot");

        let steps: Vec<&Step> = wf.stages.iter().flat_map(|s| s.steps.iter()).collect();

        // Commands should be plain (no cd prefix) with work_dir set
        // Subdirectory steps include the subdir in the name: rust(rust):check
        let rust_steps: Vec<&&Step> = steps
            .iter()
            .filter(|s| s.name.starts_with("rust("))
            .collect();
        assert!(!rust_steps.is_empty(), "should detect rust steps");
        for step in &rust_steps {
            assert_eq!(
                step.work_dir.as_ref().map(|p| p.to_str().unwrap()),
                Some("rust"),
                "rust steps should have work_dir = 'rust'"
            );
            if let StepType::Command { run } = &step.step_type {
                assert!(
                    !run.contains("cd "),
                    "commands should not contain cd prefix"
                );
            }
        }

        let python_steps: Vec<&&Step> = steps
            .iter()
            .filter(|s| s.name.starts_with("python("))
            .collect();
        assert!(!python_steps.is_empty(), "should detect python steps");
        for step in &python_steps {
            assert_eq!(
                step.work_dir.as_ref().map(|p| p.to_str().unwrap()),
                Some("python"),
                "python steps should have work_dir = 'python'"
            );
            if let StepType::Command { run } = &step.step_type {
                assert!(
                    !run.contains("cd "),
                    "commands should not contain cd prefix"
                );
            }
        }

        // Verify deterministic ordering: python before rust (alphabetical)
        let first_lang = steps[0].name.split(':').next().unwrap();
        assert_eq!(
            first_lang, "python(python)",
            "python should come before rust (alphabetical subdirectory order)"
        );
    }

    #[tokio::test]
    async fn test_detect_workflow_same_language_sibling_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        // Two sibling subdirectories with the same language (Node)
        std::fs::create_dir_all(dir.path().join("backend")).unwrap();
        std::fs::write(dir.path().join("backend/package.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join("frontend")).unwrap();
        std::fs::write(dir.path().join("frontend/package.json"), "{}").unwrap();

        let wf = detect_workflow(dir.path());
        assert_eq!(wf.name, "auto-polyglot");

        // Both subdirectories should have steps, not just the first alphabetically
        let work_dirs: Vec<Option<&Path>> = wf
            .stages
            .iter()
            .flat_map(|s| s.steps.iter())
            .map(|step| step.work_dir.as_deref())
            .collect();

        assert!(
            work_dirs.iter().any(|wd| wd == &Some(Path::new("backend"))),
            "backend should have steps"
        );
        assert!(
            work_dirs
                .iter()
                .any(|wd| wd == &Some(Path::new("frontend"))),
            "frontend should have steps"
        );

        // Step names must include the subdirectory and be unique
        let step_names: Vec<&str> = wf
            .stages
            .iter()
            .flat_map(|s| s.steps.iter())
            .map(|step| step.name.as_str())
            .collect();

        assert!(
            step_names.contains(&"node(backend):install"),
            "missing node(backend):install"
        );
        assert!(
            step_names.contains(&"node(backend):test"),
            "missing node(backend):test"
        );
        assert!(
            step_names.contains(&"node(frontend):install"),
            "missing node(frontend):install"
        );
        assert!(
            step_names.contains(&"node(frontend):test"),
            "missing node(frontend):test"
        );

        // All step names must be unique (no duplicates)
        let unique_names: std::collections::HashSet<&str> = step_names.iter().copied().collect();
        assert_eq!(
            step_names.len(),
            unique_names.len(),
            "step names must be unique, got: {:?}",
            step_names
        );
    }
}

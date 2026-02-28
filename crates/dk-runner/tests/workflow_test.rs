use std::collections::HashMap;

use dk_runner::executor::process::ProcessExecutor;
use dk_runner::executor::StepStatus;
use dk_runner::scheduler::run_workflow;
use dk_runner::workflow::parser::parse_workflow_str;
use dk_runner::workflow::validator::validate_workflow;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_full_workflow_execution() {
    let toml = r#"
[pipeline]
name = "test-pipeline"
timeout = "1m"

[[stage]]
name = "checks"
parallel = true

[[stage.step]]
name = "echo-a"
run = "echo step-a"
timeout = "5s"

[[stage.step]]
name = "echo-b"
run = "echo step-b"
timeout = "5s"

[[stage]]
name = "gates"

[[stage.step]]
name = "semantic"
type = "semantic"
check = ["no-unsafe-added"]

[[stage.step]]
name = "review"
type = "agent-review"
prompt = "Check this"

[[stage.step]]
name = "approve"
type = "human-approve"
"#;

    let workflow = parse_workflow_str(toml).unwrap();
    validate_workflow(&workflow).unwrap();

    let exec = ProcessExecutor::new();
    let (tx, mut rx) = mpsc::channel(32);
    let dir = std::env::temp_dir();

    let passed = run_workflow(&workflow, &exec, &dir, &[], &HashMap::new(), &tx, None, None, None).await;
    drop(tx);

    assert!(passed, "all steps should pass");

    let mut results = Vec::new();
    while let Some(r) = rx.recv().await {
        results.push(r);
    }

    // 2 parallel checks + 3 sequential gates = 5 results
    assert_eq!(results.len(), 5);

    // First two should be from "checks" stage
    assert!(results.iter().take(2).all(|r| r.stage_name == "checks"));
    // Last three from "gates" stage
    assert!(results.iter().skip(2).all(|r| r.stage_name == "gates"));
}

#[tokio::test]
async fn test_workflow_with_changeset_scoping() {
    let toml = r#"
[pipeline]
name = "scoped"

[[stage]]
name = "test"

[[stage.step]]
name = "test"
run = "echo cargo-test-scoped"
timeout = "5s"
changeset_aware = true
"#;

    let workflow = parse_workflow_str(toml).unwrap();
    let exec = ProcessExecutor::new();
    let (tx, mut rx) = mpsc::channel(32);
    let dir = std::env::temp_dir();

    let files = vec!["crates/dk-engine/src/repo.rs".to_string()];

    let passed = run_workflow(&workflow, &exec, &dir, &files, &HashMap::new(), &tx, None, None, None).await;
    drop(tx);

    assert!(passed);
    let result = rx.recv().await.unwrap();
    assert_eq!(result.status, StepStatus::Pass);
}

#[tokio::test]
async fn test_invalid_workflow_rejected() {
    let toml = r#"
[pipeline]
name = "bad"
"#;
    let workflow = parse_workflow_str(toml).unwrap();
    assert!(validate_workflow(&workflow).is_err());
}

use anyhow::{Context, Result};
use colored::Colorize;
use dk_protocol::VerifyRequest;
use tokio_stream::StreamExt;

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let mut stream = client
        .verify(VerifyRequest {
            session_id: state.session_id,
            changeset_id: state.changeset_id,
        })
        .await?
        .into_inner();

    let mut steps = Vec::new();
    let mut all_passed = true;

    while let Some(step) = stream.next().await {
        let step = step.context("verify stream error")?;
        let passed = matches!(step.status.as_str(), "passed" | "PASSED");
        if !passed && step.required {
            all_passed = false;
        }

        if out.is_json() {
            steps.push(serde_json::json!({
                "step": step.step_name,
                "order": step.step_order,
                "status": step.status,
                "required": step.required,
                "output": step.output,
                "findings": step.findings.iter().map(|f| {
                    serde_json::json!({
                        "severity": f.severity,
                        "check_name": f.check_name,
                        "message": f.message,
                        "file_path": f.file_path,
                        "line": f.line,
                        "symbol": f.symbol,
                    })
                }).collect::<Vec<_>>(),
                "suggestions": step.suggestions.iter().map(|s| {
                    serde_json::json!({
                        "finding_index": s.finding_index,
                        "description": s.description,
                        "file_path": s.file_path,
                        "replacement": s.replacement,
                    })
                }).collect::<Vec<_>>(),
            }));
        } else {
            let status_display = match step.status.as_str() {
                "passed" | "PASSED" => "PASS".green().bold().to_string(),
                "failed" | "FAILED" => "FAIL".red().bold().to_string(),
                "skipped" | "SKIPPED" => "SKIP".yellow().to_string(),
                other => other.to_string(),
            };
            let required_tag = if step.required { "" } else { " (optional)" };
            println!("  {} {} {}{}", status_display, step.step_name.bold(), step.output.dimmed(), required_tag.dimmed());
        }
    }

    if out.is_json() {
        out.print_json(&serde_json::json!({"steps": steps, "passed": all_passed}));
    } else {
        println!();
        if all_passed {
            println!("{}", "All checks passed.".green().bold());
        } else {
            println!("{}", "Some checks failed.".red().bold());
        }
    }

    Ok(())
}

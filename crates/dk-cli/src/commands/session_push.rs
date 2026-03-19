use anyhow::{bail, Result};
use colored::Colorize;
use dk_protocol::{merge_response, MergeRequest};

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output, message: Option<&str>, force: bool) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let commit_message = message
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("dk push from {}", state.repo));

    let resp = client
        .merge(MergeRequest {
            session_id: state.session_id,
            changeset_id: state.changeset_id,
            commit_message,
            force,
        })
        .await?
        .into_inner();

    match resp.result {
        Some(merge_response::Result::Success(s)) => {
            if out.is_json() {
                out.print_json(&serde_json::json!({
                    "commit_hash": s.commit_hash,
                    "merged_version": s.merged_version,
                    "auto_rebased": s.auto_rebased,
                    "auto_rebased_files": s.auto_rebased_files,
                }));
            } else {
                println!("{} {}", "Merged.".green().bold(), s.commit_hash.dimmed());
                println!("  Version: {}", s.merged_version);
                if s.auto_rebased {
                    println!("  Auto-rebased {} file(s)", s.auto_rebased_files.len());
                }
            }
        }
        Some(merge_response::Result::Conflict(c)) => {
            if out.is_json() {
                out.print_json(&serde_json::json!({
                    "conflict": true,
                    "changeset_id": c.changeset_id,
                    "suggested_action": c.suggested_action,
                    "available_actions": c.available_actions,
                    "conflicts": c.conflicts.iter().map(|d| {
                        serde_json::json!({
                            "file": d.file_path,
                            "symbols": d.symbols,
                            "type": d.conflict_type,
                            "description": d.description,
                            "your_agent": d.your_agent,
                            "their_agent": d.their_agent,
                        })
                    }).collect::<Vec<_>>(),
                }));
            } else {
                println!("{} {} conflict(s):", "Merge blocked.".red().bold(), c.conflicts.len());
                for d in &c.conflicts {
                    println!(
                        "  {} {} [{}] ({}) -- {}",
                        "conflict:".red(),
                        d.file_path,
                        d.symbols.join(", "),
                        d.conflict_type,
                        d.description,
                    );
                }
                println!("  Suggested action: {}", c.suggested_action);
                if !c.available_actions.is_empty() {
                    println!("  Available actions: {}", c.available_actions.join(", "));
                }
            }
        }
        Some(merge_response::Result::OverwriteWarning(w)) => {
            if out.is_json() {
                out.print_json(&serde_json::json!({
                    "overwrite_warning": true,
                    "changeset_id": w.changeset_id,
                    "available_actions": w.available_actions,
                    "overwrites": w.overwrites.iter().map(|o| {
                        serde_json::json!({
                            "file_path": o.file_path,
                            "symbol_name": o.symbol_name,
                            "other_agent": o.other_agent,
                            "other_changeset_id": o.other_changeset_id,
                            "merged_at": o.merged_at.as_ref().map(|t| t.to_string()).unwrap_or_default(),
                        })
                    }).collect::<Vec<_>>(),
                }));
            } else {
                println!(
                    "{} {} symbol(s) recently overwritten:",
                    "Overwrite warning.".yellow().bold(),
                    w.overwrites.len()
                );
                for o in &w.overwrites {
                    let merged_at = o.merged_at.as_ref()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    println!(
                        "  {} {} in {} (by {}, merged at {})",
                        "overwrite:".yellow(),
                        o.symbol_name,
                        o.file_path,
                        o.other_agent,
                        merged_at,
                    );
                }
                if !w.available_actions.is_empty() {
                    println!("  Available actions: {}", w.available_actions.join(", "));
                }
            }
        }
        None => bail!("empty merge response from server"),
    }

    Ok(())
}

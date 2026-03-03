use anyhow::Result;
use colored::Colorize;
use dk_protocol::MergeRequest;

use crate::grpc;
use crate::output::Output;

pub async fn run(out: Output, message: Option<&str>) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let commit_message = message
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("dk push from {}", state.repo));

    let resp = client
        .merge(MergeRequest {
            session_id: state.session_id,
            changeset_id: state.changeset_id,
            commit_message,
        })
        .await?
        .into_inner();

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "commit_hash": resp.commit_hash,
            "merged_version": resp.merged_version,
            "conflicts": resp.conflicts.iter().map(|c| {
                serde_json::json!({
                    "file": c.file_path,
                    "type": c.conflict_type,
                    "description": c.description,
                })
            }).collect::<Vec<_>>(),
        }));
    } else if resp.conflicts.is_empty() {
        println!("{} {}", "Merged.".green().bold(), resp.commit_hash.dimmed());
        println!("  Version: {}", resp.merged_version);
    } else {
        println!("{} {} conflict(s):", "Merge blocked.".red().bold(), resp.conflicts.len());
        for c in &resp.conflicts {
            println!("  {} {} ({}) -- {}", "conflict:".red(), c.file_path, c.conflict_type, c.description);
        }
    }

    Ok(())
}

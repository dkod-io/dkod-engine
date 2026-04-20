use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use colored::Colorize;
use dk_protocol::FileWriteRequest;

use crate::grpc;
use crate::output::Output;

pub async fn run(
    out: Output,
    path: &str,
    content: Option<String>,
    from: Option<PathBuf>,
) -> Result<()> {
    let (mut client, state) = grpc::client_from_session().await?;

    let bytes = if let Some(text) = content {
        text.into_bytes()
    } else if let Some(file_path) = from {
        std::fs::read(&file_path)
            .with_context(|| format!("failed to read {}", file_path.display()))?
    } else {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .context("failed to read from stdin")?;
        buf
    };

    let resp = client
        .file_write(FileWriteRequest {
            session_id: state.session_id,
            path: path.to_string(),
            content: bytes,
        })
        .await?
        .into_inner();

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "path": path,
            "new_hash": resp.new_hash,
            "detected_changes": resp.detected_changes.iter().map(|c| {
                serde_json::json!({"symbol": c.symbol_name, "type": c.change_type})
            }).collect::<Vec<_>>(),
        }));
    } else {
        println!("{} {}", "Written.".green().bold(), path);
        if !resp.detected_changes.is_empty() {
            for c in &resp.detected_changes {
                println!(
                    "  {} {} ({})",
                    "\u{2022}".cyan(),
                    c.symbol_name,
                    c.change_type
                );
            }
        }
    }

    Ok(())
}

use anyhow::Result;
use colored::Colorize;
use dk_protocol::SessionStatusRequest;

use crate::grpc;
use crate::output::Output;
use crate::session::SessionState;

pub async fn run(out: Output) -> Result<()> {
    let state = match SessionState::load() {
        Ok(s) => s,
        Err(_) => {
            if out.is_json() {
                out.print_json(&serde_json::json!({"status": "no_session"}));
            } else {
                println!("No active session. Run `dk init <repo>` to start.");
            }
            return Ok(());
        }
    };

    let (mut client, _) = grpc::client_from_session().await?;

    let resp = client
        .get_session_status(SessionStatusRequest {
            session_id: state.session_id.clone(),
        })
        .await?
        .into_inner();

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "repo": state.repo,
            "server": state.server,
            "session_id": resp.session_id,
            "base_commit": resp.base_commit,
            "files_modified": resp.files_modified,
            "symbols_modified": resp.symbols_modified,
            "overlay_size_bytes": resp.overlay_size_bytes,
            "active_other_sessions": resp.active_other_sessions,
        }));
    } else {
        println!("{} {}", "Session:".green().bold(), state.repo.bold());
        println!("  Server:      {}", state.server);
        println!("  Session ID:  {}", state.session_id.dimmed());
        println!("  Changeset:   {}", state.changeset_id.dimmed());
        println!("  Base commit: {}", resp.base_commit);
        println!(
            "  Modified:    {} file(s), {} symbol(s)",
            resp.files_modified.len(),
            resp.symbols_modified.len()
        );
        if !resp.files_modified.is_empty() {
            println!("  Files:");
            for f in &resp.files_modified {
                println!("    {} {}", "M".yellow(), f);
            }
        }
        if resp.active_other_sessions > 0 {
            println!(
                "  Other sessions: {}",
                resp.active_other_sessions.to_string().yellow()
            );
        }
    }

    Ok(())
}

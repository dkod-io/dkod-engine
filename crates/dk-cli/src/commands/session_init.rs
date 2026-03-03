use anyhow::{Context, Result};
use colored::Colorize;

use crate::auth;
use crate::output::Output;
use crate::session::SessionState;

pub async fn run(out: Output, server: &str, repo: &str, intent: &str) -> Result<()> {
    let api_base = auth::api_base_from_grpc(server);
    let env_token = std::env::var("DEKODE_AUTH_TOKEN").ok();
    let token = auth::resolve_token(&api_base, env_token.as_deref()).await?;

    let mut client = dk_agent_sdk::AgentClient::connect(server, &token)
        .await
        .context("failed to connect — is dk-server running?")?;

    let session = client
        .init(repo, intent)
        .await
        .context("CONNECT handshake failed")?;

    let state = SessionState {
        server: server.to_string(),
        repo: repo.to_string(),
        session_id: session.session_id.clone(),
        changeset_id: session.changeset_id.clone(),
        workspace_id: String::new(),
    };
    state.save()?;

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "session_id": session.session_id,
            "changeset_id": session.changeset_id,
            "codebase_version": session.codebase_version,
            "repo": repo,
            "server": server,
        }));
    } else {
        println!("{} {}", "Connected.".green().bold(), repo.bold());
        println!("  Session:   {}", session.session_id);
        println!("  Changeset: {}", session.changeset_id);
        println!("  Version:   {}", session.codebase_version);
        println!("  Server:    {}", server);
    }

    Ok(())
}

use anyhow::{Context, Result};
use colored::Colorize;

use crate::auth;
use crate::grpc;
use crate::output::Output;
use crate::session::SessionState;

pub async fn run(out: Output, server: &str, repo: Option<&str>, intent: &str) -> Result<()> {
    let repo = match repo {
        Some(r) => r.to_string(),
        None => {
            // Auto-detect from git remote using gix (no git binary required)
            let remote_url = crate::util::remote_origin_url().context(
                "no repository specified and no git remote found.\n\
                 Usage: dk init <owner/repo>\n\
                 Or run inside a git repository with an origin remote.",
            )?;
            crate::util::repo_name_from_remote(&remote_url)
                .context("could not parse repository name from git remote URL")?
        }
    };

    let api_base = auth::api_base_from_grpc(server);
    let env_token = std::env::var("DKOD_AUTH_TOKEN").ok();
    let token = auth::resolve_token(&api_base, env_token.as_deref()).await?;

    let mut client = grpc::connect(server, &token)
        .await
        .context("failed to connect — is dk-server running?")?;

    let resp = client
        .connect(dk_protocol::ConnectRequest {
            agent_id: format!("dk-cli-{}", std::process::id()),
            auth_token: token,
            codebase: repo.clone(),
            intent: intent.to_string(),
            workspace_config: None,
            agent_name: String::new(),
        })
        .await
        .context("CONNECT handshake failed")?
        .into_inner();

    let state = SessionState {
        server: server.to_string(),
        repo: repo.clone(),
        session_id: resp.session_id.clone(),
        changeset_id: resp.changeset_id.clone(),
        workspace_id: String::new(),
    };
    state.save()?;

    if out.is_json() {
        out.print_json(&serde_json::json!({
            "session_id": resp.session_id,
            "changeset_id": resp.changeset_id,
            "codebase_version": resp.codebase_version,
            "repo": repo,
            "server": server,
        }));
    } else {
        println!("{} {}", "Connected.".green().bold(), repo.bold());
        println!("  Session:   {}", resp.session_id);
        println!("  Changeset: {}", resp.changeset_id);
        println!("  Version:   {}", resp.codebase_version);
        println!("  Server:    {}", server);
    }

    Ok(())
}

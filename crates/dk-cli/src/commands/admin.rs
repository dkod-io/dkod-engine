use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::auth;
use crate::grpc;

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Force-abandon a stranded workspace (operator escape hatch).
    ///
    /// Requires an admin JWT (scope = "admin") set via DKOD_AUTH_TOKEN.
    Abandon {
        /// Session ID (UUID) of the stranded workspace to abandon.
        #[arg(long)]
        session_id: String,
        /// Operator name for the audit log.
        #[arg(long, default_value = "admin-cli")]
        operator: String,
        /// gRPC server address (overrides DKOD_GRPC_ADDR / default).
        #[arg(long)]
        server: Option<String>,
    },
}

/// `global_server` is the top-level `--server` flag value (from `Cli::server`).
/// It is used as a fallback when the subcommand-local `--server` flag is not set,
/// giving `dk --server <addr> admin abandon ...` the expected behaviour.
pub async fn run(args: AdminArgs, global_server: Option<String>) -> Result<()> {
    match args.command {
        AdminCommand::Abandon {
            session_id,
            operator,
            server,
        } => {
            // Resolve the server address:
            //   subcommand --server flag > global --server flag > DKOD_GRPC_ADDR > default.
            let addr = server
                .or(global_server)
                .or_else(|| std::env::var("DKOD_GRPC_ADDR").ok())
                .unwrap_or_else(|| "https://agent.dkod.io:443".to_string());

            let api_base = auth::api_base_from_grpc(&addr);
            let env_token = std::env::var("DKOD_AUTH_TOKEN").ok();
            let token = auth::resolve_token(&api_base, env_token.as_deref())
                .await
                .context("failed to resolve auth token — set DKOD_AUTH_TOKEN to an admin JWT")?;

            let mut client = grpc::connect(&addr, &token)
                .await
                .context("failed to connect to dk-server")?;

            let mut request = tonic::Request::new(dk_protocol::AbandonRequest {
                session_id: session_id.clone(),
            });
            request.metadata_mut().insert(
                "dk-admin-operator",
                operator
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid operator value: {e}"))?,
            );

            let resp = client.abandon(request).await?.into_inner();
            if !resp.success {
                anyhow::bail!(
                    "failed to abandon session {} (changeset {}, reason {})",
                    session_id, resp.changeset_id, resp.abandoned_reason
                );
            }
            println!(
                "Abandoned session {} (changeset {}, reason {})",
                session_id, resp.changeset_id, resp.abandoned_reason
            );
            Ok(())
        }
    }
}

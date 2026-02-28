use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use dk_engine::repo::Engine;
use dk_protocol::agent_service_server::AgentServiceServer;
use dk_protocol::ProtocolServer;
use sqlx::PgPool;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "dk-server", about = "Dekode Reference Server — engine + Agent Protocol")]
struct Cli {
    /// PostgreSQL connection string
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// Path to local storage (search index, repos, etc.)
    #[arg(long, env = "STORAGE_PATH", default_value = "./data")]
    storage_path: PathBuf,

    /// Address to listen on (gRPC)
    #[arg(long, env = "LISTEN_ADDR", default_value = "[::1]:50051")]
    listen_addr: String,

    /// Shared auth token agents must present on Connect
    #[arg(long, env = "AUTH_TOKEN")]
    auth_token: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("dk=info,tower=info")),
        )
        .init();

    let cli = Cli::parse();

    tracing::info!("Connecting to database...");
    let db = PgPool::connect(&cli.database_url).await?;

    tracing::info!("Running migrations...");
    sqlx::migrate!("../dk-engine/migrations").run(&db).await?;

    tracing::info!("Initializing engine at {:?}", cli.storage_path);
    std::fs::create_dir_all(&cli.storage_path)?;
    let engine = Engine::new(cli.storage_path, db)?;
    let engine = Arc::new(engine);

    let protocol = ProtocolServer::new(engine, cli.auth_token);

    let grpc_addr = cli.listen_addr.parse()?;
    tracing::info!("Starting gRPC server on {}", grpc_addr);

    tonic::transport::Server::builder()
        .add_service(AgentServiceServer::new(protocol))
        .serve(grpc_addr)
        .await?;

    Ok(())
}

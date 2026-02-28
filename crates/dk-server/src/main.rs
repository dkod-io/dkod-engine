use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use dk_engine::repo::Engine;
use dk_protocol::agent_service_server::AgentServiceServer;
use dk_protocol::auth::AuthConfig;
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

    /// JWT signing secret (enables JWT auth mode; if both --auth-token
    /// and --jwt-secret are provided, dual-mode is used)
    #[arg(long, env = "JWT_SECRET")]
    jwt_secret: Option<String>,
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

    if cli.jwt_secret.is_none() && cli.auth_token.is_empty() {
        anyhow::bail!("Either --auth-token or --jwt-secret must be provided");
    }

    let auth_config = match (cli.jwt_secret, cli.auth_token.is_empty()) {
        (Some(jwt_secret), true) => AuthConfig::Jwt { secret: jwt_secret },
        (Some(jwt_secret), false) => AuthConfig::Dual {
            jwt_secret,
            shared_token: cli.auth_token,
        },
        (None, _) => AuthConfig::SharedSecret {
            token: cli.auth_token,
        },
    };

    let protocol = ProtocolServer::new(engine, auth_config);

    let grpc_addr = cli.listen_addr.parse()?;
    tracing::info!("Starting gRPC server on {}", grpc_addr);

    let grpc_service = AgentServiceServer::new(protocol);
    let grpc_web_service = tonic_web::enable(grpc_service);

    tonic::transport::Server::builder()
        .accept_http1(true)
        .add_service(grpc_web_service)
        .serve(grpc_addr)
        .await?;

    Ok(())
}

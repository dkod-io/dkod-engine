use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls crypto provider before any TLS usage
    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    // Emit any gate-config warnings once, at startup.
    for warning in dk_mcp::review_gate::startup_warnings(&dk_mcp::review_gate::GateConfig::from_env()) {
        eprintln!("{}", warning);
    }

    let server = dk_mcp::server::DkodMcp::new().await;
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

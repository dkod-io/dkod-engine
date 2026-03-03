//! Shared gRPC client setup for session commands.

use anyhow::{Context, Result};
use dk_protocol::agent_service_client::AgentServiceClient;
use tonic::transport::Channel;

use crate::session::SessionState;

pub async fn client_from_session() -> Result<(AgentServiceClient<Channel>, SessionState)> {
    let state = SessionState::load()?;
    let channel = Channel::from_shared(state.server.clone())
        .context("invalid server address in session")?
        .connect()
        .await
        .context("failed to connect — is dk-server running?")?;
    Ok((AgentServiceClient::new(channel), state))
}

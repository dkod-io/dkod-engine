//! Shared gRPC client setup for session commands.

use anyhow::{Context, Result};
use dk_protocol::agent_service_client::AgentServiceClient;
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::{Request, Status};

use crate::session::SessionState;

/// Interceptor that adds `authorization: Bearer <token>` to every gRPC request.
#[derive(Clone)]
pub struct BearerAuth {
    token: String,
}

impl Interceptor for BearerAuth {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        req.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", self.token)
                .parse()
                .map_err(|_| Status::internal("invalid token format"))?,
        );
        Ok(req)
    }
}

/// Authenticated gRPC client type.
pub type AuthClient = AgentServiceClient<InterceptedService<Channel, BearerAuth>>;

/// Build an authenticated gRPC channel to `addr`.
pub async fn connect(addr: &str, token: &str) -> Result<AuthClient> {
    let mut endpoint = Channel::from_shared(addr.to_string()).context("invalid server address")?;

    if addr.starts_with("https://") {
        endpoint = endpoint
            .tls_config(ClientTlsConfig::new().with_webpki_roots())
            .context("TLS config error")?;
    }

    let channel = endpoint
        .connect()
        .await
        .context("failed to connect — is dk-server running?")?;

    let interceptor = BearerAuth {
        token: token.to_string(),
    };
    Ok(AgentServiceClient::with_interceptor(channel, interceptor))
}

/// Load session state and build an authenticated client from the cached token.
pub async fn client_from_session() -> Result<(AuthClient, SessionState)> {
    let state = SessionState::load()?;
    let token = crate::auth::resolve_token(
        &crate::auth::api_base_from_grpc(&state.server),
        std::env::var("DKOD_AUTH_TOKEN").ok().as_deref(),
    )
    .await?;
    let client = connect(&state.server, &token).await?;
    Ok((client, state))
}

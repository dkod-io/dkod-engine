//! gRPC client connection and authentication for dk-mcp.

use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};

use crate::agent_service_client::AgentServiceClient;

/// Interceptor that adds `authorization: Bearer <token>` to every gRPC request.
#[derive(Clone)]
pub struct BearerAuthInterceptor {
    token: String,
}

impl BearerAuthInterceptor {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

impl Interceptor for BearerAuthInterceptor {
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

/// Type alias for the authenticated gRPC client.
pub type AuthenticatedClient = AgentServiceClient<
    tonic::service::interceptor::InterceptedService<Channel, BearerAuthInterceptor>,
>;

/// Create a new gRPC client with bearer token authentication.
///
/// Every RPC will include `authorization: Bearer <token>` in metadata.
pub async fn connect_with_auth(
    addr: &str,
    token: String,
) -> Result<AuthenticatedClient, Box<dyn std::error::Error + Send + Sync>> {
    tracing::debug!("connecting to gRPC at {addr}");
    let mut endpoint = Channel::from_shared(addr.to_string()).map_err(
        |e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid gRPC address: {e}"),
            ))
        },
    )?;

    // Enable TLS with webpki root certs for https:// endpoints
    if addr.starts_with("https://") {
        tracing::debug!("enabling TLS with webpki roots");
        endpoint =
            endpoint.tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?;
    }

    // Set reasonable timeouts
    endpoint = endpoint
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30));

    tracing::debug!("calling endpoint.connect()...");
    let channel = endpoint.connect().await?;
    tracing::debug!("gRPC channel connected");
    let interceptor = BearerAuthInterceptor::new(token);
    Ok(AgentServiceClient::with_interceptor(channel, interceptor))
}

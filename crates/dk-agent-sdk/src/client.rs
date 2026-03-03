use dk_protocol::agent_service_client::AgentServiceClient;
use tonic::transport::{Channel, ClientTlsConfig};

use crate::error::{Result, SdkError};
use crate::session::Session;
use crate::types::ConnectResult;

/// Top-level client for the Dekode Agent Protocol.
///
/// Use [`AgentClient::connect`] to establish a gRPC channel to the server, then
/// call [`AgentClient::init`] to create a stateful session for a specific
/// codebase and intent.
pub struct AgentClient {
    inner: AgentServiceClient<Channel>,
    auth_token: String,
}

impl AgentClient {
    /// Connect to a Dekode Agent Protocol server at the given address.
    ///
    /// `addr` should be a full URI such as `"http://localhost:50051"` or
    /// `"https://agent.dkod.io:443"`. TLS is enabled automatically for
    /// `https://` addresses.
    pub async fn connect(addr: &str, auth_token: &str) -> Result<Self> {
        let mut endpoint = Channel::from_shared(addr.to_string())
            .map_err(|e| SdkError::Connection(e.to_string()))?;

        if addr.starts_with("https://") {
            endpoint = endpoint
                .tls_config(ClientTlsConfig::new().with_webpki_roots())
                .map_err(|e| SdkError::Connection(format!("TLS config error: {e}")))?;
        }

        let channel = endpoint.connect().await?;

        Ok(Self {
            inner: AgentServiceClient::new(channel),
            auth_token: auth_token.to_string(),
        })
    }

    /// Perform the CONNECT handshake: authenticate, specify the target codebase
    /// and intent, and receive a [`Session`] bound to the resulting changeset.
    pub async fn init(&mut self, repo: &str, intent: &str) -> Result<Session> {
        let resp = self
            .inner
            .connect(dk_protocol::ConnectRequest {
                agent_id: format!("sdk-{}", uuid::Uuid::new_v4()),
                auth_token: self.auth_token.clone(),
                codebase: repo.to_string(),
                intent: intent.to_string(),
                workspace_config: None,
            })
            .await?
            .into_inner();

        let connect_result = ConnectResult {
            session_id: resp.session_id.clone(),
            changeset_id: resp.changeset_id.clone(),
            codebase_version: resp.codebase_version.clone(),
            summary: resp.summary,
        };

        Ok(Session::new(self.inner.clone(), connect_result))
    }
}

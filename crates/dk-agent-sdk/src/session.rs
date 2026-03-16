use dk_protocol::agent_service_client::AgentServiceClient;
use dk_protocol::{
    Change as ProtoChange, ChangeType, ContextDepth, ContextRequest, MergeRequest, SubmitRequest,
    VerifyRequest, WatchRequest,
};
use tonic::transport::Channel;
use tokio_stream::StreamExt;

use crate::error::Result;
use crate::types::*;

/// A stateful agent session bound to a changeset on the server.
///
/// Obtained from [`crate::AgentClient::init`].  All operations (context,
/// submit, verify, merge, watch) are scoped to this session's changeset.
pub struct Session {
    client: AgentServiceClient<Channel>,
    /// The server-assigned session identifier.
    pub session_id: String,
    /// The changeset created by the CONNECT handshake.
    pub changeset_id: String,
    /// The codebase version at the time of connection.
    pub codebase_version: String,
}

impl Session {
    pub(crate) fn new(client: AgentServiceClient<Channel>, result: ConnectResult) -> Self {
        Self {
            client,
            session_id: result.session_id,
            changeset_id: result.changeset_id,
            codebase_version: result.codebase_version,
        }
    }

    /// Query the semantic code graph for symbols matching `query`.
    pub async fn context(
        &mut self,
        query: &str,
        depth: Depth,
        max_tokens: u32,
    ) -> Result<ContextResult> {
        let proto_depth = match depth {
            Depth::Signatures => ContextDepth::Signatures as i32,
            Depth::Full => ContextDepth::Full as i32,
            Depth::CallGraph => ContextDepth::CallGraph as i32,
        };

        let resp = self
            .client
            .context(ContextRequest {
                session_id: self.session_id.clone(),
                query: query.to_string(),
                depth: proto_depth,
                include_tests: false,
                include_dependencies: false,
                max_tokens,
            })
            .await?
            .into_inner();

        Ok(ContextResult {
            symbols: resp.symbols,
            call_graph: resp.call_graph,
            dependencies: resp.dependencies,
            estimated_tokens: resp.estimated_tokens,
        })
    }

    /// Submit a batch of code changes to the current changeset.
    pub async fn submit(&mut self, changes: Vec<Change>) -> Result<SubmitResult> {
        let proto_changes: Vec<ProtoChange> = changes
            .iter()
            .map(|c| match c {
                Change::Add { path, content } => ProtoChange {
                    r#type: ChangeType::AddFunction as i32,
                    symbol_name: String::new(),
                    file_path: path.clone(),
                    old_symbol_id: None,
                    new_source: content.clone(),
                    rationale: String::new(),
                },
                Change::Modify { path, content } => ProtoChange {
                    r#type: ChangeType::ModifyFunction as i32,
                    symbol_name: String::new(),
                    file_path: path.clone(),
                    old_symbol_id: None,
                    new_source: content.clone(),
                    rationale: String::new(),
                },
                Change::Delete { path } => ProtoChange {
                    r#type: ChangeType::DeleteFunction as i32,
                    symbol_name: String::new(),
                    file_path: path.clone(),
                    old_symbol_id: None,
                    new_source: String::new(),
                    rationale: String::new(),
                },
            })
            .collect();

        let resp = self
            .client
            .submit(SubmitRequest {
                session_id: self.session_id.clone(),
                intent: String::new(),
                changes: proto_changes,
                changeset_id: self.changeset_id.clone(),
            })
            .await?
            .into_inner();

        let status = format!("{:?}", resp.status());
        Ok(SubmitResult {
            changeset_id: resp.changeset_id,
            status,
            errors: resp.errors,
        })
    }

    /// Trigger the verification pipeline and collect all step results.
    pub async fn verify(&mut self) -> Result<Vec<VerifyStepResult>> {
        let mut stream = self
            .client
            .verify(VerifyRequest {
                session_id: self.session_id.clone(),
                changeset_id: self.changeset_id.clone(),
            })
            .await?
            .into_inner();

        let mut results = Vec::new();
        while let Some(step) = stream.next().await {
            results.push(step?);
        }
        Ok(results)
    }

    /// Merge the current changeset into a Git commit.
    pub async fn merge(&mut self, message: &str) -> Result<MergeResult> {
        let resp = self
            .client
            .merge(MergeRequest {
                session_id: self.session_id.clone(),
                changeset_id: self.changeset_id.clone(),
                commit_message: message.to_string(),
            })
            .await?
            .into_inner();

        Ok(MergeResult {
            commit_hash: resp.commit_hash,
            merged_version: resp.merged_version,
            conflicts: resp.conflicts,
        })
    }

    /// Subscribe to repository events (other agents' changes, merges, etc.).
    pub async fn watch(
        &mut self,
        filter: Filter,
    ) -> Result<tonic::Streaming<WatchEvent>> {
        let filter_str = match filter {
            Filter::All => "all",
            Filter::Symbols => "symbols",
            Filter::Files => "files",
        };

        let stream = self
            .client
            .watch(WatchRequest {
                session_id: self.session_id.clone(),
                repo_id: String::new(),
                filter: filter_str.to_string(),
            })
            .await?
            .into_inner();

        Ok(stream)
    }
}

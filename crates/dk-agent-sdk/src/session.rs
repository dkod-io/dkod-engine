use dk_protocol::agent_service_client::AgentServiceClient;
use dk_protocol::{
    merge_response, ApproveRequest, Change as ProtoChange, ChangeType, CloseRequest, ContextDepth,
    ContextRequest, FileListRequest, FileReadRequest, FileWriteRequest, MergeRequest,
    PreSubmitCheckRequest, PushMode as ProtoPushMode, PushRequest, RecordReviewRequest,
    ResolutionMode as ProtoResolutionMode, ResolveRequest, ReviewRequest, SessionStatusRequest,
    SubmitRequest, VerifyRequest, WatchRequest,
};
use tokio_stream::StreamExt;
use tonic::transport::Channel;

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
    pub async fn submit(&mut self, changes: Vec<Change>, intent: &str) -> Result<SubmitResult> {
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
                intent: intent.to_string(),
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
    ///
    /// If `force` is `true`, the recency guard is bypassed (use after the
    /// caller has acknowledged an [`MergeResult::OverwriteWarning`]).
    pub async fn merge(&mut self, message: &str, force: bool) -> Result<MergeResult> {
        let resp = self
            .client
            .merge(MergeRequest {
                session_id: self.session_id.clone(),
                changeset_id: self.changeset_id.clone(),
                commit_message: message.to_string(),
                force,
                author_name: String::new(),
                author_email: String::new(),
            })
            .await?
            .into_inner();

        match resp.result {
            Some(merge_response::Result::Success(s)) => Ok(MergeResult::Success(s)),
            Some(merge_response::Result::Conflict(c)) => Ok(MergeResult::Conflict(c)),
            Some(merge_response::Result::OverwriteWarning(w)) => {
                Ok(MergeResult::OverwriteWarning(w))
            }
            None => Err(tonic::Status::internal("empty merge response").into()),
        }
    }

    /// Read a file from the session workspace overlay.
    pub async fn file_read(&mut self, path: &str) -> Result<FileReadResult> {
        let resp = self
            .client
            .file_read(FileReadRequest {
                session_id: self.session_id.clone(),
                path: path.to_string(),
            })
            .await?
            .into_inner();

        Ok(FileReadResult {
            content: String::from_utf8_lossy(&resp.content).into_owned(),
            hash: resp.hash,
            modified_in_session: resp.modified_in_session,
        })
    }

    /// Write a file to the session workspace overlay.
    pub async fn file_write(&mut self, path: &str, content: &str) -> Result<FileWriteResult> {
        let resp = self
            .client
            .file_write(FileWriteRequest {
                session_id: self.session_id.clone(),
                path: path.to_string(),
                content: content.as_bytes().to_vec(),
            })
            .await?
            .into_inner();

        Ok(FileWriteResult {
            new_hash: resp.new_hash,
            detected_changes: resp.detected_changes,
            conflict_warnings: resp.conflict_warnings,
        })
    }

    /// List files in the session workspace, optionally filtered by path prefix.
    pub async fn file_list(&mut self, prefix: Option<&str>) -> Result<FileListResult> {
        let resp = self
            .client
            .file_list(FileListRequest {
                session_id: self.session_id.clone(),
                prefix: prefix.map(|s| s.to_string()),
                only_modified: false,
            })
            .await?
            .into_inner();

        Ok(FileListResult { files: resp.files })
    }

    /// Get the current state of this session's workspace.
    pub async fn session_status(&mut self) -> Result<SessionStatusResult> {
        let resp = self
            .client
            .get_session_status(SessionStatusRequest {
                session_id: self.session_id.clone(),
            })
            .await?
            .into_inner();

        Ok(SessionStatusResult {
            session_id: resp.session_id,
            base_commit: resp.base_commit,
            files_modified: resp.files_modified,
            symbols_modified: resp.symbols_modified,
            overlay_size_bytes: resp.overlay_size_bytes,
            active_other_sessions: resp.active_other_sessions,
        })
    }

    /// Check for semantic conflicts before submitting.
    pub async fn pre_submit_check(&mut self) -> Result<PreSubmitResult> {
        let resp = self
            .client
            .pre_submit_check(PreSubmitCheckRequest {
                session_id: self.session_id.clone(),
            })
            .await?
            .into_inner();

        Ok(PreSubmitResult {
            has_conflicts: resp.has_conflicts,
            potential_conflicts: resp.potential_conflicts,
            files_modified: resp.files_modified,
            symbols_changed: resp.symbols_changed,
        })
    }

    /// Push the merged changeset to GitHub as a branch or pull request.
    pub async fn push(
        &mut self,
        mode: PushMode,
        branch_name: &str,
        pr_title: &str,
        pr_body: &str,
    ) -> Result<PushResult> {
        let proto_mode = match mode {
            PushMode::Branch => ProtoPushMode::Branch as i32,
            PushMode::Pr => ProtoPushMode::Pr as i32,
        };

        let resp = self
            .client
            .push(PushRequest {
                session_id: self.session_id.clone(),
                mode: proto_mode,
                branch_name: branch_name.to_string(),
                pr_title: pr_title.to_string(),
                pr_body: pr_body.to_string(),
            })
            .await?
            .into_inner();

        Ok(PushResult {
            branch_name: resp.branch_name,
            pr_url: resp.pr_url,
            commit_hash: resp.commit_hash,
            changeset_ids: resp.changeset_ids,
        })
    }

    /// Approve the session's current changeset, optionally overriding a review gate.
    pub async fn approve(&mut self, override_reason: Option<&str>) -> Result<ApproveResult> {
        let resp = self
            .client
            .approve(ApproveRequest {
                session_id: self.session_id.clone(),
                override_reason: override_reason.map(|s| s.to_string()),
                review_snapshot: None,
            })
            .await?
            .into_inner();

        Ok(ApproveResult {
            success: resp.success,
            changeset_id: resp.changeset_id,
            new_state: resp.new_state,
            message: resp.message,
        })
    }

    /// Resolve conflicts on the session's changeset.
    ///
    /// For `ResolutionMode::Manual`, supply `conflict_id` (file path) and
    /// `manual_content` (the resolved file content).
    pub async fn resolve(
        &mut self,
        mode: ResolutionMode,
        conflict_id: Option<&str>,
        manual_content: Option<&str>,
    ) -> Result<ResolveResult> {
        let proto_mode = match mode {
            ResolutionMode::Proceed => ProtoResolutionMode::Proceed as i32,
            ResolutionMode::KeepYours => ProtoResolutionMode::KeepYours as i32,
            ResolutionMode::KeepTheirs => ProtoResolutionMode::KeepTheirs as i32,
            ResolutionMode::Manual => ProtoResolutionMode::Manual as i32,
        };

        let resp = self
            .client
            .resolve(ResolveRequest {
                session_id: self.session_id.clone(),
                resolution: proto_mode,
                conflict_id: conflict_id.map(|s| s.to_string()),
                manual_content: manual_content.map(|s| s.to_string()),
            })
            .await?
            .into_inner();

        Ok(ResolveResult {
            success: resp.success,
            changeset_id: resp.changeset_id,
            new_state: resp.new_state,
            message: resp.message,
            conflicts_resolved: resp.conflicts_resolved,
            conflicts_remaining: resp.conflicts_remaining,
        })
    }

    /// Close this session and destroy its workspace overlay on the server.
    pub async fn close(&mut self) -> Result<CloseResult> {
        let resp = self
            .client
            .close(CloseRequest {
                session_id: self.session_id.clone(),
            })
            .await?
            .into_inner();

        Ok(CloseResult {
            success: resp.success,
            message: resp.message,
        })
    }

    /// Fetch all AI review results recorded for this session's changeset.
    pub async fn review(&mut self) -> Result<ReviewListResult> {
        let resp = self
            .client
            .review(ReviewRequest {
                session_id: self.session_id.clone(),
                changeset_id: self.changeset_id.clone(),
            })
            .await?
            .into_inner();

        Ok(ReviewListResult {
            reviews: resp.reviews,
        })
    }

    /// Record an AI-generated code review result for this session's changeset.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_review(
        &mut self,
        tier: &str,
        score: Option<i32>,
        summary: Option<&str>,
        findings: Vec<ReviewFindingProto>,
        provider: &str,
        model: &str,
        duration_ms: i64,
    ) -> Result<RecordReviewResult> {
        let resp = self
            .client
            .record_review(RecordReviewRequest {
                session_id: self.session_id.clone(),
                changeset_id: self.changeset_id.clone(),
                tier: tier.to_string(),
                score,
                summary: summary.map(|s| s.to_string()),
                findings,
                provider: provider.to_string(),
                model: model.to_string(),
                duration_ms,
            })
            .await?
            .into_inner();

        Ok(RecordReviewResult {
            review_id: resp.review_id,
            accepted: resp.accepted,
        })
    }

    /// Subscribe to repository events (other agents' changes, merges, etc.).
    pub async fn watch(&mut self, filter: Filter) -> Result<tonic::Streaming<WatchEvent>> {
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

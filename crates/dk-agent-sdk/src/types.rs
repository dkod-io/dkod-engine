// Re-export proto types that SDK consumers will use directly.
//
// We import from the `agent` submodule rather than the crate root because
// `SymbolRef`, `CallEdgeRef`, `DependencyRef` are duplicated between the
// `agent` and `types` proto modules (rustc would otherwise emit
// `ambiguous_glob_reexports`). Reaching into `agent::` picks a single
// canonical version for SDK consumers.
pub use dk_protocol::generated::dkod::v1::agent::{
    CallEdgeRef, CodebaseSummary, ConflictDetail, ConflictWarning, DependencyRef, FileEntry,
    MergeConflict, MergeSuccess, RecentOverwriteWarning, ReviewFindingProto, ReviewResultProto,
    SemanticConflict, SubmitError, SymbolChange, SymbolOverwrite, SymbolRef, SymbolResult,
    VerifyStepResult, WatchEvent,
};

/// A high-level representation of a code change that the SDK translates into
/// the proto `Change` message before sending to the server.
#[derive(Debug, Clone)]
pub enum Change {
    Add { path: String, content: String },
    Modify { path: String, content: String },
    Delete { path: String },
}

impl Change {
    /// Convenience constructor for an add change.
    pub fn add(path: impl Into<String>, content: impl Into<String>) -> Self {
        Change::Add {
            path: path.into(),
            content: content.into(),
        }
    }

    /// Convenience constructor for a modify change.
    pub fn modify(path: impl Into<String>, content: impl Into<String>) -> Self {
        Change::Modify {
            path: path.into(),
            content: content.into(),
        }
    }

    /// Convenience constructor for a delete change.
    pub fn delete(path: impl Into<String>) -> Self {
        Change::Delete { path: path.into() }
    }
}

/// Depth of context retrieval.
#[derive(Debug, Clone, Copy)]
pub enum Depth {
    Signatures,
    Full,
    CallGraph,
}

/// Filter for watch events.
#[derive(Debug, Clone)]
pub enum Filter {
    All,
    Symbols,
    Files,
}

/// Result of a successful CONNECT handshake.
#[derive(Debug)]
pub struct ConnectResult {
    pub session_id: String,
    pub changeset_id: String,
    pub codebase_version: String,
    pub summary: Option<CodebaseSummary>,
}

/// Result of a CONTEXT query.
#[derive(Debug)]
pub struct ContextResult {
    pub symbols: Vec<SymbolResult>,
    pub call_graph: Vec<CallEdgeRef>,
    pub dependencies: Vec<DependencyRef>,
    pub estimated_tokens: u32,
}

/// Result of a SUBMIT operation.
#[derive(Debug)]
pub struct SubmitResult {
    pub changeset_id: String,
    pub status: String,
    pub errors: Vec<SubmitError>,
}

/// Result of a MERGE operation.
#[derive(Debug)]
pub enum MergeResult {
    /// Merge succeeded — changeset is now a Git commit.
    Success(MergeSuccess),
    /// Merge blocked by conflicts — agent must resolve.
    Conflict(MergeConflict),
    /// Merge blocked by recent overwrite — agent must force or abort.
    OverwriteWarning(RecentOverwriteWarning),
}

/// Result of a FILE_READ operation.
#[derive(Debug)]
pub struct FileReadResult {
    pub content: String,
    pub hash: String,
    pub modified_in_session: bool,
}

/// Result of a FILE_WRITE operation.
#[derive(Debug)]
pub struct FileWriteResult {
    pub new_hash: String,
    pub detected_changes: Vec<SymbolChange>,
    pub conflict_warnings: Vec<ConflictWarning>,
}

/// Result of a FILE_LIST operation.
#[derive(Debug)]
pub struct FileListResult {
    pub files: Vec<FileEntry>,
}

/// Result of a GET_SESSION_STATUS operation.
#[derive(Debug)]
pub struct SessionStatusResult {
    pub session_id: String,
    pub base_commit: String,
    pub files_modified: Vec<String>,
    pub symbols_modified: Vec<String>,
    pub overlay_size_bytes: u64,
    pub active_other_sessions: u32,
}

/// Push destination mode.
#[derive(Debug, Clone, Copy)]
pub enum PushMode {
    Branch,
    Pr,
}

/// Conflict resolution strategy.
#[derive(Debug, Clone, Copy)]
pub enum ResolutionMode {
    Proceed,
    KeepYours,
    KeepTheirs,
    Manual,
}

/// Result of a PRE_SUBMIT_CHECK operation.
#[derive(Debug)]
pub struct PreSubmitResult {
    pub has_conflicts: bool,
    pub potential_conflicts: Vec<SemanticConflict>,
    pub files_modified: u32,
    pub symbols_changed: u32,
}

/// Result of a PUSH operation.
#[derive(Debug)]
pub struct PushResult {
    pub branch_name: String,
    pub pr_url: String,
    pub commit_hash: String,
    pub changeset_ids: Vec<String>,
}

/// Result of an APPROVE operation.
#[derive(Debug)]
pub struct ApproveResult {
    pub success: bool,
    pub changeset_id: String,
    pub new_state: String,
    pub message: String,
}

/// Result of a RESOLVE operation.
#[derive(Debug)]
pub struct ResolveResult {
    pub success: bool,
    pub changeset_id: String,
    pub new_state: String,
    pub message: String,
    pub conflicts_resolved: i32,
    pub conflicts_remaining: i32,
}

/// Result of a CLOSE operation.
#[derive(Debug)]
pub struct CloseResult {
    pub success: bool,
    pub message: String,
}

/// Result of a REVIEW query (list of AI reviews for the changeset).
#[derive(Debug)]
pub struct ReviewListResult {
    pub reviews: Vec<ReviewResultProto>,
}

/// Result of a RECORD_REVIEW operation.
#[derive(Debug)]
pub struct RecordReviewResult {
    pub review_id: String,
    pub accepted: bool,
}

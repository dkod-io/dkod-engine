// Re-export proto types that SDK consumers will use directly.
pub use dk_protocol::{
    CallEdgeRef, CodebaseSummary, ConflictInfo, DependencyRef, SubmitError, SymbolRef,
    SymbolResult, VerifyStepResult, WatchEvent,
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
pub struct MergeResult {
    pub commit_hash: String,
    pub merged_version: String,
    pub conflicts: Vec<ConflictInfo>,
}

use dk_core::types::{CallEdge, Dependency, Symbol};

use crate::findings::Finding;

/// A file that has changed in the current changeset.
#[derive(Debug, Clone)]
pub struct ChangedFile {
    /// Relative path within the repository.
    pub path: String,
    /// File content after the change (None if the file was deleted).
    pub content: Option<String>,
}

/// Contextual data gathered from the Engine's graph stores and the changeset,
/// providing both before (DB) and after (parsed) snapshots for comparison.
pub struct CheckContext {
    /// Symbols from the database for the changed files (before state).
    pub before_symbols: Vec<Symbol>,
    /// Symbols parsed from the materialized changeset files (after state).
    pub after_symbols: Vec<Symbol>,
    /// Call graph edges from the database (before state).
    pub before_call_graph: Vec<CallEdge>,
    /// Call graph edges derived from the parsed changeset (after state).
    pub after_call_graph: Vec<CallEdge>,
    /// Dependencies from the database (before state).
    pub before_deps: Vec<Dependency>,
    /// Dependencies from the changeset (after state — currently mirrors before).
    pub after_deps: Vec<Dependency>,
    /// The set of files that changed.
    pub changed_files: Vec<ChangedFile>,
}

/// Trait that every semantic check must implement.
///
/// Checks are stateless: all mutable context is supplied via `CheckContext`.
pub trait SemanticCheck: Send + Sync {
    /// A unique, kebab-case name for the check (e.g. "no-unsafe-added").
    fn name(&self) -> &str;

    /// Execute the check against the provided context and return any findings.
    fn run(&self, ctx: &CheckContext) -> Vec<Finding>;
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use dk_core::{
    CallEdge, Error, RawCallEdge, RepoId, Result, Symbol, SymbolId,
};
use sqlx::postgres::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::changeset::ChangesetStore;
use crate::git::GitRepository;
use crate::pipeline::PipelineStore;
use crate::graph::{
    CallGraphStore, DependencyStore, SearchIndex, SymbolStore, TypeInfoStore,
};
use crate::parser::ParserRegistry;
use crate::workspace::session_manager::WorkspaceManager;

// ── Public types ──

/// High-level summary of a repository's indexed codebase.
#[derive(Debug, Clone)]
pub struct CodebaseSummary {
    pub languages: Vec<String>,
    pub total_symbols: u64,
    pub total_files: u64,
}

/// The central orchestration layer that ties together Git storage,
/// language parsing, the semantic graph stores, and full-text search.
///
/// Internally concurrent: all methods take `&self`. The `SearchIndex` is
/// wrapped in an `RwLock` (write for mutations, read for queries), and
/// per-repo Git operations are serialised via `repo_locks`.
pub struct Engine {
    pub db: PgPool,
    pub search_index: Arc<RwLock<SearchIndex>>,
    pub parser: Arc<ParserRegistry>,
    pub storage_path: PathBuf,
    symbol_store: SymbolStore,
    call_graph_store: CallGraphStore,
    #[allow(dead_code)]
    dep_store: DependencyStore,
    type_info_store: TypeInfoStore,
    changeset_store: ChangesetStore,
    pipeline_store: PipelineStore,
    workspace_manager: WorkspaceManager,
    repo_locks: DashMap<RepoId, Arc<RwLock<()>>>,
}

impl Engine {
    /// Create a new Engine instance.
    ///
    /// Initialises all graph stores from the provided `PgPool`, creates the
    /// `ParserRegistry` with Rust/TypeScript/Python parsers, and opens (or
    /// creates) a Tantivy `SearchIndex` at `storage_path/search_index`.
    pub fn new(storage_path: PathBuf, db: PgPool) -> Result<Self> {
        let search_index = SearchIndex::open(&storage_path.join("search_index"))?;
        let parser = ParserRegistry::new();
        let symbol_store = SymbolStore::new(db.clone());
        let call_graph_store = CallGraphStore::new(db.clone());
        let dep_store = DependencyStore::new(db.clone());
        let type_info_store = TypeInfoStore::new(db.clone());
        let changeset_store = ChangesetStore::new(db.clone());
        let pipeline_store = PipelineStore::new(db.clone());
        let workspace_manager = WorkspaceManager::new(db.clone());

        Ok(Self {
            db,
            search_index: Arc::new(RwLock::new(search_index)),
            parser: Arc::new(parser),
            storage_path,
            symbol_store,
            call_graph_store,
            dep_store,
            type_info_store,
            changeset_store,
            pipeline_store,
            workspace_manager,
            repo_locks: DashMap::new(),
        })
    }

    /// Returns a reference to the symbol store for direct DB queries.
    pub fn symbol_store(&self) -> &SymbolStore {
        &self.symbol_store
    }

    /// Returns a reference to the changeset store.
    pub fn changeset_store(&self) -> &ChangesetStore {
        &self.changeset_store
    }

    /// Returns a reference to the pipeline store.
    pub fn pipeline_store(&self) -> &PipelineStore {
        &self.pipeline_store
    }

    /// Returns a reference to the workspace manager.
    pub fn workspace_manager(&self) -> &WorkspaceManager {
        &self.workspace_manager
    }

    /// Returns a reference to the call graph store for direct DB queries.
    pub fn call_graph_store(&self) -> &CallGraphStore {
        &self.call_graph_store
    }

    /// Returns a reference to the dependency store for direct DB queries.
    pub fn dep_store(&self) -> &DependencyStore {
        &self.dep_store
    }

    /// Returns a reference to the parser registry.
    pub fn parser(&self) -> &ParserRegistry {
        &self.parser
    }

    /// Returns a per-repo lock for serialising Git operations.
    ///
    /// Creates a new lock on first access for a given `repo_id`.
    pub fn repo_lock(&self, repo_id: RepoId) -> Arc<RwLock<()>> {
        self.repo_locks
            .entry(repo_id)
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    }

    /// Remove the repo lock entry for a deleted repo.
    pub fn remove_repo_lock(&self, repo_id: RepoId) {
        self.repo_locks.remove(&repo_id);
    }

    // ── Repository lifecycle ──

    /// Create a new repository.
    ///
    /// Generates a UUID, initialises a Git repository at
    /// `storage_path/repos/<uuid>`, inserts a row into the `repositories`
    /// table, and returns the new `RepoId`.
    pub async fn create_repo(&self, name: &str) -> Result<RepoId> {
        let repo_id = Uuid::new_v4();
        let repo_path = self.storage_path.join("repos").join(repo_id.to_string());

        GitRepository::init(&repo_path)?;

        sqlx::query(
            r#"
            INSERT INTO repositories (id, name, path)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(repo_id)
        .bind(name)
        .bind(repo_path.to_string_lossy().as_ref())
        .execute(&self.db)
        .await?;

        Ok(repo_id)
    }

    /// Look up a repository by name.
    ///
    /// Returns the `RepoId` and an opened `GitRepository` handle.
    pub async fn get_repo(&self, name: &str) -> Result<(RepoId, GitRepository)> {
        let row: (Uuid, String) = sqlx::query_as(
            "SELECT id, path FROM repositories WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.db)
        .await?
        .ok_or_else(|| Error::RepoNotFound(name.to_string()))?;

        let (repo_id, repo_path) = row;
        let git_repo = GitRepository::open(Path::new(&repo_path))?;
        Ok((repo_id, git_repo))
    }

    /// Look up a repository by its UUID.
    ///
    /// Returns the `RepoId` and an opened `GitRepository` handle.
    pub async fn get_repo_by_db_id(&self, repo_id: RepoId) -> Result<(RepoId, GitRepository)> {
        let row: (String,) = sqlx::query_as(
            "SELECT path FROM repositories WHERE id = $1",
        )
        .bind(repo_id)
        .fetch_optional(&self.db)
        .await?
        .ok_or_else(|| Error::RepoNotFound(repo_id.to_string()))?;

        let git_repo = GitRepository::open(Path::new(&row.0))?;
        Ok((repo_id, git_repo))
    }

    // ── Indexing ──

    /// Perform a full index of a repository.
    ///
    /// Walks the working directory (skipping `.git`), parses every file with a
    /// supported extension, and populates the symbol table, type info store,
    /// call graph, and full-text search index.
    pub async fn index_repo(
        &self,
        repo_id: RepoId,
        git_repo: &GitRepository,
    ) -> Result<()> {
        let root = git_repo.path().to_path_buf();
        let files = collect_files(&root, &self.parser);

        // Accumulate all symbols across every file so we can resolve call
        // edges at the end.
        let mut all_symbols: Vec<Symbol> = Vec::new();
        let mut all_raw_edges: Vec<RawCallEdge> = Vec::new();

        // Acquire the search index write lock for the duration of indexing.
        let mut search_index = self.search_index.write().await;

        for file_path in &files {
            let relative = file_path
                .strip_prefix(&root)
                .unwrap_or(file_path);

            let source = std::fs::read(file_path).map_err(|e| {
                Error::Io(e)
            })?;

            let analysis = self.parser.parse_file(relative, &source)?;

            // Symbols
            for sym in &analysis.symbols {
                self.symbol_store.upsert_symbol(repo_id, sym).await?;
                search_index.index_symbol(repo_id, sym)?;
            }

            // Type info
            for ti in &analysis.types {
                self.type_info_store.upsert_type_info(ti).await?;
            }

            all_symbols.extend(analysis.symbols);
            all_raw_edges.extend(analysis.calls);
        }

        // Commit search index once after all files.
        search_index.commit()?;
        drop(search_index);

        // Resolve and insert call edges.
        let edges = resolve_call_edges(&all_raw_edges, &all_symbols, repo_id);
        for edge in &edges {
            self.call_graph_store.insert_edge(edge).await?;
        }

        Ok(())
    }

    /// Incrementally re-index a set of changed files.
    ///
    /// For each path: deletes old symbols and call edges, re-parses, and
    /// upserts the new data.
    pub async fn update_files(
        &self,
        repo_id: RepoId,
        git_repo: &GitRepository,
        changed_files: &[PathBuf],
    ) -> Result<()> {
        self.update_files_by_root(repo_id, git_repo.path(), changed_files)
            .await
    }

    /// Incrementally re-index a set of changed files, given the repository
    /// root path directly.
    ///
    /// This variant avoids holding a `GitRepository` reference (which is
    /// `!Sync`) across `.await` points, making the resulting future `Send`.
    pub async fn update_files_by_root(
        &self,
        repo_id: RepoId,
        root: &Path,
        changed_files: &[PathBuf],
    ) -> Result<()> {
        let root = root.to_path_buf();

        let mut all_symbols: Vec<Symbol> = Vec::new();
        let mut all_raw_edges: Vec<RawCallEdge> = Vec::new();

        // Acquire the search index write lock for the duration of re-indexing.
        let mut search_index = self.search_index.write().await;

        for file_path in changed_files {
            let relative = file_path
                .strip_prefix(&root)
                .unwrap_or(file_path);
            let rel_str = relative.to_string_lossy().to_string();

            // Fetch existing symbols for this file so we can remove their
            // search index entries.
            let old_symbols = self
                .symbol_store
                .find_by_file(repo_id, &rel_str)
                .await?;
            for old_sym in &old_symbols {
                search_index.remove_symbol(old_sym.id)?;
            }

            // Delete old DB rows.
            self.call_graph_store
                .delete_edges_for_file(repo_id, &rel_str)
                .await?;
            self.symbol_store
                .delete_by_file(repo_id, &rel_str)
                .await?;

            // Re-parse.
            let full_path = root.join(relative);
            if !full_path.exists() {
                // File was deleted; nothing more to do for this path.
                continue;
            }

            if !self.parser.supports_file(relative) {
                continue;
            }

            let source = std::fs::read(&full_path)?;
            let analysis = self.parser.parse_file(relative, &source)?;

            for sym in &analysis.symbols {
                self.symbol_store.upsert_symbol(repo_id, sym).await?;
                search_index.index_symbol(repo_id, sym)?;
            }

            for ti in &analysis.types {
                self.type_info_store.upsert_type_info(ti).await?;
            }

            all_symbols.extend(analysis.symbols);
            all_raw_edges.extend(analysis.calls);
        }

        search_index.commit()?;
        drop(search_index);

        let edges = resolve_call_edges(&all_raw_edges, &all_symbols, repo_id);
        for edge in &edges {
            self.call_graph_store.insert_edge(edge).await?;
        }

        Ok(())
    }

    // ── Querying ──

    /// Search for symbols matching a free-text query.
    ///
    /// Uses Tantivy full-text search to find candidate `SymbolId`s, then
    /// fetches the full `Symbol` objects from the database.
    pub async fn query_symbols(
        &self,
        repo_id: RepoId,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<Symbol>> {
        let search_index = self.search_index.read().await;
        let ids = search_index.search(repo_id, query, max_results)?;
        drop(search_index);

        self.symbol_store.get_by_ids(&ids).await
    }

    /// Retrieve the call graph neighbourhood of a symbol.
    ///
    /// Returns `(callers, callees)` — the full `Symbol` objects for every
    /// direct caller and every direct callee.
    pub async fn get_call_graph(
        &self,
        _repo_id: RepoId,
        symbol_id: SymbolId,
    ) -> Result<(Vec<Symbol>, Vec<Symbol>)> {
        let caller_edges = self.call_graph_store.find_callers(symbol_id).await?;
        let callee_edges = self.call_graph_store.find_callees(symbol_id).await?;

        let mut callers = Vec::with_capacity(caller_edges.len());
        for edge in &caller_edges {
            if let Some(sym) = self.symbol_store.get_by_id(edge.caller).await? {
                callers.push(sym);
            }
        }

        let mut callees = Vec::with_capacity(callee_edges.len());
        for edge in &callee_edges {
            if let Some(sym) = self.symbol_store.get_by_id(edge.callee).await? {
                callees.push(sym);
            }
        }

        Ok((callers, callees))
    }

    /// Produce a high-level summary of the indexed codebase.
    ///
    /// Queries the symbols table for distinct file extensions (→ languages),
    /// distinct file paths (→ total_files), and total row count
    /// (→ total_symbols).
    pub async fn codebase_summary(&self, repo_id: RepoId) -> Result<CodebaseSummary> {
        let total_symbols = self.symbol_store.count(repo_id).await? as u64;

        // Count distinct files and collect unique extensions in a single query.
        let row: (i64, Vec<String>) = sqlx::query_as(
            r#"
            SELECT
                COUNT(DISTINCT file_path),
                COALESCE(
                    array_agg(DISTINCT substring(file_path FROM '\.([^.]+)$'))
                        FILTER (WHERE substring(file_path FROM '\.([^.]+)$') IS NOT NULL),
                    ARRAY[]::text[]
                )
            FROM symbols
            WHERE repo_id = $1
            "#,
        )
        .bind(repo_id)
        .fetch_one(&self.db)
        .await?;

        let total_files = row.0 as u64;
        let mut languages = row.1;
        languages.sort();

        Ok(CodebaseSummary {
            languages,
            total_symbols,
            total_files,
        })
    }
}

// ── Helpers ──

/// Recursively collect all files under `root` that are supported by the
/// parser registry, skipping the `.git` directory.
fn collect_files(root: &Path, parser: &ParserRegistry) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_recursive(root, root, parser, &mut files);
    files
}

fn collect_files_recursive(
    root: &Path,
    dir: &Path,
    parser: &ParserRegistry,
    out: &mut Vec<PathBuf>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // Skip .git and hidden directories.
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == ".git" || name.starts_with('.') {
                    continue;
                }
            }
            collect_files_recursive(root, &path, parser, out);
        } else if path.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if parser.supports_file(relative) {
                out.push(path);
            }
        }
    }
}

/// Resolve `RawCallEdge`s (which use string names) into `CallEdge`s
/// (which use `SymbolId`s) by building a name-to-id lookup table.
fn resolve_call_edges(
    raw_edges: &[RawCallEdge],
    symbols: &[Symbol],
    repo_id: RepoId,
) -> Vec<CallEdge> {
    // Build name -> SymbolId lookup.
    // Insert both `name` and `qualified_name` so either form resolves.
    let mut name_to_id: HashMap<String, SymbolId> = HashMap::new();
    for sym in symbols {
        name_to_id.insert(sym.name.clone(), sym.id);
        name_to_id.insert(sym.qualified_name.clone(), sym.id);
    }

    raw_edges
        .iter()
        .filter_map(|raw| {
            let caller = name_to_id.get(&raw.caller_name)?;
            let callee = name_to_id.get(&raw.callee_name)?;
            Some(CallEdge {
                id: Uuid::new_v4(),
                repo_id,
                caller: *caller,
                callee: *callee,
                kind: raw.kind.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_call_edges_basic() {
        let sym_a_id = Uuid::new_v4();
        let sym_b_id = Uuid::new_v4();
        let repo_id = Uuid::new_v4();

        let symbols = vec![
            Symbol {
                id: sym_a_id,
                name: "foo".into(),
                qualified_name: "crate::foo".into(),
                kind: dk_core::SymbolKind::Function,
                visibility: dk_core::Visibility::Public,
                file_path: "src/lib.rs".into(),
                span: dk_core::Span {
                    start_byte: 0,
                    end_byte: 100,
                },
                signature: None,
                doc_comment: None,
                parent: None,
                last_modified_by: None,
                last_modified_intent: None,
            },
            Symbol {
                id: sym_b_id,
                name: "bar".into(),
                qualified_name: "crate::bar".into(),
                kind: dk_core::SymbolKind::Function,
                visibility: dk_core::Visibility::Public,
                file_path: "src/lib.rs".into(),
                span: dk_core::Span {
                    start_byte: 100,
                    end_byte: 200,
                },
                signature: None,
                doc_comment: None,
                parent: None,
                last_modified_by: None,
                last_modified_intent: None,
            },
        ];

        let raw_edges = vec![RawCallEdge {
            caller_name: "foo".into(),
            callee_name: "bar".into(),
            call_site: dk_core::Span {
                start_byte: 50,
                end_byte: 60,
            },
            kind: dk_core::CallKind::DirectCall,
        }];

        let edges = resolve_call_edges(&raw_edges, &symbols, repo_id);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].caller, sym_a_id);
        assert_eq!(edges[0].callee, sym_b_id);
        assert_eq!(edges[0].repo_id, repo_id);
    }

    #[test]
    fn test_resolve_call_edges_unresolved_skipped() {
        let sym_a_id = Uuid::new_v4();
        let repo_id = Uuid::new_v4();

        let symbols = vec![Symbol {
            id: sym_a_id,
            name: "foo".into(),
            qualified_name: "crate::foo".into(),
            kind: dk_core::SymbolKind::Function,
            visibility: dk_core::Visibility::Public,
            file_path: "src/lib.rs".into(),
            span: dk_core::Span {
                start_byte: 0,
                end_byte: 100,
            },
            signature: None,
            doc_comment: None,
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        }];

        // callee "unknown" doesn't exist in symbols
        let raw_edges = vec![RawCallEdge {
            caller_name: "foo".into(),
            callee_name: "unknown".into(),
            call_site: dk_core::Span {
                start_byte: 50,
                end_byte: 60,
            },
            kind: dk_core::CallKind::DirectCall,
        }];

        let edges = resolve_call_edges(&raw_edges, &symbols, repo_id);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_resolve_call_edges_qualified_name() {
        let sym_a_id = Uuid::new_v4();
        let sym_b_id = Uuid::new_v4();
        let repo_id = Uuid::new_v4();

        let symbols = vec![
            Symbol {
                id: sym_a_id,
                name: "foo".into(),
                qualified_name: "crate::mod_a::foo".into(),
                kind: dk_core::SymbolKind::Function,
                visibility: dk_core::Visibility::Public,
                file_path: "src/mod_a.rs".into(),
                span: dk_core::Span {
                    start_byte: 0,
                    end_byte: 100,
                },
                signature: None,
                doc_comment: None,
                parent: None,
                last_modified_by: None,
                last_modified_intent: None,
            },
            Symbol {
                id: sym_b_id,
                name: "bar".into(),
                qualified_name: "crate::mod_b::bar".into(),
                kind: dk_core::SymbolKind::Function,
                visibility: dk_core::Visibility::Public,
                file_path: "src/mod_b.rs".into(),
                span: dk_core::Span {
                    start_byte: 0,
                    end_byte: 100,
                },
                signature: None,
                doc_comment: None,
                parent: None,
                last_modified_by: None,
                last_modified_intent: None,
            },
        ];

        // Use qualified names for resolution
        let raw_edges = vec![RawCallEdge {
            caller_name: "crate::mod_a::foo".into(),
            callee_name: "crate::mod_b::bar".into(),
            call_site: dk_core::Span {
                start_byte: 50,
                end_byte: 60,
            },
            kind: dk_core::CallKind::DirectCall,
        }];

        let edges = resolve_call_edges(&raw_edges, &symbols, repo_id);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].caller, sym_a_id);
        assert_eq!(edges[0].callee, sym_b_id);
    }

    #[test]
    fn test_collect_files_skips_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create a .git directory with a file inside.
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), b"git config").unwrap();

        // Create a supported source file.
        std::fs::write(root.join("main.rs"), b"fn main() {}").unwrap();

        // Create an unsupported file.
        std::fs::write(root.join("notes.txt"), b"hello").unwrap();

        let parser = ParserRegistry::new();
        let files = collect_files(root, &parser);

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
    }

    #[test]
    fn test_codebase_summary_struct() {
        let summary = CodebaseSummary {
            languages: vec!["rs".into(), "py".into()],
            total_symbols: 42,
            total_files: 5,
        };
        assert_eq!(summary.languages.len(), 2);
        assert_eq!(summary.total_symbols, 42);
        assert_eq!(summary.total_files, 5);
    }
}

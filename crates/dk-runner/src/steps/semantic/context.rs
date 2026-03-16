use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use uuid::Uuid;

use dk_core::types::{CallEdge, Symbol};
use dk_engine::repo::Engine;

use super::checks::{ChangedFile, CheckContext};

/// Build a `CheckContext` by querying the Engine's graph stores for the
/// "before" snapshot and parsing the materialized changeset files for the
/// "after" snapshot.
///
/// # Arguments
///
/// * `engine` — the dk-engine orchestrator (symbol store, call graph, etc.)
/// * `repo_id` — the repository UUID
/// * `changeset_files` — relative paths of changed files
/// * `work_dir` — the directory where changeset files have been materialized
pub async fn build_check_context(
    engine: &Arc<Engine>,
    repo_id: Uuid,
    changeset_files: &[String],
    work_dir: &Path,
) -> Result<CheckContext> {
    let mut before_symbols: Vec<Symbol> = Vec::new();
    let mut after_symbols: Vec<Symbol> = Vec::new();
    let mut before_call_graph: Vec<CallEdge> = Vec::new();
    let mut changed_files: Vec<ChangedFile> = Vec::new();

    for file_path in changeset_files {
        // ── Before state: query symbols from DB ──
        let file_symbols = engine
            .symbol_store()
            .find_by_file(repo_id, file_path)
            .await
            .unwrap_or_default();

        // Gather call graph edges for every before-symbol.
        for sym in &file_symbols {
            let callee_edges = engine
                .call_graph_store()
                .find_callees(sym.id)
                .await
                .unwrap_or_default();
            before_call_graph.extend(callee_edges);

            let caller_edges = engine
                .call_graph_store()
                .find_callers(sym.id)
                .await
                .unwrap_or_default();
            before_call_graph.extend(caller_edges);
        }

        before_symbols.extend(file_symbols);

        // ── After state: parse the materialized file ──
        let abs_path = work_dir.join(file_path);
        let rel_path = Path::new(file_path);

        if abs_path.exists() {
            let bytes = tokio::fs::read(&abs_path).await?;
            let content = String::from_utf8_lossy(&bytes).to_string();

            // Only parse files the parser supports.
            if engine.parser().supports_file(rel_path) {
                match engine.parser().parse_file(rel_path, &bytes) {
                    Ok(analysis) => {
                        after_symbols.extend(analysis.symbols);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse {}: {e}", file_path);
                    }
                }
            }

            changed_files.push(ChangedFile {
                path: file_path.clone(),
                content: Some(content),
            });
        } else {
            // File was deleted.
            changed_files.push(ChangedFile {
                path: file_path.clone(),
                content: None,
            });
        }
    }

    // ── Dependencies (repo-level, before state) ──
    let before_deps = engine
        .dep_store()
        .find_by_repo(repo_id)
        .await
        .unwrap_or_default();

    // For now the after-deps mirror the before-deps since we don't parse
    // manifest files from the changeset yet.
    let after_deps = before_deps.clone();

    // After call graph: we don't yet persist parsed call edges back, so use
    // an empty vec. The quality checks that need it will work on
    // before_call_graph (the full DB state).
    let after_call_graph = Vec::new();

    Ok(CheckContext {
        before_symbols,
        after_symbols,
        before_call_graph,
        after_call_graph,
        before_deps,
        after_deps,
        changed_files,
    })
}

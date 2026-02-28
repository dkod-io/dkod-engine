use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::{
    CallEdgeRef, ContextDepth, ContextRequest, ContextResponse, SymbolRef, SymbolResult,
};

/// Handle a CONTEXT RPC.
///
/// 1. Validates the session (and keeps it alive).
/// 2. Runs a full-text search for the given query.
/// 3. Depending on depth:
///    - `SIGNATURES` -- return symbol metadata only.
///    - `FULL`       -- also include source code.
///    - `CALL_GRAPH` -- include source code + caller/callee edges.
/// 4. Estimates token usage and truncates if `max_tokens` is set.
///
/// File reads now go through the session workspace overlay, so agents
/// see their own in-progress modifications reflected in CONTEXT results.
pub async fn handle_context(
    server: &ProtocolServer,
    req: ContextRequest,
) -> Result<Response<ContextResponse>, Status> {
    // 1. Validate session
    let session = server.validate_session(&req.session_id)?;

    // Touch session to keep it alive
    let sid = req
        .session_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid session ID"))?;
    server.session_mgr().touch_session(&sid);

    // 2-3. Query symbols and build results.
    //      `GitRepository` is !Sync, so all usage must stay within a single
    //      block that does not hold it across `.await` points.
    let depth = req.depth();
    let include_source = depth == ContextDepth::Full || depth == ContextDepth::CallGraph;
    let include_call_graph = depth == ContextDepth::CallGraph;

    let max_results = if req.max_tokens > 0 {
        ((req.max_tokens / 100) as usize).max(10)
    } else {
        50
    };

    let engine = server.engine();

    let (symbol_results, call_edges) = {
        let (repo_id, git_repo) = engine
            .get_repo(&session.codebase)
            .await
            .map_err(|e| Status::internal(format!("Repo error: {e}")))?;

        let symbols = engine
            .query_symbols(repo_id, &req.query, max_results)
            .await
            .map_err(|e| Status::internal(format!("Query error: {e}")))?;

        // Try to get the workspace for session-aware reads.
        // If no workspace exists (shouldn't happen after CONNECT, but be defensive),
        // fall through to direct file reads.
        let maybe_ws = engine.workspace_manager().get_workspace(&sid);

        let mut results = Vec::with_capacity(symbols.len());
        let mut edges = Vec::new();

        for sym in &symbols {
            let mut result = SymbolResult {
                symbol: Some(symbol_to_ref(sym)),
                source: None,
                caller_ids: vec![],
                callee_ids: vec![],
                test_symbol_ids: vec![],
            };

            // FULL / CALL_GRAPH depth: include source code (symbol span only)
            if include_source {
                let source_bytes = if let Some(ref ws) = maybe_ws {
                    // Read through workspace overlay (sees session modifications)
                    ws.read_file(
                        &sym.file_path.to_string_lossy(),
                        &git_repo,
                    )
                    .ok()
                    .map(|r| r.content)
                } else {
                    // Fallback: read directly from working directory
                    let file_path = git_repo.path().join(&sym.file_path);
                    std::fs::read(&file_path).ok()
                };

                if let Some(source) = source_bytes {
                    let start = sym.span.start_byte as usize;
                    let end = sym.span.end_byte as usize;
                    if end <= source.len() {
                        result.source = Some(
                            String::from_utf8_lossy(&source[start..end]).to_string(),
                        );
                    }
                }
            }

            // CALL_GRAPH depth: include callers/callees
            if include_call_graph {
                // Drop git_repo borrow concern: get_call_graph doesn't need it
                if let Ok((callers, callees)) = engine.get_call_graph(repo_id, sym.id).await {
                    result.caller_ids = callers.iter().map(|s| s.id.to_string()).collect();
                    result.callee_ids = callees.iter().map(|s| s.id.to_string()).collect();

                    for caller in &callers {
                        edges.push(CallEdgeRef {
                            caller_id: caller.id.to_string(),
                            callee_id: sym.id.to_string(),
                            kind: "direct_call".to_string(),
                        });
                    }
                }
            }

            results.push(result);
        }

        (results, edges)
    };

    // 4. Estimate tokens (~4 chars per token, rough heuristic)
    let total_chars: usize = symbol_results
        .iter()
        .map(|r| {
            let sym_size = r
                .symbol
                .as_ref()
                .map(|s| s.name.len() + s.signature.len())
                .unwrap_or(0);
            let source_size = r.source.as_ref().map(|s| s.len()).unwrap_or(0);
            sym_size + source_size
        })
        .sum();
    let mut estimated_tokens = (total_chars / 4) as u32;

    // 5. Truncate if max_tokens is set
    let mut symbol_results = symbol_results;
    if req.max_tokens > 0 && estimated_tokens > req.max_tokens {
        let mut remaining = req.max_tokens;

        for result in &mut symbol_results {
            let sym_tokens = result
                .symbol
                .as_ref()
                .map(|s| ((s.name.len() + s.signature.len()) / 4) as u32)
                .unwrap_or(0);

            if remaining < sym_tokens {
                // Can't fit even the symbol header -- drop source.
                result.source = None;
                continue;
            }
            remaining -= sym_tokens;

            if let Some(ref source) = result.source {
                let source_tokens = (source.len() / 4) as u32;
                if remaining < source_tokens {
                    let max_chars = (remaining as usize) * 4;
                    result.source = Some(source[..max_chars.min(source.len())].to_string());
                    remaining = 0;
                } else {
                    remaining -= source_tokens;
                }
            }
        }

        estimated_tokens = req.max_tokens - remaining;
    }

    info!(
        session_id = %req.session_id,
        query = %req.query,
        results = symbol_results.len(),
        estimated_tokens,
        "CONTEXT: query served"
    );

    Ok(Response::new(ContextResponse {
        symbols: symbol_results,
        call_graph: call_edges,
        dependencies: vec![], // TODO: populate when include_dependencies is set
        estimated_tokens,
    }))
}

/// Convert a `dk_core::Symbol` into the protobuf `SymbolRef` message.
fn symbol_to_ref(sym: &dk_core::Symbol) -> SymbolRef {
    SymbolRef {
        id: sym.id.to_string(),
        name: sym.name.clone(),
        qualified_name: sym.qualified_name.clone(),
        kind: sym.kind.to_string(),
        visibility: format!("{:?}", sym.visibility),
        file_path: sym.file_path.to_string_lossy().to_string(),
        start_byte: sym.span.start_byte,
        end_byte: sym.span.end_byte,
        signature: sym.signature.clone().unwrap_or_default(),
        doc_comment: sym.doc_comment.clone(),
        parent_id: sym.parent.map(|p| p.to_string()),
    }
}

//! Programmatic Tool Calling interface for the dkod Agent Protocol.
//!
//! Provides tool definitions compatible with Anthropic's `allowed_callers`
//! mechanism. These definitions can be passed directly to the Messages API
//! `tools=` parameter, or loaded from the generated `dkod-tools.json` manifest.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::Result;
use crate::session::Session;
use crate::types::{Change, ContextResult, Depth};

const ALLOWED_CALLER: &str = "code_execution_20260120";

/// A single tool definition in Anthropic's tool format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub allowed_callers: Vec<String>,
}

/// Returns all 6 dkod tool definitions for programmatic calling.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "dkod_connect".into(),
            description: concat!(
                "Establish an isolated session workspace on a dkod repository. ",
                "Returns a session_id, base_commit hash, codebase summary ",
                "(languages, modules, symbol count), and count of other active ",
                "sessions. The session workspace is automatically isolated — ",
                "changes made in this session are invisible to other sessions ",
                "until merged. Response is JSON: {session_id, base_commit, ",
                "codebase_summary: {languages, total_symbols, total_files}, ",
                "active_sessions}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "codebase": {
                        "type": "string",
                        "description": "Repository identifier: 'org/repo'"
                    },
                    "intent": {
                        "type": "string",
                        "description": "What this agent session intends to accomplish"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["ephemeral", "persistent"],
                        "description": "Ephemeral (default): auto-cleanup on disconnect. Persistent: survives disconnect for later resume."
                    }
                },
                "required": ["codebase", "intent"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_context".into(),
            description: concat!(
                "Query semantic context from the codebase. Returns symbols ",
                "(functions, classes, types) matching the query, with signatures, ",
                "file locations, call graph edges, and associated tests. ",
                "Response is JSON: {symbols: [{name, qualified_name, kind, ",
                "file_path, signature, source, callers, callees}], token_count, ",
                "freshness}. Results reflect this session's workspace (including ",
                "uncommitted local changes)."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string"
                    },
                    "query": {
                        "type": "string",
                        "description": "Natural language or structured query: 'All functions that handle user authentication' or 'symbol:authenticate_user'"
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["signatures", "full", "call_graph"],
                        "description": "signatures: names + types only. full: complete source. call_graph: signatures + caller/callee edges."
                    },
                    "include_tests": {
                        "type": "boolean"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Cap response size in tokens"
                    }
                },
                "required": ["session_id", "query"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_read_file".into(),
            description: concat!(
                "Read a file from this session's workspace. Returns the session's ",
                "view: if the file was modified in this session, returns the ",
                "modified version; otherwise returns the base version. Response is ",
                "JSON: {content, hash, modified_in_session}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["session_id", "path"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_write_file".into(),
            description: concat!(
                "Write a file to this session's workspace overlay. The change is ",
                "only visible to this session until submitted. Response is JSON: ",
                "{new_hash, detected_changes: [{symbol_name, change_type}]}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["session_id", "path", "content"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_submit".into(),
            description: concat!(
                "Submit this session's changes as a semantic changeset for ",
                "verification and merge. The platform auto-rebases onto current ",
                "HEAD if the base moved. Response is JSON with one of: ",
                "{status: 'accepted', version, changeset_id} or ",
                "{status: 'verification_failed', failures: [{gate, test_name, ",
                "error, suggestion}]} or {status: 'conflict', conflicts: [{file, ",
                "symbol, our_change, their_change}]} or {status: 'pending_review', ",
                "changeset_id}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "intent": {
                        "type": "string",
                        "description": "What this changeset accomplishes"
                    },
                    "verify": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Verification gates to run: 'typecheck', 'affected_tests', 'all_tests', 'lint', 'invariants'"
                    }
                },
                "required": ["session_id", "intent"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_session_status".into(),
            description: concat!(
                "Get the current state of this session's workspace. Response is ",
                "JSON: {session_id, base_commit, files_modified, symbols_modified, ",
                "overlay_size_bytes, active_other_sessions}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_list_files".into(),
            description: concat!(
                "List files in this session's workspace, optionally filtered by ",
                "path prefix. Modified files are tagged. Response is JSON: ",
                "{files: [{path, modified_in_session}], total}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "prefix": {
                        "type": "string",
                        "description": "Optional path prefix filter (e.g. 'src/')"
                    }
                },
                "required": ["session_id"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_verify".into(),
            description: concat!(
                "Run verification pipeline (lint, test, type-check) on the ",
                "session's changeset. Returns step-by-step results. Response is ",
                "JSON: {changeset_id, passed, steps: [{step_name, status, output, required}]}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
        ToolDefinition {
            name: "dkod_merge".into(),
            description: concat!(
                "Merge the verified changeset into a Git commit. Session is ",
                "cleared on success. Changeset must be in 'approved' state. ",
                "Response is JSON: {commit_hash, merged_version, auto_rebased, ",
                "auto_rebased_files, conflicts}."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "message": {
                        "type": "string",
                        "description": "Optional commit message (auto-generated if omitted)"
                    }
                },
                "required": ["session_id"]
            }),
            allowed_callers: vec![ALLOWED_CALLER.into()],
        },
    ]
}

/// Execute a tool call against *session* and return a JSON string result.
///
/// Resolves legacy aliases (`search_codebase` → `dkod_context`,
/// `submit_changes` → `dkod_submit`) before dispatch.
pub async fn dispatch_tool(session: &mut Session, name: &str, input: &Value) -> Result<String> {
    let name = match name {
        "search_codebase" => "dkod_context",
        "submit_changes" => "dkod_submit",
        other => other,
    };

    let get_str = |key: &str| -> String {
        input.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
    };

    match name {
        "dkod_context" => {
            let query = get_str("query");
            let depth = match input.get("depth").and_then(|v| v.as_str()).unwrap_or("full") {
                "signatures" => Depth::Signatures,
                "call_graph" => Depth::CallGraph,
                _ => Depth::Full,
            };
            let max_tokens = input.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(8000) as u32;
            let result: ContextResult = session.context(&query, depth, max_tokens).await?;
            Ok(serde_json::to_string(&json!({
                "symbols": result.symbols.len(),
                "estimated_tokens": result.estimated_tokens,
            })).unwrap())
        }

        "dkod_submit" => {
            let intent = get_str("intent");
            let changes_json = input.get("changes").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let changes: Vec<Change> = changes_json
                .iter()
                .filter_map(|c| {
                    let path = c.get("path")?.as_str()?.to_string();
                    let content = c.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    match c.get("type").and_then(|v| v.as_str()).unwrap_or("modify") {
                        "add" => Some(Change::add(path, content)),
                        "delete" => Some(Change::delete(path)),
                        _ => Some(Change::modify(path, content)),
                    }
                })
                .collect();
            let result = session.submit(changes, &intent).await?;
            Ok(serde_json::to_string(&json!({
                "status": result.status,
                "changeset_id": result.changeset_id,
            })).unwrap())
        }

        "dkod_read_file" => {
            let path = get_str("path");
            let result = session.file_read(&path).await?;
            Ok(serde_json::to_string(&json!({
                "content": result.content,
                "hash": result.hash,
                "modified_in_session": result.modified_in_session,
            })).unwrap())
        }

        "dkod_write_file" => {
            let path = get_str("path");
            let content = get_str("content");
            let result = session.file_write(&path, &content).await?;
            Ok(serde_json::to_string(&json!({
                "new_hash": result.new_hash,
                "detected_changes": result.detected_changes.len(),
            })).unwrap())
        }

        "dkod_session_status" => {
            let result = session.session_status().await?;
            Ok(serde_json::to_string(&json!({
                "session_id": result.session_id,
                "base_commit": result.base_commit,
                "files_modified": result.files_modified,
                "symbols_modified": result.symbols_modified,
                "overlay_size_bytes": result.overlay_size_bytes,
                "active_other_sessions": result.active_other_sessions,
            })).unwrap())
        }

        "dkod_list_files" => {
            let prefix = input.get("prefix").and_then(|v| v.as_str());
            let result = session.file_list(prefix).await?;
            let files: Vec<_> = result.files.iter().map(|f| json!({
                "path": f.path,
                "modified_in_session": f.modified_in_session,
            })).collect();
            Ok(serde_json::to_string(&json!({
                "files": files,
                "total": files.len(),
            })).unwrap())
        }

        "dkod_verify" => {
            let steps = session.verify().await?;
            let passed = steps.iter().all(|s| s.status == "passed" || !s.required);
            let step_results: Vec<_> = steps.iter().map(|s| json!({
                "step_name": s.step_name,
                "status": s.status,
                "output": s.output,
                "required": s.required,
            })).collect();
            Ok(serde_json::to_string(&json!({
                "changeset_id": session.changeset_id,
                "passed": passed,
                "steps": step_results,
            })).unwrap())
        }

        "dkod_merge" => {
            let message = get_str("message");
            let result = session.merge(&message, false).await?;
            use crate::types::MergeResult;
            let json_result = match result {
                MergeResult::Success(s) => json!({
                    "status": "success",
                    "commit_hash": s.commit_hash,
                    "merged_version": s.merged_version,
                    "auto_rebased": s.auto_rebased,
                    "auto_rebased_files": s.auto_rebased_files,
                }),
                MergeResult::Conflict(c) => json!({
                    "status": "conflict",
                    "changeset_id": c.changeset_id,
                    "suggested_action": c.suggested_action,
                }),
                MergeResult::OverwriteWarning(w) => json!({
                    "status": "overwrite_warning",
                    "changeset_id": w.changeset_id,
                }),
            };
            Ok(serde_json::to_string(&json_result).unwrap())
        }

        "dkod_connect" => Ok(serde_json::to_string(&json!({
            "session_id": session.session_id,
            "status": "already_connected",
        })).unwrap()),

        other => Err(crate::error::SdkError::Connection(format!("Unknown tool: {other}"))),
    }
}

/// Serialize all tool definitions to a JSON string (for dkod-tools.json).
pub fn generate_manifest() -> String {
    serde_json::to_string_pretty(&tool_definitions()).expect("tool definitions are valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definitions_count() {
        assert_eq!(tool_definitions().len(), 9);
    }

    #[test]
    fn test_all_tools_have_allowed_callers() {
        for tool in tool_definitions() {
            assert_eq!(tool.allowed_callers, vec!["code_execution_20260120"]);
        }
    }

    #[test]
    fn test_manifest_is_valid_json() {
        let manifest = generate_manifest();
        let parsed: Vec<ToolDefinition> = serde_json::from_str(&manifest).unwrap();
        assert_eq!(parsed.len(), 9);
    }

    #[test]
    fn test_tool_names() {
        let names: Vec<String> = tool_definitions().iter().map(|t| t.name.clone()).collect();
        assert!(names.contains(&"dkod_connect".to_string()));
        assert!(names.contains(&"dkod_context".to_string()));
        assert!(names.contains(&"dkod_read_file".to_string()));
        assert!(names.contains(&"dkod_write_file".to_string()));
        assert!(names.contains(&"dkod_submit".to_string()));
        assert!(names.contains(&"dkod_session_status".to_string()));
    }

    #[test]
    fn test_required_fields_present() {
        for tool in tool_definitions() {
            let schema = &tool.input_schema;
            assert!(schema.get("required").is_some(),
                "tool {} must have required fields", tool.name);
            assert!(schema.get("properties").is_some(),
                "tool {} must have properties", tool.name);
        }
    }

    #[test]
    fn generate_manifest_file() {
        let manifest = generate_manifest();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()  // crates/
            .parent().unwrap()  // repo root
            .join("sdk/dkod-tools.json");
        // Create sdk dir if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, &manifest).unwrap();
        println!("Manifest written to {}", path.display());
    }
}

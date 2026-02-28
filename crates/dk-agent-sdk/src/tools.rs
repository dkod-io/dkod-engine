//! Programmatic Tool Calling interface for the dkod Agent Protocol.
//!
//! Provides tool definitions compatible with Anthropic's `allowed_callers`
//! mechanism. These definitions can be passed directly to the Messages API
//! `tools=` parameter, or loaded from the generated `dkod-tools.json` manifest.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
    ]
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
        assert_eq!(tool_definitions().len(), 6);
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
        assert_eq!(parsed.len(), 6);
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

//! Tool-use variant of the Claude review provider.
//!
//! Unlike [`super::claude::ClaudeReviewProvider`] (which sends a single
//! prompt → receives a single JSON verdict), this provider exposes three
//! read-only tools to the model so it can *pull* additional context before
//! rendering a verdict:
//!
//!   - `read_file`         — fetch the current contents of a file in the workspace
//!   - `search_symbols`    — semantic symbol search (ContextDepth::SIGNATURES)
//!   - `get_call_graph`    — call-graph edges around a symbol
//!
//! The tools are implemented by a pluggable [`ReviewToolBackend`]. In the
//! dk-mcp / dk-server wiring path the backend is backed by the same gRPC
//! `AgentService` the user's agent already talks to; in tests and for
//! standalone usage a simple "in-memory" backend is provided that reads from
//! the `ReviewRequest::context` files and returns empty search/call-graph
//! responses.
//!
//! The tool-use loop is bounded by `max_tool_iterations` (default: 8). The
//! final verdict is parsed by the same [`super::parse::parse_review_response`]
//! helper used by [`super::claude::ClaudeReviewProvider`].

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::parse::parse_review_response;
use super::prompt::build_review_prompt;
use super::provider::{ReviewProvider, ReviewRequest, ReviewResponse};

const DEFAULT_MAX_TOOL_ITERATIONS: usize = 8;
const DEFAULT_MODEL: &str = "claude-opus-4-7";

/// Backend that implements the tool calls Claude can issue during a review.
///
/// All methods return errors as strings so that the response can be fed
/// back to the model as a `tool_result` block rather than aborting the loop.
#[async_trait]
pub trait ReviewToolBackend: Send + Sync {
    /// Read the current contents of a file in the workspace.
    async fn read_file(&self, path: &str) -> std::result::Result<String, String>;

    /// Semantic symbol search. `depth` is one of "signatures", "full", "call_graph".
    async fn search_symbols(
        &self,
        query: &str,
        depth: &str,
    ) -> std::result::Result<Vec<SymbolHit>, String>;

    /// Fetch caller/callee edges for a symbol.
    async fn get_call_graph(&self, symbol: &str) -> std::result::Result<CallGraphHit, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolHit {
    pub qualified_name: String,
    pub file_path: String,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraphHit {
    pub symbol: String,
    pub callers: Vec<String>,
    pub callees: Vec<String>,
}

/// Default in-memory backend. `read_file` answers from the
/// [`ReviewRequest::context`] map populated before the review was kicked off;
/// symbol search and call graph return empty results.
///
/// This keeps the provider self-contained when no gRPC wiring is available
/// (tests, local dev, CI without a live `AgentService`).
pub struct InMemoryReviewToolBackend {
    files: std::collections::HashMap<String, String>,
}

impl InMemoryReviewToolBackend {
    pub fn from_request(request: &ReviewRequest) -> Self {
        let files = request
            .context
            .iter()
            .map(|f| (f.path.clone(), f.content.clone()))
            .collect();
        Self { files }
    }
}

#[async_trait]
impl ReviewToolBackend for InMemoryReviewToolBackend {
    async fn read_file(&self, path: &str) -> std::result::Result<String, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("file not in review context: {path}"))
    }

    async fn search_symbols(
        &self,
        _query: &str,
        _depth: &str,
    ) -> std::result::Result<Vec<SymbolHit>, String> {
        Ok(Vec::new())
    }

    async fn get_call_graph(&self, symbol: &str) -> std::result::Result<CallGraphHit, String> {
        Ok(CallGraphHit {
            symbol: symbol.to_string(),
            callers: Vec::new(),
            callees: Vec::new(),
        })
    }
}

/// A `ReviewProvider` that lets Claude use read-only tools to gather context
/// before rendering a verdict.
pub struct ClaudeToolUseReviewProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: usize,
    max_tool_iterations: usize,
    backend: std::sync::Arc<dyn ReviewToolBackend>,
}

impl ClaudeToolUseReviewProvider {
    pub fn new(
        api_key: String,
        model: Option<String>,
        backend: std::sync::Arc<dyn ReviewToolBackend>,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .context("failed to build reqwest client")?;
        let max_tool_iterations = std::env::var("DKOD_REVIEW_TOOLUSE_MAX_ITER")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_TOOL_ITERATIONS);
        Ok(Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            max_tokens: 64000,
            max_tool_iterations,
            backend,
        })
    }

    /// Convenience constructor: use an in-memory tool backend seeded from the
    /// caller's `ReviewRequest::context`. Useful in tests and when no gRPC
    /// client is wired in yet.
    pub fn with_in_memory_backend(
        api_key: String,
        model: Option<String>,
        request: &ReviewRequest,
    ) -> Result<Self> {
        let backend = std::sync::Arc::new(InMemoryReviewToolBackend::from_request(request));
        Self::new(api_key, model, backend)
    }

    /// Construct from environment variables. Returns `None` when no API key
    /// is set. Opts in when `DKOD_REVIEW_TOOLUSE=1`; otherwise the caller
    /// should continue using the single-shot [`super::claude::ClaudeReviewProvider`].
    pub fn from_env(backend: std::sync::Arc<dyn ReviewToolBackend>) -> Option<Self> {
        if !env_flag_enabled("DKOD_REVIEW_TOOLUSE") {
            return None;
        }
        let api_key = std::env::var("DKOD_REVIEW_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .ok()?;
        let model = std::env::var("DKOD_REVIEW_MODEL").ok();
        Self::new(api_key, model, backend).ok()
    }
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|v| !matches!(v.to_lowercase().as_str(), "" | "0" | "false" | "no" | "off"))
        .unwrap_or(false)
}

// ── Anthropic wire types (subset sufficient for tool-use) ──────────────────

#[derive(Serialize)]
struct ToolSchema {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
struct WireMessage {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: usize,
    messages: &'a [WireMessage],
    tools: &'a [ToolSchema],
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

fn tool_definitions() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "read_file",
            description: "Read the full current contents of a file in the agent's workspace overlay. Returns the file contents as UTF-8 text.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Repo-relative path, e.g. 'src/auth/login.rs'."
                    }
                },
                "required": ["path"],
            }),
        },
        ToolSchema {
            name: "search_symbols",
            description: "Semantic symbol search. Returns matching functions/types/constants with their qualified name, file path, and signature.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Symbol name, pattern, or natural-language query."
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["signatures", "full", "call_graph"],
                        "default": "signatures",
                        "description": "Retrieval depth."
                    }
                },
                "required": ["query"],
            }),
        },
        ToolSchema {
            name: "get_call_graph",
            description: "Return callers and callees of a symbol (by qualified name). Use to check blast radius before approving a refactor.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Fully-qualified symbol name, e.g. 'crate::auth::login'."
                    }
                },
                "required": ["symbol"],
            }),
        },
    ]
}

impl ClaudeToolUseReviewProvider {
    async fn call_anthropic(
        &self,
        messages: &[WireMessage],
        tools: &[ToolSchema],
    ) -> Result<AnthropicResponse> {
        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            messages,
            tools,
        };
        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to call Anthropic API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API returned {status}: {body}");
        }
        resp.json::<AnthropicResponse>()
            .await
            .context("Failed to parse Anthropic API response")
    }

    /// Dispatch a single `tool_use` block against the backend and return the
    /// rendered tool_result string.
    async fn run_tool(
        backend: &dyn ReviewToolBackend,
        name: &str,
        input: &serde_json::Value,
    ) -> (String, bool) {
        match name {
            "read_file" => {
                let Some(path) = input.get("path").and_then(|v| v.as_str()) else {
                    return ("missing required parameter: path".to_string(), true);
                };
                match backend.read_file(path).await {
                    Ok(content) => (content, false),
                    Err(e) => (e, true),
                }
            }
            "search_symbols" => {
                let Some(query) = input.get("query").and_then(|v| v.as_str()) else {
                    return ("missing required parameter: query".to_string(), true);
                };
                let depth = input
                    .get("depth")
                    .and_then(|v| v.as_str())
                    .unwrap_or("signatures");
                match backend.search_symbols(query, depth).await {
                    Ok(hits) => (
                        serde_json::to_string_pretty(&hits).unwrap_or_else(|_| "[]".to_string()),
                        false,
                    ),
                    Err(e) => (e, true),
                }
            }
            "get_call_graph" => {
                let Some(symbol) = input.get("symbol").and_then(|v| v.as_str()) else {
                    return ("missing required parameter: symbol".to_string(), true);
                };
                match backend.get_call_graph(symbol).await {
                    Ok(hit) => (
                        serde_json::to_string_pretty(&hit).unwrap_or_else(|_| "{}".to_string()),
                        false,
                    ),
                    Err(e) => (e, true),
                }
            }
            other => (format!("unknown tool: {other}"), true),
        }
    }
}

#[async_trait]
impl ReviewProvider for ClaudeToolUseReviewProvider {
    fn name(&self) -> &str {
        "anthropic-tooluse"
    }

    async fn review(&self, request: ReviewRequest) -> Result<ReviewResponse> {
        let tools = tool_definitions();
        let initial_prompt = format!(
            "{prompt}\n\n## Tools\nYou may call `read_file`, `search_symbols`, or `get_call_graph` to gather more context before returning your review JSON. Call tools only when they will materially change your verdict; otherwise return the review directly. Always end the conversation with the JSON response described above.",
            prompt = build_review_prompt(&request)
        );

        let mut messages: Vec<WireMessage> = vec![WireMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: initial_prompt,
            }],
        }];

        let mut final_text: Option<String> = None;

        for _ in 0..self.max_tool_iterations {
            let resp = self.call_anthropic(&messages, &tools).await?;

            // Mirror the assistant turn back so subsequent tool_result blocks
            // reference the same tool_use IDs.
            messages.push(WireMessage {
                role: "assistant".to_string(),
                content: resp.content.clone(),
            });

            // Collect any tool_use blocks and execute them before the next turn.
            let mut tool_results: Vec<ContentBlock> = Vec::new();
            let mut had_tool_use = false;
            let mut assembled_text = String::new();
            for block in &resp.content {
                match block {
                    ContentBlock::Text { text } => {
                        assembled_text.push_str(text);
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        had_tool_use = true;
                        let (content, is_error) =
                            Self::run_tool(self.backend.as_ref(), name, input).await;
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content,
                            is_error: if is_error { Some(true) } else { None },
                        });
                    }
                    ContentBlock::ToolResult { .. } => {
                        // Assistants shouldn't emit tool_result — ignore defensively.
                    }
                }
            }

            if had_tool_use {
                messages.push(WireMessage {
                    role: "user".to_string(),
                    content: tool_results,
                });
                continue;
            }

            // No tool calls this turn — Claude returned the final verdict.
            if !assembled_text.is_empty() {
                final_text = Some(assembled_text);
            }
            if resp.stop_reason.as_deref() == Some("end_turn") || final_text.is_some() {
                break;
            }
        }

        let raw = final_text.ok_or_else(|| {
            anyhow!("tool-use loop exhausted without a final verdict from Claude")
        })?;
        parse_review_response(&raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::agent_review::provider::FileContext;

    fn mk_request() -> ReviewRequest {
        ReviewRequest {
            diff: "@@ diff @@".to_string(),
            context: vec![FileContext {
                path: "src/lib.rs".to_string(),
                content: "pub fn hello() {}".to_string(),
            }],
            language: "rust".to_string(),
            intent: "refactor hello".to_string(),
        }
    }

    #[test]
    fn tool_definitions_expose_three_tools() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["read_file", "search_symbols", "get_call_graph"]);
    }

    #[tokio::test]
    async fn in_memory_backend_reads_context_files() {
        let req = mk_request();
        let backend = InMemoryReviewToolBackend::from_request(&req);
        let body = backend.read_file("src/lib.rs").await.unwrap();
        assert_eq!(body, "pub fn hello() {}");
        let err = backend.read_file("missing.rs").await.unwrap_err();
        assert!(err.contains("missing.rs"));
    }

    #[tokio::test]
    async fn run_tool_reports_missing_argument_as_error() {
        let req = mk_request();
        let backend = InMemoryReviewToolBackend::from_request(&req);
        let (msg, is_err) =
            ClaudeToolUseReviewProvider::run_tool(&backend, "read_file", &serde_json::json!({}))
                .await;
        assert!(is_err);
        assert!(msg.contains("path"));
    }

    #[tokio::test]
    async fn run_tool_unknown_tool_is_error() {
        let req = mk_request();
        let backend = InMemoryReviewToolBackend::from_request(&req);
        let (msg, is_err) = ClaudeToolUseReviewProvider::run_tool(
            &backend,
            "definitely_not_a_tool",
            &serde_json::json!({}),
        )
        .await;
        assert!(is_err);
        assert!(msg.contains("unknown tool"));
    }

    #[test]
    fn from_env_requires_flag_and_api_key() {
        // Flag off → None regardless of key.
        std::env::remove_var("DKOD_REVIEW_TOOLUSE");
        std::env::set_var("DKOD_REVIEW_API_KEY", "sk-test");
        let backend: std::sync::Arc<dyn ReviewToolBackend> =
            std::sync::Arc::new(InMemoryReviewToolBackend {
                files: std::collections::HashMap::new(),
            });
        assert!(ClaudeToolUseReviewProvider::from_env(backend.clone()).is_none());
        std::env::remove_var("DKOD_REVIEW_API_KEY");
    }
}

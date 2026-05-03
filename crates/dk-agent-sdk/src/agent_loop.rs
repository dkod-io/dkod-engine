//! Tool-using agent loop powered by [`adk-rust`](https://docs.rs/adk-core).
//!
//! `AgentDriver` wires a live [`Session`] (gRPC dkod handle) into an
//! [`adk_runner::Runner`] so an LLM (Gemini, Claude, OpenRouter) can drive the
//! dkod tools (`dkod_context`, `dkod_submit`, `dkod_read_file`, …) on its own.
//!
//! Provider selection is env-driven by default:
//!
//! | env var               | provider     |
//! |-----------------------|--------------|
//! | `OPENROUTER_API_KEY`  | OpenRouter   |
//! | `ANTHROPIC_API_KEY`   | Anthropic    |
//! | `GOOGLE_API_KEY`      | Gemini       |
//!
//! ```rust,no_run
//! use dk_agent_sdk::{AgentClient, agent_loop::AgentDriver};
//! # async fn ex() -> dk_agent_sdk::Result<()> {
//! let mut client = AgentClient::connect("http://[::1]:50051", "tok").await?;
//! let session = client.init("acme/web", "fix flaky test").await?;
//! let mut driver = AgentDriver::from_env(session, "dkod-agent")?;
//! let _stream = driver.run("user-1", "find the flaky test and propose a fix").await?;
//! # Ok(()) }
//! ```
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use adk_agent::LlmAgentBuilder;
use adk_core::{
    AdkError, Content, ErrorCategory, ErrorComponent, EventStream, Llm, Tool, ToolContext,
};
use adk_runner::Runner;
use adk_session::InMemorySessionService;
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::{Result, SdkError};
use crate::session::Session;
use crate::tools::{dispatch_tool, tool_definitions};

const DEFAULT_INSTRUCTION: &str = "\
You are a dkod-aware coding agent. You have a connected dkod session and a set \
of dkod_* tools that act on it (dkod_context, dkod_read_file, dkod_write_file, \
dkod_submit, dkod_pre_submit_check, dkod_session_status, dkod_close, …). \
Always reach for those tools instead of guessing. The session is already \
established; do not look for a `session_id` argument — it is injected for you.";

const DEFAULT_OPENROUTER_MODEL: &str = "openai/gpt-4.1-mini";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-5-20250929";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";

/// LLM provider selection for [`AgentDriver`].
#[derive(Debug, Clone)]
pub enum AgentProvider {
    /// OpenRouter — multi-model gateway. `model` is e.g. `openai/gpt-4.1-mini`.
    OpenRouter { api_key: String, model: String },
    /// Anthropic Claude — `model` e.g. `claude-sonnet-4-5-20250929`.
    Anthropic { api_key: String, model: String },
    /// Google Gemini — `model` e.g. `gemini-2.5-flash`.
    Gemini { api_key: String, model: String },
}

impl AgentProvider {
    /// Auto-detect a provider from environment variables.
    ///
    /// Precedence: `OPENROUTER_API_KEY` → `ANTHROPIC_API_KEY` → `GOOGLE_API_KEY`.
    /// Optional model overrides: `OPENROUTER_MODEL`, `ANTHROPIC_MODEL`, `GEMINI_MODEL`.
    pub fn from_env() -> Result<Self> {
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            let model = std::env::var("OPENROUTER_MODEL")
                .unwrap_or_else(|_| DEFAULT_OPENROUTER_MODEL.into());
            return Ok(Self::OpenRouter {
                api_key: key,
                model,
            });
        }
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            let model =
                std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| DEFAULT_ANTHROPIC_MODEL.into());
            return Ok(Self::Anthropic {
                api_key: key,
                model,
            });
        }
        if let Ok(key) = std::env::var("GOOGLE_API_KEY") {
            let model =
                std::env::var("GEMINI_MODEL").unwrap_or_else(|_| DEFAULT_GEMINI_MODEL.into());
            return Ok(Self::Gemini {
                api_key: key,
                model,
            });
        }
        Err(SdkError::AgentConfig(
            "no LLM provider env var found — set OPENROUTER_API_KEY, ANTHROPIC_API_KEY, or GOOGLE_API_KEY".into(),
        ))
    }

    /// Build an `Arc<dyn Llm>` for this provider.
    pub fn build(&self) -> Result<Arc<dyn Llm>> {
        match self {
            Self::OpenRouter { api_key, model } => {
                let cfg =
                    adk_model::openrouter::OpenRouterConfig::new(api_key.clone(), model.clone())
                        .with_http_referer("https://github.com/sjoerd2025/dkod-engine")
                        .with_title("dkod-agent-sdk");
                let client = adk_model::openrouter::OpenRouterClient::new(cfg)
                    .map_err(|e| SdkError::AgentConfig(format!("openrouter: {e}")))?;
                Ok(Arc::new(client) as Arc<dyn Llm>)
            }
            Self::Anthropic { api_key, model } => {
                let cfg =
                    adk_model::anthropic::AnthropicConfig::new(api_key.clone(), model.clone());
                let client = adk_model::anthropic::AnthropicClient::new(cfg)
                    .map_err(|e| SdkError::AgentConfig(format!("anthropic: {e}")))?;
                Ok(Arc::new(client) as Arc<dyn Llm>)
            }
            Self::Gemini { api_key, model } => {
                let client = adk_model::GeminiModel::new(api_key, model)
                    .map_err(|e| SdkError::AgentConfig(format!("gemini: {e}")))?;
                Ok(Arc::new(client) as Arc<dyn Llm>)
            }
        }
    }
}

/// `adk-core` Tool wrapper that dispatches against a shared dkod [`Session`].
///
/// A single instance per dkod tool name (`dkod_context`, etc.). Concurrency is
/// serialized on the session through a [`tokio::sync::Mutex`] because the
/// session holds a unique gRPC stream.
struct DkodSessionTool {
    name: String,
    description: String,
    schema: Value,
    session: Arc<Mutex<Session>>,
}

#[async_trait]
impl Tool for DkodSessionTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(self.schema.clone())
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        _ctx: Arc<dyn ToolContext>,
        args: Value,
    ) -> std::result::Result<Value, AdkError> {
        let mut guard = self.session.lock().await;
        let raw = dispatch_tool(&mut guard, &self.name, &args)
            .await
            .map_err(|e| {
                AdkError::new(
                    ErrorComponent::Tool,
                    ErrorCategory::Internal,
                    "dkod.dispatch",
                    format!("dkod tool {} failed: {e}", self.name),
                )
            })?;
        // dispatch_tool returns a JSON string; surface it as structured Value
        // when possible, otherwise as a JSON string literal.
        Ok(serde_json::from_str(&raw).unwrap_or(Value::String(raw)))
    }
}

/// LLM-driven agent loop bound to a single dkod [`Session`].
///
/// Construction does not consume the session; the driver only borrows it
/// through an [`Arc<Mutex<Session>>`] so callers can interleave manual API
/// calls between agent runs.
pub struct AgentDriver {
    runner: Runner,
    app_name: String,
    /// Underlying dkod session shared with all tool wrappers.
    pub session: Arc<Mutex<Session>>,
}

impl AgentDriver {
    /// Convenience constructor — picks a provider via [`AgentProvider::from_env`].
    pub fn from_env(session: Session, app_name: impl Into<String>) -> Result<Self> {
        Self::new(AgentProvider::from_env()?, session, app_name, None)
    }

    /// Build a driver with an explicit provider and (optionally) an instruction
    /// override. `app_name` is used as the adk app identity for session storage
    /// and observability (it is *not* the dkod repo name).
    pub fn new(
        provider: AgentProvider,
        session: Session,
        app_name: impl Into<String>,
        instruction: Option<String>,
    ) -> Result<Self> {
        let app_name = app_name.into();
        let llm = provider.build()?;
        let session = Arc::new(Mutex::new(session));

        let mut builder = LlmAgentBuilder::new(app_name.clone())
            .model(llm)
            .description("dkod-aware coding agent using adk-rust")
            .instruction(instruction.unwrap_or_else(|| DEFAULT_INSTRUCTION.into()));

        for def in tool_definitions() {
            let tool: Arc<dyn Tool> = Arc::new(DkodSessionTool {
                name: def.name,
                description: def.description,
                schema: def.input_schema,
                session: session.clone(),
            });
            builder = builder.tool(tool);
        }

        let agent = builder
            .build()
            .map_err(|e| SdkError::AgentConfig(format!("build LlmAgent: {e}")))?;

        let runner = Runner::builder()
            .app_name(app_name.clone())
            .agent(Arc::new(agent))
            .session_service(Arc::new(InMemorySessionService::new()))
            .build()
            .map_err(|e| SdkError::AgentConfig(format!("build Runner: {e}")))?;

        Ok(Self {
            runner,
            app_name,
            session,
        })
    }

    /// Returns the adk app name configured at construction.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Run one user turn against the dkod session and stream agent events.
    ///
    /// `user_id` is an opaque adk identifier used for conversation storage; it
    /// has no relationship to the dkod session id.
    /// `session_id` defaults to the dkod session's own id so adk and dkod stay
    /// aligned on a per-session basis.
    pub async fn run(&self, user_id: &str, user_intent: &str) -> Result<EventStream> {
        let session_id = self.session.lock().await.session_id.clone();
        self.run_with_session(user_id, &session_id, user_intent)
            .await
    }

    /// Lower-level variant that lets callers pick an explicit adk session id
    /// (useful for multi-turn conversations independent of the dkod session).
    pub async fn run_with_session(
        &self,
        user_id: &str,
        adk_session_id: &str,
        user_intent: &str,
    ) -> Result<EventStream> {
        let content = Content::new("user").with_text(user_intent);
        self.runner
            .run_str(user_id, adk_session_id, content)
            .await
            .map_err(|e| SdkError::AgentRuntime(format!("runner.run_str: {e}")))
    }
}

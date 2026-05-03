use std::time::Duration;

use super::parse::parse_review_response;
use super::prompt::build_review_prompt;
use super::provider::{ReviewProvider, ReviewRequest, ReviewResponse};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub struct ClaudeReviewProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: usize,
    effort: String,
    adaptive_thinking: bool,
}

impl ClaudeReviewProvider {
    pub fn new(api_key: String, model: Option<String>, max_tokens: Option<usize>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        const DEFAULT_EFFORT: &str = "xhigh";
        const VALID_EFFORTS: &[&str] = &["max", "xhigh", "high", "medium", "low"];
        // Invalid values fall back to the default with a warning — matches the
        // tolerant pattern used for DKOD_REVIEW_MIN_SCORE in dk-mcp's GateConfig.
        // Bailing here would also make ClaudeReviewProvider::new flakey under
        // parallel tests that temporarily set this env var in sibling modules.
        let effort = match std::env::var("DKOD_REVIEW_EFFORT") {
            Ok(v) if VALID_EFFORTS.contains(&v.as_str()) => v,
            Ok(v) => {
                tracing::warn!(
                    "DKOD_REVIEW_EFFORT has invalid value {:?}; valid values: {}. Falling back to {:?}.",
                    v,
                    VALID_EFFORTS.join(", "),
                    DEFAULT_EFFORT
                );
                DEFAULT_EFFORT.to_string()
            }
            Err(_) => DEFAULT_EFFORT.to_string(),
        };
        let adaptive_thinking = std::env::var("DKOD_REVIEW_ADAPTIVE_THINKING")
            .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "no" | "off"))
            .unwrap_or(true);
        Ok(Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| "claude-opus-4-7".to_string()),
            max_tokens: max_tokens.unwrap_or(64000),
            effort,
            adaptive_thinking,
        })
    }

    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DKOD_REVIEW_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .ok()?;
        let model = std::env::var("DKOD_REVIEW_MODEL").ok();
        Self::new(api_key, model, None)
            .map_err(|e| {
                tracing::error!("ClaudeReviewProvider failed to initialise: {e:#}");
                e
            })
            .ok()
    }

    fn is_opus_4_7_or_later(&self) -> bool {
        let Some(rest) = self.model.strip_prefix("claude-opus-") else {
            return false;
        };
        let mut parts = rest.split('-');
        let major = parts.next().and_then(|s| s.parse::<u32>().ok());
        let minor = parts.next().and_then(|s| s.parse::<u32>().ok());
        match (major, minor) {
            (Some(m), _) if m >= 5 => true,
            (Some(4), Some(n)) if n >= 7 => true,
            _ => false,
        }
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: usize,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<OutputConfig>,
}

#[derive(Serialize)]
struct ThinkingConfig {
    r#type: String,
}

#[derive(Serialize)]
struct OutputConfig {
    effort: String,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[async_trait::async_trait]
impl ReviewProvider for ClaudeReviewProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn review(&self, request: ReviewRequest) -> Result<ReviewResponse> {
        let prompt = build_review_prompt(&request);
        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&AnthropicRequest {
                model: self.model.clone(),
                max_tokens: self.max_tokens,
                messages: vec![Message {
                    role: "user".to_string(),
                    content: prompt,
                }],
                thinking: if self.is_opus_4_7_or_later() && self.adaptive_thinking {
                    Some(ThinkingConfig {
                        r#type: "adaptive".to_string(),
                    })
                } else {
                    None
                },
                output_config: if self.is_opus_4_7_or_later() {
                    Some(OutputConfig {
                        effort: self.effort.clone(),
                    })
                } else {
                    None
                },
            })
            .send()
            .await
            .context("Failed to call Anthropic API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API returned {status}: {body}");
        }

        let api_resp: AnthropicResponse = resp
            .json()
            .await
            .context("Failed to parse Anthropic API response")?;
        let text = api_resp
            .content
            .into_iter()
            .filter_map(|b| b.text)
            .collect::<Vec<_>>()
            .join("");
        parse_review_response(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-var mutation across tests in this module.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_env() {
        for k in [
            "DKOD_REVIEW_EFFORT",
            "DKOD_REVIEW_ADAPTIVE_THINKING",
            "DKOD_REVIEW_MODEL",
            "DKOD_REVIEW_API_KEY",
            "ANTHROPIC_API_KEY",
        ] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn is_opus_4_7_or_later_matches_new_models() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        for name in [
            "claude-opus-4-7",
            "claude-opus-4-7-20260101",
            "claude-opus-4-8",
            "claude-opus-4-9",
            "claude-opus-4-10",
            "claude-opus-4-11",
            "claude-opus-4-15-preview",
            "claude-opus-5",
            "claude-opus-5-0",
            "claude-opus-6",
            "claude-opus-7-0-20270101",
        ] {
            let p = ClaudeReviewProvider::new("k".into(), Some(name.into()), None).unwrap();
            assert!(p.is_opus_4_7_or_later(), "expected {name} to match");
        }
    }

    #[test]
    fn is_opus_4_7_or_later_rejects_older_models() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let p =
            ClaudeReviewProvider::new("k".into(), Some("claude-opus-4-6".into()), None).unwrap();
        assert!(!p.is_opus_4_7_or_later());
        let p =
            ClaudeReviewProvider::new("k".into(), Some("claude-sonnet-4-6".into()), None).unwrap();
        assert!(!p.is_opus_4_7_or_later());
    }

    #[test]
    fn defaults_effort_xhigh_and_adaptive_on() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let p = ClaudeReviewProvider::new("k".into(), None, None).unwrap();
        assert_eq!(p.effort, "xhigh");
        assert!(p.adaptive_thinking);
        assert_eq!(p.max_tokens, 64000);
        assert_eq!(p.model, "claude-opus-4-7");
    }

    #[test]
    fn effort_overridable_from_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        std::env::set_var("DKOD_REVIEW_EFFORT", "high");
        let p = ClaudeReviewProvider::new("k".into(), None, None).unwrap();
        assert_eq!(p.effort, "high");
        clear_env();
    }

    #[test]
    fn adaptive_thinking_disabled_by_env_zero() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        std::env::set_var("DKOD_REVIEW_ADAPTIVE_THINKING", "0");
        let p = ClaudeReviewProvider::new("k".into(), None, None).unwrap();
        assert!(!p.adaptive_thinking);
        clear_env();
    }

    #[test]
    fn request_body_includes_thinking_and_effort_for_opus_4_7() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        let p = ClaudeReviewProvider::new("k".into(), None, None).unwrap();
        let body = AnthropicRequest {
            model: p.model.clone(),
            max_tokens: p.max_tokens,
            messages: vec![Message {
                role: "user".into(),
                content: "hi".into(),
            }],
            thinking: Some(ThinkingConfig {
                r#type: "adaptive".into(),
            }),
            output_config: Some(OutputConfig {
                effort: p.effort.clone(),
            }),
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"thinking\":{\"type\":\"adaptive\"}"));
        assert!(json.contains("\"output_config\":{\"effort\":\"xhigh\"}"));
    }

    #[test]
    fn request_body_omits_thinking_and_effort_when_none() {
        let body = AnthropicRequest {
            model: "claude-opus-4-6".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: "user".into(),
                content: "hi".into(),
            }],
            thinking: None,
            output_config: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("\"thinking\""));
        assert!(!json.contains("\"output_config\""));
    }

    #[test]
    fn invalid_effort_value_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        std::env::set_var("DKOD_REVIEW_EFFORT", "extreme");
        let p = ClaudeReviewProvider::new("k".into(), None, None)
            .expect("invalid effort should fall back, not bail");
        assert_eq!(p.effort, "xhigh");
        clear_env();
    }

    #[test]
    fn adaptive_thinking_disable_variants_case_insensitive() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in ["0", "false", "False", "FALSE", "no", "NO", "off", "Off"] {
            clear_env();
            std::env::set_var("DKOD_REVIEW_ADAPTIVE_THINKING", v);
            let p = ClaudeReviewProvider::new("k".into(), None, None).unwrap();
            assert!(!p.adaptive_thinking, "expected {v} to disable");
        }
        clear_env();
    }

    #[test]
    fn adaptive_thinking_enable_variants() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in ["1", "true", "yes", "on", ""] {
            clear_env();
            std::env::set_var("DKOD_REVIEW_ADAPTIVE_THINKING", v);
            let p = ClaudeReviewProvider::new("k".into(), None, None).unwrap();
            assert!(p.adaptive_thinking, "expected {v:?} to leave thinking on");
        }
        clear_env();
    }

    #[test]
    fn from_env_falls_back_to_default_effort_when_invalid() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        std::env::set_var("ANTHROPIC_API_KEY", "k");
        std::env::set_var("DKOD_REVIEW_EFFORT", "turbo");
        // Invalid effort now logs a warning and falls back to the default,
        // so from_env() still returns Some. This keeps the provider factory
        // robust against stray env vars and avoids cross-module test races.
        let p = ClaudeReviewProvider::from_env().expect("fallback should succeed");
        assert_eq!(p.effort, "xhigh");
        clear_env();
    }

    #[test]
    fn from_env_returns_some_when_valid() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();
        std::env::set_var("ANTHROPIC_API_KEY", "k");
        // Defaults (xhigh effort, adaptive on) are valid.
        assert!(ClaudeReviewProvider::from_env().is_some());
        clear_env();
    }
}

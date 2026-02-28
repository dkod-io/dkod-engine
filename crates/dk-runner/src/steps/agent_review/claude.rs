use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use super::parse::parse_review_response;
use super::prompt::build_review_prompt;
use super::provider::{ReviewProvider, ReviewRequest, ReviewResponse};

pub struct ClaudeReviewProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: usize,
}

impl ClaudeReviewProvider {
    pub fn new(api_key: String, model: Option<String>, max_tokens: Option<usize>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| "claude-sonnet-4-6".to_string()),
            max_tokens: max_tokens.unwrap_or(4096),
        })
    }

    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DEKODE_REVIEW_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .ok()?;
        let model = std::env::var("DEKODE_REVIEW_MODEL").ok();
        Self::new(api_key, model, None).ok()
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: usize,
    messages: Vec<Message>,
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
        "claude"
    }

    async fn review(&self, request: ReviewRequest) -> Result<ReviewResponse> {
        let prompt = build_review_prompt(&request);
        let resp = self.client
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

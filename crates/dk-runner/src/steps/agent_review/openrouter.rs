use super::parse::parse_review_response;
use super::prompt::build_review_prompt;
use super::provider::{ReviewProvider, ReviewRequest, ReviewResponse};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct OpenRouterReviewProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: usize,
    base_url: String,
}

impl OpenRouterReviewProvider {
    pub fn new(
        api_key: String,
        model: Option<String>,
        max_tokens: Option<usize>,
        base_url: Option<String>,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| "anthropic/claude-opus-4.7".to_string()),
            max_tokens: max_tokens.unwrap_or(4096),
            base_url: base_url.unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
        })
    }

    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DKOD_OPENROUTER_API_KEY").ok()?;
        let model = std::env::var("DKOD_REVIEW_MODEL").ok();
        let base_url = std::env::var("DKOD_OPENROUTER_BASE_URL").ok();
        Self::new(api_key, model, None, base_url).ok()
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: usize,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

#[async_trait::async_trait]
impl ReviewProvider for OpenRouterReviewProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    async fn review(&self, request: ReviewRequest) -> Result<ReviewResponse> {
        let prompt = build_review_prompt(&request);
        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://dkod.io")
            .header("X-Title", "dkod code review")
            .json(&ChatRequest {
                model: self.model.clone(),
                max_tokens: self.max_tokens,
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                }],
            })
            .send()
            .await
            .context("Failed to call OpenRouter API")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenRouter API returned {status}: {body}");
        }
        let api_resp: ChatResponse = resp
            .json()
            .await
            .context("Failed to parse OpenRouter API response")?;
        let text = api_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();
        parse_review_response(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::OpenRouterReviewProvider;
    use crate::steps::agent_review::provider::ReviewProvider;

    fn clear() {
        for k in [
            "DKOD_OPENROUTER_API_KEY",
            "DKOD_REVIEW_MODEL",
            "DKOD_OPENROUTER_BASE_URL",
        ] {
            std::env::remove_var(k);
        }
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn from_env_returns_none_without_key() {
        clear();
        assert!(OpenRouterReviewProvider::from_env().is_none());
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn from_env_builds_provider_with_key() {
        clear();
        std::env::set_var("DKOD_OPENROUTER_API_KEY", "sk-test");
        let p = OpenRouterReviewProvider::from_env().unwrap();
        assert_eq!(p.name(), "openrouter");
        clear();
    }
}

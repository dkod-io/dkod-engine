use anyhow::Result;
use crate::findings::{Finding, Suggestion};

#[derive(Debug, Clone)]
pub struct ReviewRequest {
    pub diff: String,
    pub context: Vec<FileContext>,
    pub language: String,
    pub intent: String,
}

#[derive(Debug, Clone)]
pub struct FileContext {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
    Comment,
}

#[derive(Debug, Clone)]
pub struct ReviewResponse {
    pub summary: String,
    pub findings: Vec<Finding>,
    pub suggestions: Vec<Suggestion>,
    pub verdict: ReviewVerdict,
}

#[async_trait::async_trait]
pub trait ReviewProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn review(&self, request: ReviewRequest) -> Result<ReviewResponse>;
}

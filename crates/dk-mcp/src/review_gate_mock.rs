//! Mock `ReviewProvider` for use in unit and integration tests. This module is
//! gated behind `#[cfg(any(test, feature = "mock-review"))]` so it never
//! contributes to a production build unless the `mock-review` feature is
//! explicitly enabled by a test binary.

use anyhow::Result;
use async_trait::async_trait;
use dk_runner::findings::{Finding, Suggestion};
use dk_runner::steps::agent_review::provider::{
    ReviewProvider, ReviewRequest, ReviewResponse, ReviewVerdict,
};

/// Fixed-response `ReviewProvider` used by tests. Always returns the
/// `verdict` and `findings` supplied at construction time.
pub struct MockReviewProvider {
    verdict: ReviewVerdict,
    findings: Vec<Finding>,
}

impl MockReviewProvider {
    pub fn new(verdict: ReviewVerdict, findings: Vec<Finding>) -> Self {
        Self { verdict, findings }
    }
}

#[async_trait]
impl ReviewProvider for MockReviewProvider {
    fn name(&self) -> &str {
        "mock"
    }
    async fn review(&self, _req: ReviewRequest) -> Result<ReviewResponse> {
        Ok(ReviewResponse {
            summary: "mock".into(),
            findings: self.findings.clone(),
            suggestions: Vec::<Suggestion>::new(),
            verdict: self.verdict.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MockReviewProvider;
    use dk_runner::steps::agent_review::provider::{ReviewProvider, ReviewRequest, ReviewVerdict};

    #[tokio::test]
    async fn mock_returns_configured_score() {
        let m = MockReviewProvider::new(ReviewVerdict::Approve, vec![]);
        let resp = m
            .review(ReviewRequest {
                diff: "".into(),
                context: vec![],
                language: "rust".into(),
                intent: "t".into(),
            })
            .await
            .unwrap();
        assert!(matches!(resp.verdict, ReviewVerdict::Approve));
    }
}

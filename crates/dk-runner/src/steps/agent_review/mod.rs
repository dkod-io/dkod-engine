pub mod provider;
pub mod claude;
pub mod prompt;
pub mod parse;

use std::time::Instant;
use crate::executor::{StepOutput, StepStatus};
use crate::findings::{Finding, Suggestion};
use provider::{ReviewProvider, ReviewRequest, FileContext, ReviewVerdict};

pub async fn run_agent_review_step_with_provider(
    provider: &dyn ReviewProvider,
    diff: &str,
    files: Vec<FileContext>,
    intent: &str,
) -> (StepOutput, Vec<Finding>, Vec<Suggestion>) {
    let start = Instant::now();
    let request = ReviewRequest {
        diff: diff.to_string(),
        context: files,
        language: "rust".to_string(),
        intent: intent.to_string(),
    };

    match provider.review(request).await {
        Ok(response) => {
            let status = match response.verdict {
                ReviewVerdict::Approve => StepStatus::Pass,
                ReviewVerdict::RequestChanges => StepStatus::Fail,
                ReviewVerdict::Comment => StepStatus::Pass,
            };
            (
                StepOutput {
                    status,
                    stdout: format!(
                        "Agent Review ({}): {}",
                        provider.name(),
                        response.summary
                    ),
                    stderr: String::new(),
                    duration: start.elapsed(),
                },
                response.findings,
                response.suggestions,
            )
        }
        Err(e) => {
            let finding = Finding {
                severity: crate::findings::Severity::Warning,
                check_name: "agent-review-error".to_string(),
                message: format!("Agent review failed: {e}"),
                file_path: None,
                line: None,
                symbol: None,
            };
            (
                StepOutput {
                    status: StepStatus::Pass,
                    stdout: format!(
                        "Agent Review ({}): error -- {e}",
                        provider.name()
                    ),
                    stderr: String::new(),
                    duration: start.elapsed(),
                },
                vec![finding],
                Vec::new(),
            )
        }
    }
}

/// Legacy stub for when no provider is configured.
pub async fn run_agent_review_step(prompt: &str) -> StepOutput {
    let start = Instant::now();
    StepOutput {
        status: StepStatus::Pass,
        stdout: format!(
            "agent review: skipped (no provider configured)\nprompt: {}",
            prompt
        ),
        stderr: String::new(),
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::Severity;

    struct MockProvider {
        response: Result<provider::ReviewResponse, String>,
    }

    #[async_trait::async_trait]
    impl provider::ReviewProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }
        async fn review(
            &self,
            _req: provider::ReviewRequest,
        ) -> anyhow::Result<provider::ReviewResponse> {
            match &self.response {
                Ok(r) => Ok(r.clone()),
                Err(msg) => anyhow::bail!("{}", msg),
            }
        }
    }

    fn make_request_args() -> (String, Vec<provider::FileContext>, String) {
        ("diff".to_string(), vec![], "test intent".to_string())
    }

    #[tokio::test]
    async fn test_approve_verdict_returns_pass() {
        let provider = MockProvider {
            response: Ok(provider::ReviewResponse {
                summary: "LGTM".to_string(),
                findings: vec![],
                suggestions: vec![],
                verdict: provider::ReviewVerdict::Approve,
            }),
        };
        let (diff, files, intent) = make_request_args();
        let (output, findings, suggestions) =
            run_agent_review_step_with_provider(&provider, &diff, files, &intent).await;
        assert_eq!(output.status, StepStatus::Pass);
        assert!(findings.is_empty());
        assert!(suggestions.is_empty());
    }

    #[tokio::test]
    async fn test_request_changes_verdict_returns_fail() {
        let provider = MockProvider {
            response: Ok(provider::ReviewResponse {
                summary: "Issues found".to_string(),
                findings: vec![Finding {
                    severity: Severity::Error,
                    check_name: "test".to_string(),
                    message: "bad".to_string(),
                    file_path: None,
                    line: None,
                    symbol: None,
                }],
                suggestions: vec![],
                verdict: provider::ReviewVerdict::RequestChanges,
            }),
        };
        let (diff, files, intent) = make_request_args();
        let (output, findings, _) =
            run_agent_review_step_with_provider(&provider, &diff, files, &intent).await;
        assert_eq!(output.status, StepStatus::Fail);
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn test_provider_error_returns_pass_with_warning() {
        let provider = MockProvider {
            response: Err("API error".to_string()),
        };
        let (diff, files, intent) = make_request_args();
        let (output, findings, _) =
            run_agent_review_step_with_provider(&provider, &diff, files, &intent).await;
        assert_eq!(output.status, StepStatus::Pass); // soft fail
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[tokio::test]
    async fn test_legacy_stub_passes() {
        let output = run_agent_review_step("test prompt").await;
        assert_eq!(output.status, StepStatus::Pass);
        assert!(output.stdout.contains("no provider configured"));
    }
}

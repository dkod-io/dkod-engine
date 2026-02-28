use anyhow::{Context, Result};
use serde::Deserialize;
use crate::findings::{Finding, Severity, Suggestion};
use super::provider::{ReviewResponse, ReviewVerdict};

#[derive(Deserialize)]
struct LlmResponse {
    summary: String,
    #[serde(default)]
    issues: Vec<LlmIssue>,
    verdict: String,
}

#[derive(Deserialize)]
struct LlmIssue {
    severity: String,
    check_name: String,
    message: String,
    file_path: Option<String>,
    line: Option<u32>,
    suggestion: Option<String>,
}

pub fn parse_review_response(raw: &str) -> Result<ReviewResponse> {
    let json_str = extract_json(raw);
    let parsed: LlmResponse = serde_json::from_str(json_str)
        .context("Failed to parse LLM review response as JSON")?;

    let mut findings = Vec::new();
    let mut suggestions = Vec::new();

    for (i, issue) in parsed.issues.iter().enumerate() {
        let severity = match issue.severity.as_str() {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => Severity::Info,
        };
        findings.push(Finding {
            severity,
            check_name: issue.check_name.clone(),
            message: issue.message.clone(),
            file_path: issue.file_path.clone(),
            line: issue.line,
            symbol: None,
        });
        if let Some(text) = &issue.suggestion {
            suggestions.push(Suggestion {
                finding_index: i,
                description: text.clone(),
                file_path: issue.file_path.clone().unwrap_or_default(),
                replacement: None,
            });
        }
    }

    let verdict = match parsed.verdict.as_str() {
        "approve" => ReviewVerdict::Approve,
        "request_changes" => ReviewVerdict::RequestChanges,
        _ => ReviewVerdict::Comment,
    };

    Ok(ReviewResponse {
        summary: parsed.summary,
        findings,
        suggestions,
        verdict,
    })
}

fn extract_json(raw: &str) -> &str {
    if let Some(start) = raw.find("```json") {
        let after = &raw[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = raw.find("```") {
        let after = &raw[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = raw.find('{') {
        if let Some(end) = raw.rfind('}') {
            return &raw[start..=end];
        }
    }
    raw.trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let raw = r#"{"summary":"LGTM","issues":[],"verdict":"approve"}"#;
        let resp = parse_review_response(raw).unwrap();
        assert_eq!(resp.summary, "LGTM");
        assert!(resp.findings.is_empty());
        assert!(matches!(resp.verdict, ReviewVerdict::Approve));
    }

    #[test]
    fn test_parse_json_in_code_block() {
        let raw = "```json\n{\"summary\":\"ok\",\"issues\":[],\"verdict\":\"approve\"}\n```";
        let resp = parse_review_response(raw).unwrap();
        assert_eq!(resp.summary, "ok");
    }

    #[test]
    fn test_parse_with_issues() {
        let raw = r#"{"summary":"Issues found","issues":[{"severity":"error","check_name":"null-check","message":"Missing null check","file_path":"src/lib.rs","line":42,"suggestion":"Add a null check"}],"verdict":"request_changes"}"#;
        let resp = parse_review_response(raw).unwrap();
        assert_eq!(resp.findings.len(), 1);
        assert_eq!(resp.suggestions.len(), 1);
        assert!(matches!(resp.verdict, ReviewVerdict::RequestChanges));
    }
}

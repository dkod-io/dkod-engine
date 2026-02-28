use super::provider::ReviewRequest;

const MAX_DIFF_CHARS: usize = 50_000;
const MAX_CONTEXT_CHARS: usize = 100_000;

pub fn build_review_prompt(request: &ReviewRequest) -> String {
    let diff = if request.diff.len() > MAX_DIFF_CHARS {
        &request.diff[..MAX_DIFF_CHARS]
    } else {
        &request.diff
    };

    let mut context_budget = MAX_CONTEXT_CHARS;
    let mut file_sections = Vec::new();
    let mut sorted_files = request.context.clone();
    sorted_files.sort_by_key(|f| f.content.len());

    for file in &sorted_files {
        if file.content.len() > context_budget {
            break;
        }
        context_budget -= file.content.len();
        file_sections.push(format!("### {}\n```\n{}\n```", file.path, file.content));
    }

    format!(
        r#"You are a code reviewer. Review the following changeset and return your review as JSON.

## Intent
{intent}

## Language
{language}

## Diff
```
{diff}
```

## File Context
{context}

## Response Format
Return ONLY valid JSON with this structure:
{{
  "summary": "Brief summary of the changes",
  "issues": [
    {{
      "severity": "error"|"warning"|"info",
      "check_name": "descriptive-name",
      "message": "Description of the issue",
      "file_path": "path/to/file.rs",
      "line": 42,
      "suggestion": "How to fix it"
    }}
  ],
  "verdict": "approve"|"request_changes"|"comment"
}}"#,
        intent = request.intent,
        language = request.language,
        diff = diff,
        context = file_sections.join("\n\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::agent_review::provider::{FileContext, ReviewRequest};

    #[test]
    fn test_prompt_contains_intent_and_language() {
        let req = ReviewRequest {
            diff: "some diff".to_string(),
            context: vec![],
            language: "rust".to_string(),
            intent: "Add feature X".to_string(),
        };
        let prompt = build_review_prompt(&req);
        assert!(prompt.contains("Add feature X"));
        assert!(prompt.contains("rust"));
        assert!(prompt.contains("some diff"));
    }

    #[test]
    fn test_diff_truncation() {
        let long_diff = "x".repeat(MAX_DIFF_CHARS + 1000);
        let req = ReviewRequest {
            diff: long_diff,
            context: vec![],
            language: "rust".to_string(),
            intent: "test".to_string(),
        };
        let prompt = build_review_prompt(&req);
        // Prompt should not contain the full diff
        assert!(prompt.len() < MAX_DIFF_CHARS + 5000); // some overhead for template
    }

    #[test]
    fn test_context_budget_excludes_large_files() {
        let small = FileContext {
            path: "small.rs".to_string(),
            content: "fn small() {}".to_string(),
        };
        let huge = FileContext {
            path: "huge.rs".to_string(),
            content: "x".repeat(MAX_CONTEXT_CHARS + 1),
        };
        let req = ReviewRequest {
            diff: "diff".to_string(),
            context: vec![small.clone(), huge],
            language: "rust".to_string(),
            intent: "test".to_string(),
        };
        let prompt = build_review_prompt(&req);
        assert!(prompt.contains("small.rs"));
        assert!(!prompt.contains("huge.rs")); // excluded by budget
    }
}

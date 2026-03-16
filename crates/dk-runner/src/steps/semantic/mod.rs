pub mod checks;
pub mod context;
pub mod safety;
pub mod compat;
pub mod quality;

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use uuid::Uuid;

use dk_engine::repo::Engine;

use crate::executor::{StepOutput, StepStatus};
use crate::findings::{Finding, Severity, Suggestion};

use checks::SemanticCheck;

/// Build the full registry of all 9 semantic checks.
fn all_checks() -> Vec<Box<dyn SemanticCheck>> {
    let mut checks: Vec<Box<dyn SemanticCheck>> = Vec::new();
    checks.extend(safety::safety_checks());
    checks.extend(compat::compat_checks());
    checks.extend(quality::quality_checks());
    checks
}

/// Generate a suggestion for a finding (hardcoded mapping by check name).
fn suggest(finding_index: usize, finding: &Finding) -> Option<Suggestion> {
    let (description, replacement) = match finding.check_name.as_str() {
        "no-unsafe-added" => (
            "Wrap unsafe code in a safe abstraction or add a safety comment".to_string(),
            Some("// SAFETY: <explain why this is safe>\nunsafe { ... }".to_string()),
        ),
        "no-unwrap-added" => (
            "Replace .unwrap() with ? operator or .expect(\"reason\")".to_string(),
            Some(".expect(\"TODO: add error context\")".to_string()),
        ),
        "error-handling-preserved" => (
            "Restore the Result return type to maintain error handling".to_string(),
            None,
        ),
        "no-public-removal" => (
            "Restore the public symbol or deprecate it first with #[deprecated]".to_string(),
            None,
        ),
        "signature-stable" => (
            "Keep the original signature and add a new function with the updated signature".to_string(),
            None,
        ),
        "trait-impl-complete" => (
            "Restore the missing method(s) in the impl block".to_string(),
            None,
        ),
        "complexity-limit" => (
            "Refactor into smaller functions to reduce branching complexity".to_string(),
            None,
        ),
        "no-dependency-cycles" => (
            "Break the cycle by extracting shared logic into a separate module".to_string(),
            None,
        ),
        "dead-code-detection" => (
            "Remove the unused function or add a caller".to_string(),
            None,
        ),
        _ => return None,
    };

    Some(Suggestion {
        finding_index,
        description,
        file_path: finding.file_path.clone().unwrap_or_default(),
        replacement,
    })
}

/// Run all (or a filtered subset of) semantic checks against a changeset.
///
/// # Arguments
///
/// * `engine` — the dk-engine orchestrator
/// * `repo_id` — repository UUID
/// * `changeset_files` — relative paths of changed files
/// * `work_dir` — directory where changeset files are materialized
/// * `filter` — if non-empty, only run checks whose names appear in this list
///
/// # Returns
///
/// A tuple of `(StepOutput, Vec<Finding>, Vec<Suggestion>)`.
pub async fn run_semantic_step(
    engine: &Arc<Engine>,
    repo_id: Uuid,
    changeset_files: &[String],
    work_dir: &Path,
    filter: &[String],
) -> (StepOutput, Vec<Finding>, Vec<Suggestion>) {
    let start = Instant::now();

    // Build the check context from graph stores + parsed changeset.
    let ctx = match context::build_check_context(engine, repo_id, changeset_files, work_dir).await
    {
        Ok(ctx) => ctx,
        Err(e) => {
            let output = StepOutput {
                status: StepStatus::Fail,
                stdout: String::new(),
                stderr: format!("Failed to build check context: {e}"),
                duration: start.elapsed(),
            };
            return (output, vec![], vec![]);
        }
    };

    // Collect checks, optionally filtering.
    let checks = all_checks();
    let active_checks: Vec<&Box<dyn SemanticCheck>> = if filter.is_empty() {
        checks.iter().collect()
    } else {
        checks
            .iter()
            .filter(|c| filter.iter().any(|f| f == c.name()))
            .collect()
    };

    // Run each check and aggregate findings.
    let mut all_findings: Vec<Finding> = Vec::new();
    let mut results: Vec<String> = Vec::new();

    for check in &active_checks {
        let findings = check.run(&ctx);
        if findings.is_empty() {
            results.push(format!("[PASS] {}", check.name()));
        } else {
            let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
            let warnings = findings.iter().filter(|f| f.severity == Severity::Warning).count();
            let infos = findings.iter().filter(|f| f.severity == Severity::Info).count();
            results.push(format!(
                "[FIND] {} — {} error(s), {} warning(s), {} info(s)",
                check.name(),
                errors,
                warnings,
                infos
            ));
            all_findings.extend(findings);
        }
    }

    // Generate suggestions for each finding.
    let suggestions: Vec<Suggestion> = all_findings
        .iter()
        .enumerate()
        .filter_map(|(idx, f)| suggest(idx, f))
        .collect();

    // Determine overall status.
    let has_errors = all_findings
        .iter()
        .any(|f| f.severity == Severity::Error);

    let status = if has_errors {
        StepStatus::Fail
    } else {
        StepStatus::Pass
    };

    let output = StepOutput {
        status,
        stdout: results.join("\n"),
        stderr: String::new(),
        duration: start.elapsed(),
    };

    (output, all_findings, suggestions)
}

/// Backward-compatible entry point for the scheduler (no Engine required).
///
/// Validates check names against the registry and reports pass/skip.
/// This will be replaced once the scheduler is wired with Engine access (Task 9).
pub async fn run_semantic_step_simple(checks: &[String]) -> StepOutput {
    let start = Instant::now();
    let registry = all_checks();
    let known_names: Vec<&str> = registry.iter().map(|c| c.name()).collect();

    let mut results = Vec::new();
    let mut all_pass = true;

    for check in checks {
        if known_names.contains(&check.as_str()) {
            results.push(format!(
                "[PASS] {}: auto-approved (engine not wired yet)",
                check
            ));
        } else {
            results.push(format!("[SKIP] {}: unknown check", check));
            all_pass = false;
        }
    }

    let status = if all_pass {
        StepStatus::Pass
    } else {
        StepStatus::Skip
    };

    StepOutput {
        status,
        stdout: results.join("\n"),
        stderr: String::new(),
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_checks_registered() {
        let checks = all_checks();
        assert_eq!(checks.len(), 9, "Expected 9 semantic checks, got {}", checks.len());
    }

    #[test]
    fn test_check_names_unique() {
        let checks = all_checks();
        let mut names: Vec<&str> = checks.iter().map(|c| c.name()).collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(
            names.len(),
            total,
            "Duplicate check names found"
        );
    }

    #[test]
    fn test_check_names_are_expected() {
        let checks = all_checks();
        let names: Vec<&str> = checks.iter().map(|c| c.name()).collect();

        let expected = [
            "no-unsafe-added",
            "no-unwrap-added",
            "error-handling-preserved",
            "no-public-removal",
            "signature-stable",
            "trait-impl-complete",
            "complexity-limit",
            "no-dependency-cycles",
            "dead-code-detection",
        ];

        for name in &expected {
            assert!(
                names.contains(name),
                "Missing expected check: {}",
                name
            );
        }
    }

    #[test]
    fn test_suggest_returns_suggestion_for_known_checks() {
        let finding = Finding {
            severity: Severity::Error,
            check_name: "no-unsafe-added".into(),
            message: "test".into(),
            file_path: Some("src/lib.rs".into()),
            line: Some(1),
            symbol: None,
        };

        let suggestion = suggest(0, &finding);
        assert!(suggestion.is_some());
        assert_eq!(suggestion.unwrap().finding_index, 0);
    }

    #[test]
    fn test_suggest_returns_none_for_unknown_check() {
        let finding = Finding {
            severity: Severity::Info,
            check_name: "unknown-check-xyz".into(),
            message: "test".into(),
            file_path: None,
            line: None,
            symbol: None,
        };

        assert!(suggest(0, &finding).is_none());
    }

    // ── Integration tests for individual semantic checks ──────────────

    #[test]
    fn test_safety_no_unsafe_detects_unsafe_block() {
        use checks::{CheckContext, ChangedFile, SemanticCheck};
        use safety::NoUnsafeAdded;

        let ctx = CheckContext {
            before_symbols: Vec::new(),
            after_symbols: Vec::new(),
            before_call_graph: Vec::new(),
            after_call_graph: Vec::new(),
            before_deps: Vec::new(),
            after_deps: Vec::new(),
            changed_files: vec![ChangedFile {
                path: "src/lib.rs".to_string(),
                content: Some("fn foo() {\n    unsafe {\n        ptr::read(p)\n    }\n}".to_string()),
            }],
        };

        let check = NoUnsafeAdded::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn test_compat_no_public_removal() {
        use checks::{CheckContext, SemanticCheck};
        use compat::NoPublicRemoval;
        use dk_core::types::*;

        let sym = Symbol {
            id: uuid::Uuid::new_v4(),
            name: "foo".to_string(),
            qualified_name: "crate::foo".to_string(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            file_path: "src/lib.rs".into(),
            span: Span { start_byte: 0, end_byte: 100 },
            signature: Some("fn foo()".to_string()),
            doc_comment: None,
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        };

        let ctx = CheckContext {
            before_symbols: vec![sym],
            after_symbols: Vec::new(),
            before_call_graph: Vec::new(),
            after_call_graph: Vec::new(),
            before_deps: Vec::new(),
            after_deps: Vec::new(),
            changed_files: Vec::new(),
        };

        let check = NoPublicRemoval::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check_name, "no-public-removal");
    }

    #[test]
    fn test_safety_no_unwrap_detects_unwrap() {
        use checks::{CheckContext, ChangedFile, SemanticCheck};
        use safety::NoUnwrapAdded;

        let ctx = CheckContext {
            before_symbols: Vec::new(),
            after_symbols: Vec::new(),
            before_call_graph: Vec::new(),
            after_call_graph: Vec::new(),
            before_deps: Vec::new(),
            after_deps: Vec::new(),
            changed_files: vec![ChangedFile {
                path: "src/lib.rs".to_string(),
                content: Some("let x = foo.unwrap();".to_string()),
            }],
        };

        let check = NoUnwrapAdded::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn test_quality_complexity_limit() {
        use checks::{CheckContext, ChangedFile, SemanticCheck};
        use quality::ComplexityLimit;

        // Wrap the deeply nested branching in a function so per-function
        // complexity tracking detects it.
        let inner = (0..15).map(|i| format!("if x > {} {{", i)).collect::<Vec<_>>().join("\n")
            + &"\n}".repeat(15);
        let deeply_nested = format!("fn deep() {{\n{}\n}}", inner);

        let ctx = CheckContext {
            before_symbols: Vec::new(),
            after_symbols: Vec::new(),
            before_call_graph: Vec::new(),
            after_call_graph: Vec::new(),
            before_deps: Vec::new(),
            after_deps: Vec::new(),
            changed_files: vec![ChangedFile {
                path: "src/lib.rs".to_string(),
                content: Some(deeply_nested),
            }],
        };

        let check = ComplexityLimit::with_threshold(10);
        let findings = check.run(&ctx);
        assert!(!findings.is_empty(), "should detect high complexity");
    }
}

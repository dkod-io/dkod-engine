use regex::Regex;

use crate::findings::{Finding, Severity};

use super::checks::{CheckContext, SemanticCheck};

// ─── no-unsafe-added ─────────────────────────────────────────────────────

/// Flags any `unsafe {` blocks found in changed files.
pub struct NoUnsafeAdded {
    re: Regex,
}

impl NoUnsafeAdded {
    pub fn new() -> Self {
        Self {
            re: Regex::new(r"unsafe\s*\{").expect("invalid regex"),
        }
    }
}

impl SemanticCheck for NoUnsafeAdded {
    fn name(&self) -> &str {
        "no-unsafe-added"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        for file in &ctx.changed_files {
            let content = match &file.content {
                Some(c) => c,
                None => continue,
            };

            for (line_idx, line) in content.lines().enumerate() {
                if self.re.is_match(line) {
                    findings.push(Finding {
                        severity: Severity::Error,
                        check_name: self.name().to_string(),
                        message: format!(
                            "unsafe block found at line {}",
                            line_idx + 1
                        ),
                        file_path: Some(file.path.clone()),
                        line: Some((line_idx + 1) as u32),
                        symbol: None,
                    });
                }
            }
        }

        findings
    }
}

// ─── no-unwrap-added ─────────────────────────────────────────────────────

/// Flags `.unwrap()` calls in changed files, skipping test files.
pub struct NoUnwrapAdded {
    re: Regex,
}

impl NoUnwrapAdded {
    pub fn new() -> Self {
        Self {
            re: Regex::new(r"\.unwrap\(\)").expect("invalid regex"),
        }
    }
}

impl SemanticCheck for NoUnwrapAdded {
    fn name(&self) -> &str {
        "no-unwrap-added"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        for file in &ctx.changed_files {
            // Skip test files.
            if file.path.contains("test")
                || file.path.contains("tests/")
                || file.path.ends_with("_test.rs")
                || file.path.ends_with("_test.py")
                || file.path.ends_with(".test.ts")
                || file.path.ends_with(".test.tsx")
                || file.path.ends_with(".spec.ts")
                || file.path.ends_with(".spec.tsx")
            {
                continue;
            }

            let content = match &file.content {
                Some(c) => c,
                None => continue,
            };

            for (line_idx, line) in content.lines().enumerate() {
                if self.re.is_match(line) {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        check_name: self.name().to_string(),
                        message: format!(
                            ".unwrap() call at line {} — consider using ? or .expect()",
                            line_idx + 1
                        ),
                        file_path: Some(file.path.clone()),
                        line: Some((line_idx + 1) as u32),
                        symbol: None,
                    });
                }
            }
        }

        findings
    }
}

// ─── error-handling-preserved ────────────────────────────────────────────

/// Detects functions whose signature previously returned `Result` but no
/// longer does after the changeset.
pub struct ErrorHandlingPreserved;

impl ErrorHandlingPreserved {
    pub fn new() -> Self {
        Self
    }
}

impl SemanticCheck for ErrorHandlingPreserved {
    fn name(&self) -> &str {
        "error-handling-preserved"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        use dk_core::types::SymbolKind;
        use std::collections::HashMap;

        let mut findings = Vec::new();

        // Build a map of before functions by qualified_name that return Result.
        let before_result_fns: HashMap<&str, &str> = ctx
            .before_symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .filter(|s| {
                s.signature
                    .as_deref()
                    .map(|sig| sig.contains("Result"))
                    .unwrap_or(false)
            })
            .map(|s| {
                (
                    s.qualified_name.as_str(),
                    s.signature.as_deref().unwrap_or(""),
                )
            })
            .collect();

        // Check the after symbols.
        for after_sym in &ctx.after_symbols {
            if after_sym.kind != SymbolKind::Function {
                continue;
            }
            if let Some(_before_sig) = before_result_fns.get(after_sym.qualified_name.as_str()) {
                let after_has_result = after_sym
                    .signature
                    .as_deref()
                    .map(|sig| sig.contains("Result"))
                    .unwrap_or(false);

                if !after_has_result {
                    findings.push(Finding {
                        severity: Severity::Error,
                        check_name: self.name().to_string(),
                        message: format!(
                            "function '{}' previously returned Result but no longer does",
                            after_sym.qualified_name
                        ),
                        file_path: Some(after_sym.file_path.to_string_lossy().to_string()),
                        line: None,
                        symbol: Some(after_sym.qualified_name.clone()),
                    });
                }
            }
        }

        findings
    }
}

/// Returns all 3 safety checks.
pub fn safety_checks() -> Vec<Box<dyn SemanticCheck>> {
    vec![
        Box::new(NoUnsafeAdded::new()),
        Box::new(NoUnwrapAdded::new()),
        Box::new(ErrorHandlingPreserved::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::Severity;
    use super::super::checks::{ChangedFile, CheckContext};

    fn empty_context() -> CheckContext {
        CheckContext {
            before_symbols: vec![],
            after_symbols: vec![],
            before_call_graph: vec![],
            after_call_graph: vec![],
            before_deps: vec![],
            after_deps: vec![],
            changed_files: vec![],
        }
    }

    #[test]
    fn test_no_unsafe_detects_block() {
        let mut ctx = empty_context();
        ctx.changed_files.push(ChangedFile {
            path: "src/lib.rs".into(),
            content: Some("fn foo() {\n    unsafe {\n        ptr::read(p)\n    }\n}".into()),
        });

        let check = NoUnsafeAdded::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].line, Some(2));
    }

    #[test]
    fn test_no_unsafe_clean_file() {
        let mut ctx = empty_context();
        ctx.changed_files.push(ChangedFile {
            path: "src/lib.rs".into(),
            content: Some("fn safe_fn() { let x = 1; }".into()),
        });

        let check = NoUnsafeAdded::new();
        assert!(check.run(&ctx).is_empty());
    }

    #[test]
    fn test_no_unwrap_detects_call() {
        let mut ctx = empty_context();
        ctx.changed_files.push(ChangedFile {
            path: "src/main.rs".into(),
            content: Some("let val = opt.unwrap();".into()),
        });

        let check = NoUnwrapAdded::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn test_no_unwrap_skips_test_files() {
        let mut ctx = empty_context();
        ctx.changed_files.push(ChangedFile {
            path: "tests/integration.rs".into(),
            content: Some("let val = opt.unwrap();".into()),
        });

        let check = NoUnwrapAdded::new();
        assert!(check.run(&ctx).is_empty());
    }

    #[test]
    fn test_error_handling_preserved_detects_removal() {
        use dk_core::types::{Span, Symbol, SymbolKind, Visibility};
        use uuid::Uuid;

        let sym_id = Uuid::new_v4();
        let mut ctx = empty_context();

        ctx.before_symbols.push(Symbol {
            id: sym_id,
            name: "process".into(),
            qualified_name: "crate::process".into(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            file_path: "src/lib.rs".into(),
            span: Span { start_byte: 0, end_byte: 100 },
            signature: Some("fn process() -> Result<(), Error>".into()),
            doc_comment: None,
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        });

        ctx.after_symbols.push(Symbol {
            id: sym_id,
            name: "process".into(),
            qualified_name: "crate::process".into(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            file_path: "src/lib.rs".into(),
            span: Span { start_byte: 0, end_byte: 80 },
            signature: Some("fn process() -> ()".into()),
            doc_comment: None,
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        });

        let check = ErrorHandlingPreserved::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(findings[0].message.contains("Result"));
    }
}

use std::collections::{HashMap, HashSet};

use dk_core::types::{SymbolKind, Visibility};

use crate::findings::{Finding, Severity};

use super::checks::{CheckContext, SemanticCheck};

// ─── no-public-removal ───────────────────────────────────────────────────

/// Flags public symbols that existed before but are absent after the change
/// (by `qualified_name`), indicating a breaking API removal.
pub struct NoPublicRemoval;

impl NoPublicRemoval {
    pub fn new() -> Self {
        Self
    }
}

impl SemanticCheck for NoPublicRemoval {
    fn name(&self) -> &str {
        "no-public-removal"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        let after_names: HashSet<&str> = ctx
            .after_symbols
            .iter()
            .map(|s| s.qualified_name.as_str())
            .collect();

        for before_sym in &ctx.before_symbols {
            if before_sym.visibility != Visibility::Public {
                continue;
            }
            if !after_names.contains(before_sym.qualified_name.as_str()) {
                findings.push(Finding {
                    severity: Severity::Error,
                    check_name: self.name().to_string(),
                    message: format!(
                        "public {} '{}' was removed",
                        before_sym.kind, before_sym.qualified_name
                    ),
                    file_path: Some(before_sym.file_path.to_string_lossy().to_string()),
                    line: None,
                    symbol: Some(before_sym.qualified_name.clone()),
                });
            }
        }

        findings
    }
}

// ─── signature-stable ────────────────────────────────────────────────────

/// Flags public symbols whose signature changed between before and after,
/// indicating a potential breaking change in the API contract.
pub struct SignatureStable;

impl SignatureStable {
    pub fn new() -> Self {
        Self
    }
}

impl SemanticCheck for SignatureStable {
    fn name(&self) -> &str {
        "signature-stable"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Build before map: qualified_name → signature.
        let before_sigs: HashMap<&str, Option<&str>> = ctx
            .before_symbols
            .iter()
            .filter(|s| s.visibility == Visibility::Public)
            .map(|s| (s.qualified_name.as_str(), s.signature.as_deref()))
            .collect();

        for after_sym in &ctx.after_symbols {
            if after_sym.visibility != Visibility::Public {
                continue;
            }

            if let Some(before_sig) = before_sigs.get(after_sym.qualified_name.as_str()) {
                let after_sig = after_sym.signature.as_deref();
                if *before_sig != after_sig {
                    findings.push(Finding {
                        severity: Severity::Error,
                        check_name: self.name().to_string(),
                        message: format!(
                            "public {} '{}' signature changed: {:?} → {:?}",
                            after_sym.kind,
                            after_sym.qualified_name,
                            before_sig,
                            after_sig
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

// ─── trait-impl-complete ─────────────────────────────────────────────────

/// Detects impl blocks that lost methods compared to the before state.
/// Groups symbols by `parent` (SymbolId) and compares method counts.
pub struct TraitImplComplete;

impl TraitImplComplete {
    pub fn new() -> Self {
        Self
    }
}

impl SemanticCheck for TraitImplComplete {
    fn name(&self) -> &str {
        "trait-impl-complete"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        use dk_core::types::SymbolId;

        let mut findings = Vec::new();

        // Build before: parent_id → set of method qualified_names.
        let mut before_impl_methods: HashMap<SymbolId, HashSet<&str>> = HashMap::new();
        for sym in &ctx.before_symbols {
            if sym.kind == SymbolKind::Function {
                if let Some(parent_id) = sym.parent {
                    before_impl_methods
                        .entry(parent_id)
                        .or_default()
                        .insert(sym.qualified_name.as_str());
                }
            }
        }

        // Build after: parent_id → set of method qualified_names.
        let mut after_impl_methods: HashMap<SymbolId, HashSet<&str>> = HashMap::new();
        for sym in &ctx.after_symbols {
            if sym.kind == SymbolKind::Function {
                if let Some(parent_id) = sym.parent {
                    after_impl_methods
                        .entry(parent_id)
                        .or_default()
                        .insert(sym.qualified_name.as_str());
                }
            }
        }

        // Find impl blocks that lost methods.
        for (parent_id, before_methods) in &before_impl_methods {
            let after_methods = after_impl_methods.get(parent_id);
            let after_set = after_methods.cloned().unwrap_or_default();

            let lost: Vec<&str> = before_methods
                .difference(&after_set)
                .copied()
                .collect();

            if !lost.is_empty() {
                // Try to find the parent symbol name for a better message.
                let parent_name = ctx
                    .before_symbols
                    .iter()
                    .find(|s| s.id == *parent_id)
                    .map(|s| s.qualified_name.as_str())
                    .unwrap_or("unknown");

                findings.push(Finding {
                    severity: Severity::Warning,
                    check_name: self.name().to_string(),
                    message: format!(
                        "impl block '{}' lost {} method(s): {}",
                        parent_name,
                        lost.len(),
                        lost.join(", ")
                    ),
                    file_path: None,
                    line: None,
                    symbol: Some(parent_name.to_string()),
                });
            }
        }

        findings
    }
}

/// Returns all 3 compatibility checks.
pub fn compat_checks() -> Vec<Box<dyn SemanticCheck>> {
    vec![
        Box::new(NoPublicRemoval::new()),
        Box::new(SignatureStable::new()),
        Box::new(TraitImplComplete::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::checks::CheckContext;
    use dk_core::types::{Span, Symbol, SymbolKind, Visibility};
    use uuid::Uuid;

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

    fn make_sym(name: &str, vis: Visibility, sig: Option<&str>, parent: Option<Uuid>) -> Symbol {
        Symbol {
            id: Uuid::new_v4(),
            name: name.split("::").last().unwrap_or(name).into(),
            qualified_name: name.into(),
            kind: SymbolKind::Function,
            visibility: vis,
            file_path: "src/lib.rs".into(),
            span: Span { start_byte: 0, end_byte: 100 },
            signature: sig.map(String::from),
            doc_comment: None,
            parent,
            last_modified_by: None,
            last_modified_intent: None,
        }
    }

    #[test]
    fn test_no_public_removal_detects() {
        let mut ctx = empty_context();
        ctx.before_symbols.push(make_sym("crate::foo", Visibility::Public, None, None));
        // after_symbols is empty — foo was removed.

        let check = NoPublicRemoval::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn test_no_public_removal_ignores_private() {
        let mut ctx = empty_context();
        ctx.before_symbols.push(make_sym("crate::internal", Visibility::Private, None, None));

        let check = NoPublicRemoval::new();
        assert!(check.run(&ctx).is_empty());
    }

    #[test]
    fn test_signature_stable_detects_change() {
        let mut ctx = empty_context();
        ctx.before_symbols.push(make_sym(
            "crate::process",
            Visibility::Public,
            Some("fn process(x: u32) -> u32"),
            None,
        ));
        ctx.after_symbols.push(make_sym(
            "crate::process",
            Visibility::Public,
            Some("fn process(x: u32, y: u32) -> u32"),
            None,
        ));

        let check = SignatureStable::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn test_signature_stable_no_change() {
        let mut ctx = empty_context();
        let sig = "fn process(x: u32) -> u32";
        ctx.before_symbols.push(make_sym("crate::process", Visibility::Public, Some(sig), None));
        ctx.after_symbols.push(make_sym("crate::process", Visibility::Public, Some(sig), None));

        let check = SignatureStable::new();
        assert!(check.run(&ctx).is_empty());
    }

    #[test]
    fn test_trait_impl_complete_detects_lost_method() {
        let parent_id = Uuid::new_v4();
        let mut ctx = empty_context();

        // Parent symbol (the impl block).
        let mut parent_sym = make_sym("crate::MyStruct", Visibility::Public, None, None);
        parent_sym.id = parent_id;
        parent_sym.kind = SymbolKind::Impl;
        ctx.before_symbols.push(parent_sym.clone());
        ctx.after_symbols.push(parent_sym);

        // Before: two methods.
        ctx.before_symbols.push(make_sym("crate::MyStruct::method_a", Visibility::Public, None, Some(parent_id)));
        ctx.before_symbols.push(make_sym("crate::MyStruct::method_b", Visibility::Public, None, Some(parent_id)));

        // After: only one method.
        ctx.after_symbols.push(make_sym("crate::MyStruct::method_a", Visibility::Public, None, Some(parent_id)));

        let check = TraitImplComplete::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(findings[0].message.contains("method_b"));
    }
}

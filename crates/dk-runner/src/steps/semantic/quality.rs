use std::collections::{HashMap, HashSet};

use regex::Regex;

use dk_core::types::{SymbolId, Visibility, SymbolKind};

use crate::findings::{Finding, Severity};

use super::checks::{CheckContext, SemanticCheck};

// ─── complexity-limit ────────────────────────────────────────────────────

/// Counts nesting depth of branching constructs (if, else, match, for,
/// while, loop) in changed files and flags functions that exceed a
/// configurable threshold.
pub struct ComplexityLimit {
    threshold: usize,
    branch_re: Regex,
}

impl ComplexityLimit {
    pub fn new() -> Self {
        Self::with_threshold(10)
    }

    pub fn with_threshold(threshold: usize) -> Self {
        Self {
            threshold,
            // Match branching keywords at word boundaries. We count occurrences
            // per file as a rough proxy for McCabe-like complexity.
            branch_re: Regex::new(
                r"\b(if|else|match|for|while|loop)\b"
            )
            .expect("invalid regex"),
        }
    }
}

impl SemanticCheck for ComplexityLimit {
    fn name(&self) -> &str {
        "complexity-limit"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        let fn_re = Regex::new(r"\b(pub\s+)?(async\s+)?fn\s+(\w+)").expect("invalid fn regex");

        for file in &ctx.changed_files {
            let content = match &file.content {
                Some(c) => c,
                None => continue,
            };

            // Track per-function complexity by detecting `fn` declarations
            // and measuring branching depth within each function's scope.
            let mut current_fn: Option<(String, usize)> = None; // (name, start_line)
            let mut fn_depth: usize = 0;       // brace depth within current function
            let mut branch_depth: usize = 0;   // branching nesting within current function
            let mut max_branch_depth: usize = 0;
            let mut max_branch_line: usize = 0;
            let mut in_function = false;

            for (line_idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();

                // Detect function start
                if let Some(caps) = fn_re.captures(trimmed) {
                    if !in_function {
                        let fn_name = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
                        current_fn = Some((fn_name, line_idx + 1));
                        fn_depth = 0;
                        branch_depth = 0;
                        max_branch_depth = 0;
                        max_branch_line = line_idx + 1;
                        in_function = true;
                    }
                }

                if in_function {
                    // Count opening braces
                    fn_depth += trimmed.matches('{').count();

                    // Track branching keywords for complexity
                    if self.branch_re.is_match(trimmed) {
                        branch_depth += 1;
                        if branch_depth > max_branch_depth {
                            max_branch_depth = branch_depth;
                            max_branch_line = line_idx + 1;
                        }
                    }

                    // Count closing braces
                    let close_count = trimmed.matches('}').count();
                    if close_count > 0 {
                        // Reduce branch depth for closing braces (heuristic)
                        if branch_depth > 0 && trimmed.starts_with('}') {
                            branch_depth = branch_depth.saturating_sub(1);
                        }
                    }
                    fn_depth = fn_depth.saturating_sub(close_count);

                    // Function ended when brace depth returns to 0
                    if fn_depth == 0 && current_fn.is_some() {
                        if max_branch_depth > self.threshold {
                            let (fn_name, fn_start) = current_fn.as_ref().unwrap();
                            findings.push(Finding {
                                severity: Severity::Warning,
                                check_name: self.name().to_string(),
                                message: format!(
                                    "function '{}' (line {}) has branching complexity {} exceeding threshold {} (deepest near line {})",
                                    fn_name, fn_start, max_branch_depth, self.threshold, max_branch_line
                                ),
                                file_path: Some(file.path.clone()),
                                line: Some(max_branch_line as u32),
                                symbol: Some(fn_name.clone()),
                            });
                        }
                        current_fn = None;
                        in_function = false;
                    }
                }
            }
        }

        findings
    }
}

// ─── no-dependency-cycles ────────────────────────────────────────────────

/// Performs DFS cycle detection on the call graph and flags any cycles found.
pub struct NoDependencyCycles;

impl NoDependencyCycles {
    pub fn new() -> Self {
        Self
    }
}

impl SemanticCheck for NoDependencyCycles {
    fn name(&self) -> &str {
        "no-dependency-cycles"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Build adjacency list from the call graph.
        let mut adjacency: HashMap<SymbolId, Vec<SymbolId>> = HashMap::new();
        for edge in &ctx.before_call_graph {
            adjacency.entry(edge.caller).or_default().push(edge.callee);
        }
        // Also include after call graph if available.
        for edge in &ctx.after_call_graph {
            adjacency.entry(edge.caller).or_default().push(edge.callee);
        }

        if adjacency.is_empty() {
            return findings;
        }

        // DFS cycle detection.
        let nodes: Vec<SymbolId> = adjacency.keys().copied().collect();
        let mut visited: HashSet<SymbolId> = HashSet::new();
        let mut on_stack: HashSet<SymbolId> = HashSet::new();
        let mut cycles: Vec<Vec<SymbolId>> = Vec::new();

        for &node in &nodes {
            if !visited.contains(&node) {
                let mut path = Vec::new();
                dfs_detect_cycle(
                    node,
                    &adjacency,
                    &mut visited,
                    &mut on_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        for cycle in &cycles {
            let cycle_ids: Vec<String> = cycle.iter().map(|id| id.to_string()).collect();
            findings.push(Finding {
                severity: Severity::Error,
                check_name: self.name().to_string(),
                message: format!(
                    "dependency cycle detected involving {} symbol(s): {}",
                    cycle.len(),
                    cycle_ids.join(" -> ")
                ),
                file_path: None,
                line: None,
                symbol: None,
            });
        }

        findings
    }
}

fn dfs_detect_cycle(
    node: SymbolId,
    adj: &HashMap<SymbolId, Vec<SymbolId>>,
    visited: &mut HashSet<SymbolId>,
    on_stack: &mut HashSet<SymbolId>,
    path: &mut Vec<SymbolId>,
    cycles: &mut Vec<Vec<SymbolId>>,
) {
    visited.insert(node);
    on_stack.insert(node);
    path.push(node);

    if let Some(neighbors) = adj.get(&node) {
        for &next in neighbors {
            if !visited.contains(&next) {
                dfs_detect_cycle(next, adj, visited, on_stack, path, cycles);
            } else if on_stack.contains(&next) {
                // Found a cycle: extract the cycle from the path.
                if let Some(pos) = path.iter().position(|&n| n == next) {
                    let cycle: Vec<SymbolId> = path[pos..].to_vec();
                    cycles.push(cycle);
                }
            }
        }
    }

    path.pop();
    on_stack.remove(&node);
}

// ─── dead-code-detection ─────────────────────────────────────────────────

/// Detects private functions with zero incoming calls in the call graph.
pub struct DeadCodeDetection;

impl DeadCodeDetection {
    pub fn new() -> Self {
        Self
    }
}

impl SemanticCheck for DeadCodeDetection {
    fn name(&self) -> &str {
        "dead-code-detection"
    }

    fn run(&self, ctx: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Collect all callee symbol IDs from the call graph.
        let mut called_symbols: HashSet<SymbolId> = HashSet::new();
        for edge in &ctx.before_call_graph {
            called_symbols.insert(edge.callee);
        }
        for edge in &ctx.after_call_graph {
            called_symbols.insert(edge.callee);
        }

        // Check after_symbols (current state) for private functions with zero callers.
        for sym in &ctx.after_symbols {
            if sym.kind != SymbolKind::Function {
                continue;
            }
            if sym.visibility != Visibility::Private {
                continue;
            }
            // Skip "main" functions and test helpers.
            if sym.name == "main" || sym.name.starts_with("test") {
                continue;
            }

            if !called_symbols.contains(&sym.id) {
                findings.push(Finding {
                    severity: Severity::Info,
                    check_name: self.name().to_string(),
                    message: format!(
                        "private function '{}' has no callers and may be dead code",
                        sym.qualified_name
                    ),
                    file_path: Some(sym.file_path.to_string_lossy().to_string()),
                    line: None,
                    symbol: Some(sym.qualified_name.clone()),
                });
            }
        }

        findings
    }
}

/// Returns all 3 quality checks.
pub fn quality_checks() -> Vec<Box<dyn SemanticCheck>> {
    vec![
        Box::new(ComplexityLimit::new()),
        Box::new(NoDependencyCycles::new()),
        Box::new(DeadCodeDetection::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::checks::{ChangedFile, CheckContext};
    use dk_core::types::{CallEdge, CallKind, Span, Symbol};
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

    fn make_fn(name: &str, vis: Visibility) -> Symbol {
        Symbol {
            id: Uuid::new_v4(),
            name: name.split("::").last().unwrap_or(name).into(),
            qualified_name: name.into(),
            kind: SymbolKind::Function,
            visibility: vis,
            file_path: "src/lib.rs".into(),
            span: Span { start_byte: 0, end_byte: 100 },
            signature: None,
            doc_comment: None,
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        }
    }

    #[test]
    fn test_complexity_under_threshold() {
        let mut ctx = empty_context();
        ctx.changed_files.push(ChangedFile {
            path: "src/main.rs".into(),
            content: Some("fn simple() {\n    if true {\n        return;\n    }\n}".into()),
        });

        let check = ComplexityLimit::with_threshold(10);
        assert!(check.run(&ctx).is_empty());
    }

    #[test]
    fn test_complexity_over_threshold() {
        let mut ctx = empty_context();
        // Create deeply nested code inside a function that exceeds threshold of 2.
        let code = "\
fn complex() {
    if a {
        if b {
            if c {
                x
            }
        }
    }
}";
        ctx.changed_files.push(ChangedFile {
            path: "src/main.rs".into(),
            content: Some(code.into()),
        });

        let check = ComplexityLimit::with_threshold(2);
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(findings[0].symbol.is_some(), "should report the function name");
    }

    #[test]
    fn test_no_dependency_cycles_clean() {
        let repo_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let mut ctx = empty_context();
        ctx.before_call_graph.push(CallEdge {
            id: Uuid::new_v4(),
            repo_id,
            caller: a,
            callee: b,
            kind: CallKind::DirectCall,
        });

        let check = NoDependencyCycles::new();
        assert!(check.run(&ctx).is_empty());
    }

    #[test]
    fn test_no_dependency_cycles_detects_cycle() {
        let repo_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let mut ctx = empty_context();
        ctx.before_call_graph.push(CallEdge {
            id: Uuid::new_v4(),
            repo_id,
            caller: a,
            callee: b,
            kind: CallKind::DirectCall,
        });
        ctx.before_call_graph.push(CallEdge {
            id: Uuid::new_v4(),
            repo_id,
            caller: b,
            callee: a,
            kind: CallKind::DirectCall,
        });

        let check = NoDependencyCycles::new();
        let findings = check.run(&ctx);
        assert!(!findings.is_empty());
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn test_dead_code_detects_uncalled_private() {
        let mut ctx = empty_context();
        let sym = make_fn("crate::helper", Visibility::Private);
        // No call edges point to this symbol.
        ctx.after_symbols.push(sym);

        let check = DeadCodeDetection::new();
        let findings = check.run(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
    }

    #[test]
    fn test_dead_code_ignores_public() {
        let mut ctx = empty_context();
        ctx.after_symbols.push(make_fn("crate::api_handler", Visibility::Public));

        let check = DeadCodeDetection::new();
        assert!(check.run(&ctx).is_empty());
    }

    #[test]
    fn test_dead_code_ignores_called_private() {
        let repo_id = Uuid::new_v4();
        let mut ctx = empty_context();
        let sym = make_fn("crate::helper", Visibility::Private);
        let sym_id = sym.id;
        ctx.after_symbols.push(sym);

        // Add a call edge pointing to this symbol.
        ctx.before_call_graph.push(CallEdge {
            id: Uuid::new_v4(),
            repo_id,
            caller: Uuid::new_v4(),
            callee: sym_id,
            kind: CallKind::DirectCall,
        });

        let check = DeadCodeDetection::new();
        assert!(check.run(&ctx).is_empty());
    }
}

//! TypeScript/JavaScript language configuration for the query-driven parser.

use crate::parser::engine::QueryDrivenParser;
use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use crate::parser::LanguageParser;
use dk_core::{FileAnalysis, Import, RawCallEdge, Result, Symbol, SymbolKind, TypeInfo, Visibility};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Language;

/// TypeScript language configuration for [`QueryDrivenParser`].
///
/// Uses the TSX grammar (a superset of TypeScript) so `.ts`, `.tsx`, `.js`,
/// and `.jsx` files are all handled correctly.
pub struct TypeScriptConfig;

impl LanguageConfig for TypeScriptConfig {
    fn language(&self) -> Language {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/typescript_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/typescript_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/typescript_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, modifiers: Option<&str>, _name: &str) -> Visibility {
        // If @modifiers captured text (meaning the declaration was inside an
        // export_statement), the symbol is Public. Otherwise Private.
        match modifiers {
            Some(_) => Visibility::Public,
            None => Visibility::Private,
        }
    }

    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, source: &[u8]) {
        // For expression_statement nodes (captured as @definition.expression),
        // derive a meaningful name from the call structure.
        // e.g. `router.get("/health", ...)` → "router.get:/health"
        //       `app.use(middleware)` → "app.use"
        //       `module.exports = ...` → "module.exports"
        //       `export default router` → "export default router"
        if sym.kind == SymbolKind::Const && node.kind() == "call_expression" {
            // The @definition.expression captures the call_expression inside
            // an expression_statement. Walk up to get the expression_statement
            // span and doc comments.
            if let Some(parent) = node.parent() {
                if parent.kind() == "expression_statement" {
                    sym.span = dk_core::Span {
                        start_byte: parent.start_byte() as u32,
                        end_byte: parent.end_byte() as u32,
                    };
                    // Collect doc comments from the expression_statement's
                    // preceding siblings (the engine only looked at the
                    // call_expression's siblings, which don't include comments).
                    if sym.doc_comment.is_none() {
                        sym.doc_comment = Self::collect_preceding_comments(&parent, source);
                    }
                }
            }

            // Derive a name from the call: func_text + optional first string arg
            if let Some(func_node) = node.child_by_field_name("function") {
                let func_text = std::str::from_utf8(
                    &source[func_node.start_byte()..func_node.end_byte()],
                )
                .unwrap_or("")
                .to_string();

                // Look for the first string argument to append as a path
                let name = if let Some(args) = node.child_by_field_name("arguments") {
                    let mut path_name = None;
                    let mut cursor = args.walk();
                    for arg_child in args.children(&mut cursor) {
                        if arg_child.kind() == "string" || arg_child.kind() == "template_string" {
                            let raw = std::str::from_utf8(
                                &source[arg_child.start_byte()..arg_child.end_byte()],
                            )
                            .unwrap_or("");
                            let path = raw
                                .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                                .to_string();
                            path_name = Some(format!("{func_text}:{path}"));
                            break;
                        }
                    }
                    path_name.unwrap_or(func_text)
                } else {
                    func_text
                };

                sym.name = name.clone();
                sym.qualified_name = name;
            }
        } else if sym.kind == SymbolKind::Const && node.kind() == "assignment_expression" {
            // Assignment: use the left-hand side as the name
            if let Some(parent) = node.parent() {
                if parent.kind() == "expression_statement" {
                    sym.span = dk_core::Span {
                        start_byte: parent.start_byte() as u32,
                        end_byte: parent.end_byte() as u32,
                    };
                    if sym.doc_comment.is_none() {
                        sym.doc_comment = Self::collect_preceding_comments(&parent, source);
                    }
                }
            }
        } else if node.kind() == "export_statement" {
            // `export default <expr>` — prefix the name
            let name = format!("export default {}", sym.name);
            sym.name = name.clone();
            sym.qualified_name = name;
        }
    }

    fn is_external_import(&self, module_path: &str) -> bool {
        !module_path.starts_with('.') && !module_path.starts_with('/')
    }
}

impl TypeScriptConfig {
    /// Collect `//` comment lines immediately preceding a node.
    ///
    /// Preserves the full comment text (including the `//` prefix) so
    /// that AST merge can reconstruct valid TypeScript.
    fn collect_preceding_comments(
        node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        let mut lines = Vec::new();
        let mut sibling = node.prev_sibling();

        while let Some(prev) = sibling {
            if prev.kind() == "comment" {
                let text = std::str::from_utf8(&source[prev.start_byte()..prev.end_byte()])
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if text.starts_with("//") {
                    lines.push(text);
                    sibling = prev.prev_sibling();
                    continue;
                }
            }
            break;
        }

        if lines.is_empty() {
            None
        } else {
            lines.reverse();
            Some(lines.join("\n"))
        }
    }
}

/// TypeScript parser wrapper that adds qualified-name deduplication.
///
/// Multiple top-level expressions can produce the same `qualified_name`
/// (e.g. several `app.use(...)` calls). This wrapper calls the generic
/// [`QueryDrivenParser`] and then appends `#N` suffixes to duplicates so
/// every symbol has a unique key for the AST merge BTreeMap.
pub struct TypeScriptParser {
    inner: QueryDrivenParser,
}

impl TypeScriptParser {
    /// Create a new TypeScript query-driven parser.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: QueryDrivenParser::new(Box::new(TypeScriptConfig))?,
        })
    }
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new().expect("TypeScript parser initialization should not fail")
    }
}

impl TypeScriptParser {
    /// Filter nested symbols and deduplicate qualified names.
    fn dedup_symbols(mut symbols: Vec<Symbol>) -> Vec<Symbol> {
        // Filter out nested symbols: if one symbol's span is entirely
        // inside another's, remove the inner one. This prevents extracting
        // `res.json(...)` or `const note = ...` from inside arrow functions.
        let ranges: Vec<(u32, u32)> = symbols
            .iter()
            .map(|s| (s.span.start_byte, s.span.end_byte))
            .collect();
        symbols.retain(|sym| {
            let start = sym.span.start_byte;
            let end = sym.span.end_byte;
            !ranges.iter().any(|(rs, re)| *rs < start && end < *re)
        });

        // Deduplicate qualified_names: append #N for duplicates.
        let mut seen: HashMap<String, usize> = HashMap::new();
        for sym in &mut symbols {
            let count = seen.entry(sym.qualified_name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                sym.qualified_name = format!("{}#{}", sym.qualified_name, count);
                sym.name = sym.qualified_name.clone();
            }
        }

        symbols
    }
}

impl LanguageParser for TypeScriptParser {
    fn extensions(&self) -> &[&str] {
        self.inner.extensions()
    }

    fn extract_symbols(&self, source: &[u8], file_path: &Path) -> Result<Vec<Symbol>> {
        let symbols = self.inner.extract_symbols(source, file_path)?;
        Ok(Self::dedup_symbols(symbols))
    }

    fn extract_calls(&self, source: &[u8], file_path: &Path) -> Result<Vec<RawCallEdge>> {
        self.inner.extract_calls(source, file_path)
    }

    fn extract_types(&self, source: &[u8], file_path: &Path) -> Result<Vec<TypeInfo>> {
        self.inner.extract_types(source, file_path)
    }

    fn extract_imports(&self, source: &[u8], file_path: &Path) -> Result<Vec<Import>> {
        self.inner.extract_imports(source, file_path)
    }

    fn parse_file(&self, source: &[u8], file_path: &Path) -> Result<FileAnalysis> {
        let mut analysis = self.inner.parse_file(source, file_path)?;
        analysis.symbols = Self::dedup_symbols(analysis.symbols);
        Ok(analysis)
    }
}

//! Python language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::{Symbol, Visibility};
use tree_sitter::Language;

/// Python language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct PythonConfig;

impl LanguageConfig for PythonConfig {
    fn language(&self) -> Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/python_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/python_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/python_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::Hash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, name: &str) -> Visibility {
        if name.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, source: &[u8]) {
        // 1. Decorated definitions: expand span to include decorator(s).
        //    If this function/class is inside a `decorated_definition`, use
        //    the parent's full span and its first line as the signature.
        if let Some(parent) = node.parent() {
            if parent.kind() == "decorated_definition" {
                sym.span = dk_core::Span {
                    start_byte: parent.start_byte() as u32,
                    end_byte: parent.end_byte() as u32,
                };
                // Use the decorator's first line as the signature.
                let text = std::str::from_utf8(&source[parent.start_byte()..parent.end_byte()])
                    .unwrap_or("");
                if let Some(first_line) = text.lines().next() {
                    let trimmed = first_line.trim();
                    if !trimmed.is_empty() {
                        sym.signature = Some(trimmed.to_string());
                    }
                }
            }
        }

        // 2. Docstrings: for functions and classes, check the body for a
        //    leading expression_statement containing a string (triple-quoted).
        //    Docstrings take priority over `# comment` blocks collected by
        //    the engine's `collect_doc_comments`.
        if matches!(
            sym.kind,
            dk_core::SymbolKind::Function | dk_core::SymbolKind::Class
        ) {
            if let Some(docstring) = Self::extract_docstring(node, source) {
                sym.doc_comment = Some(docstring);
            }
        }
    }

    fn is_external_import(&self, module_path: &str) -> bool {
        !module_path.starts_with('.')
    }
}

impl PythonConfig {
    /// Extract a docstring from a function or class body.
    ///
    /// In Python, a docstring is the first statement in the body if it is an
    /// `expression_statement` containing a `string` node.
    fn extract_docstring(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let body = node.child_by_field_name("body")?;
        let first_stmt = body.named_child(0)?;

        if first_stmt.kind() == "expression_statement" {
            let expr = first_stmt.child(0)?;
            if expr.kind() == "string" {
                let raw =
                    std::str::from_utf8(&source[expr.start_byte()..expr.end_byte()]).unwrap_or("");
                let content = raw
                    .strip_prefix("\"\"\"")
                    .and_then(|s| s.strip_suffix("\"\"\""))
                    .or_else(|| raw.strip_prefix("'''").and_then(|s| s.strip_suffix("'''")))
                    .unwrap_or(raw);
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }

        None
    }
}

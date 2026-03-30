//! Generic query-driven parser engine.
//!
//! [`QueryDrivenParser`] uses tree-sitter's Query API to extract symbols,
//! calls, and imports from any language. Each language supplies a
//! [`LanguageConfig`](super::lang_config::LanguageConfig) with its grammar
//! and S-expression queries; the engine compiles and runs them.

use super::lang_config::{CommentStyle, LanguageConfig};
use super::LanguageParser;
use dk_core::{CallKind, Error, Import, RawCallEdge, Result, Span, Symbol, TypeInfo};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor};
use uuid::Uuid;

/// A language-agnostic parser driven by tree-sitter queries.
///
/// One instance handles a single language, configured via [`LanguageConfig`].
pub struct QueryDrivenParser {
    config: Box<dyn LanguageConfig>,
    symbols_query: Query,
    calls_query: Option<Query>,
    imports_query: Option<Query>,
}

impl QueryDrivenParser {
    /// Create a new parser from a language configuration.
    ///
    /// Compiles the S-expression query strings from `config` into
    /// [`Query`] objects. Returns [`Error::ParseError`] if compilation fails.
    pub fn new(config: Box<dyn LanguageConfig>) -> Result<Self> {
        let lang = config.language();

        let symbols_query = Query::new(&lang, config.symbols_query()).map_err(|e| {
            Error::ParseError(format!("Failed to compile symbols query: {e}"))
        })?;

        let calls_query = {
            let q = config.calls_query();
            if q.is_empty() {
                None
            } else {
                Some(Query::new(&lang, q).map_err(|e| {
                    Error::ParseError(format!("Failed to compile calls query: {e}"))
                })?)
            }
        };

        let imports_query = {
            let q = config.imports_query();
            if q.is_empty() {
                None
            } else {
                Some(Query::new(&lang, q).map_err(|e| {
                    Error::ParseError(format!("Failed to compile imports query: {e}"))
                })?)
            }
        };

        Ok(Self {
            config,
            symbols_query,
            calls_query,
            imports_query,
        })
    }

    // ── Helpers ──

    /// Parse source bytes into a tree-sitter syntax tree.
    fn parse_tree(&self, source: &[u8]) -> Result<tree_sitter::Tree> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.config.language())
            .map_err(|e| Error::ParseError(format!("Failed to set language: {e}")))?;
        parser
            .parse(source, None)
            .ok_or_else(|| Error::ParseError("tree-sitter parse returned None".into()))
    }

    /// Extract the UTF-8 text of a node from the source bytes.
    fn node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
        let bytes = &source[node.start_byte()..node.end_byte()];
        std::str::from_utf8(bytes).unwrap_or("")
    }

    /// Extract the first line of a node's text as its signature.
    fn node_signature(node: &Node, source: &[u8]) -> Option<String> {
        let text = Self::node_text(node, source);
        let first_line = text.lines().next()?;
        let trimmed = first_line.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Collect doc-comment lines immediately preceding `node`.
    ///
    /// Walks backwards through previous siblings, collecting lines that
    /// match the configured [`CommentStyle`].
    fn collect_doc_comments(&self, node: &Node, source: &[u8]) -> Option<String> {
        let comment_prefix = match self.config.comment_style() {
            CommentStyle::TripleSlash => "///",
            CommentStyle::Hash => "#",
            CommentStyle::SlashSlash => "//",
        };

        let mut lines = Vec::new();
        let mut sibling = node.prev_sibling();

        while let Some(prev) = sibling {
            if prev.kind() == "line_comment" || prev.kind() == "comment" {
                let text = Self::node_text(&prev, source).trim();
                if text.starts_with(comment_prefix) {
                    // Strip the prefix (and optional trailing space)
                    let content = text.strip_prefix(comment_prefix).unwrap_or(text);
                    let content = content.strip_prefix(' ').unwrap_or(content);
                    lines.push(content.to_string());
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

    /// Walk parent nodes to find the name of the enclosing function.
    ///
    /// Returns `"<module>"` if the node is at the top level.
    fn enclosing_function_name(&self, node: &Node, source: &[u8]) -> String {
        let function_kinds = [
            "function_item",
            "function_definition",
            "function_declaration",
            "method_definition",
            "arrow_function",
        ];

        let mut current = node.parent();
        while let Some(parent) = current {
            if function_kinds.contains(&parent.kind()) {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    let name = Self::node_text(&name_node, source);
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            }
            current = parent.parent();
        }
        "<module>".to_string()
    }
}

impl LanguageParser for QueryDrivenParser {
    fn extensions(&self) -> &[&str] {
        self.config.extensions()
    }

    fn extract_symbols(&self, source: &[u8], file_path: &Path) -> Result<Vec<Symbol>> {
        if source.is_empty() {
            return Ok(vec![]);
        }

        let tree = self.parse_tree(source)?;
        let root = tree.root_node();
        let capture_names = self.symbols_query.capture_names();

        let mut cursor = QueryCursor::new();
        let mut symbols = Vec::new();
        let mut matches = cursor.matches(&self.symbols_query, root, source);

        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text: Option<String> = None;
            let mut definition_node: Option<Node> = None;
            let mut kind_suffix: Option<String> = None;
            let mut modifiers_text: Option<String> = None;

            for capture in m.captures {
                let capture_name = capture_names[capture.index as usize];

                if capture_name == "name" {
                    name_text = Some(Self::node_text(&capture.node, source).to_string());
                } else if let Some(suffix) = capture_name.strip_prefix("definition.") {
                    definition_node = Some(capture.node);
                    kind_suffix = Some(suffix.to_string());
                } else if capture_name == "modifiers" {
                    modifiers_text = Some(Self::node_text(&capture.node, source).to_string());
                }
            }

            // We need at least a name and a definition node with a kind suffix.
            let name = match &name_text {
                Some(n) if !n.is_empty() => n.as_str(),
                _ => continue,
            };
            let def_node = match definition_node {
                Some(n) => n,
                None => continue,
            };
            let suffix = match &kind_suffix {
                Some(s) => s.as_str(),
                None => continue,
            };

            let symbol_kind = match self.config.map_capture_to_kind(suffix) {
                Some(k) => k,
                None => continue,
            };

            let visibility = self
                .config
                .resolve_visibility(modifiers_text.as_deref(), name);
            let signature = Self::node_signature(&def_node, source);
            let doc_comment = self.collect_doc_comments(&def_node, source);

            let mut sym = Symbol {
                id: Uuid::new_v4(),
                name: name.to_string(),
                qualified_name: name.to_string(),
                kind: symbol_kind,
                visibility,
                file_path: file_path.to_path_buf(),
                span: Span {
                    start_byte: def_node.start_byte() as u32,
                    end_byte: def_node.end_byte() as u32,
                },
                signature,
                doc_comment,
                parent: None,
                last_modified_by: None,
                last_modified_intent: None,
            };

            self.config.adjust_symbol(&mut sym, &def_node, source);
            symbols.push(sym);
        }

        Ok(symbols)
    }

    fn extract_calls(&self, source: &[u8], _file_path: &Path) -> Result<Vec<RawCallEdge>> {
        if source.is_empty() {
            return Ok(vec![]);
        }

        let calls_query = match &self.calls_query {
            Some(q) => q,
            None => return Ok(vec![]),
        };

        let tree = self.parse_tree(source)?;
        let root = tree.root_node();
        let capture_names = calls_query.capture_names();

        let mut cursor = QueryCursor::new();
        let mut calls = Vec::new();
        let mut matches = cursor.matches(calls_query, root, source);

        while let Some(m) = { matches.advance(); matches.get() } {
            let mut callee_text: Option<String> = None;
            let mut method_callee_text: Option<String> = None;
            let mut call_node: Option<Node> = None;
            let mut first_node: Option<Node> = None;

            for capture in m.captures {
                let capture_name = capture_names[capture.index as usize];

                if first_node.is_none() {
                    first_node = Some(capture.node);
                }

                match capture_name {
                    "callee" => {
                        callee_text =
                            Some(Self::node_text(&capture.node, source).to_string());
                    }
                    "method_callee" => {
                        method_callee_text =
                            Some(Self::node_text(&capture.node, source).to_string());
                    }
                    "call" => call_node = Some(capture.node),
                    _ => {}
                }
            }

            // Determine call kind and callee name.
            let (callee_name, call_kind) = if let Some(method) =
                method_callee_text.filter(|s| !s.is_empty())
            {
                (method, CallKind::MethodCall)
            } else if let Some(direct) = callee_text.filter(|s| !s.is_empty()) {
                (direct, CallKind::DirectCall)
            } else {
                continue;
            };

            // Use the @call node for span, falling back to the first captured node.
            let span_node = call_node
                .or(first_node)
                .expect("match has at least one capture");

            let caller_name = self.enclosing_function_name(&span_node, source);

            calls.push(RawCallEdge {
                caller_name,
                callee_name,
                call_site: Span {
                    start_byte: span_node.start_byte() as u32,
                    end_byte: span_node.end_byte() as u32,
                },
                kind: call_kind,
            });
        }

        Ok(calls)
    }

    fn extract_types(&self, _source: &[u8], _file_path: &Path) -> Result<Vec<TypeInfo>> {
        Ok(vec![])
    }

    fn extract_imports(&self, source: &[u8], _file_path: &Path) -> Result<Vec<Import>> {
        if source.is_empty() {
            return Ok(vec![]);
        }

        let imports_query = match &self.imports_query {
            Some(q) => q,
            None => return Ok(vec![]),
        };

        let tree = self.parse_tree(source)?;
        let root = tree.root_node();
        let capture_names = imports_query.capture_names();

        let mut cursor = QueryCursor::new();
        let mut imports = Vec::new();
        let mut matches = cursor.matches(imports_query, root, source);

        while let Some(m) = { matches.advance(); matches.get() } {
            let mut module_text: Option<String> = None;
            let mut import_name_text: Option<String> = None;
            let mut alias_text: Option<String> = None;

            for capture in m.captures {
                let capture_name = capture_names[capture.index as usize];

                match capture_name {
                    "module" => {
                        let text = Self::node_text(&capture.node, source);
                        // Strip surrounding quotes if present.
                        module_text = Some(
                            text.trim_matches(|c| c == '"' || c == '\'').to_string(),
                        );
                    }
                    "import_name" => {
                        import_name_text =
                            Some(Self::node_text(&capture.node, source).to_string());
                    }
                    "alias" => {
                        alias_text = Some(Self::node_text(&capture.node, source).to_string());
                    }
                    _ => {}
                }
            }

            let module_path = match module_text {
                Some(ref m) if !m.is_empty() => m.clone(),
                _ => continue,
            };

            let imported_name = import_name_text
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    // Derive imported name from the last segment of the module path.
                    module_path
                        .rsplit(|c| c == '/' || c == '.' || c == ':')
                        .next()
                        .unwrap_or(&module_path)
                        .to_string()
                });

            let alias = alias_text.filter(|s| !s.is_empty());

            let is_external = self.config.is_external_import(&module_path);

            imports.push(Import {
                module_path,
                imported_name,
                alias,
                is_external,
            });
        }

        Ok(imports)
    }
}

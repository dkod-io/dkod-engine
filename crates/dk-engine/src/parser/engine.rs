//! Generic query-driven parser engine.
//!
//! [`QueryDrivenParser`] uses tree-sitter's Query API to extract symbols,
//! calls, and imports from any language. Each language supplies a
//! [`LanguageConfig`](super::lang_config::LanguageConfig) with its grammar
//! and S-expression queries; the engine compiles and runs them.

use super::lang_config::{CommentStyle, LanguageConfig};
use super::LanguageParser;
use dk_core::{
    CallKind, Error, FileAnalysis, Import, RawCallEdge, Result, Span, Symbol, TypeInfo,
};
use std::path::Path;
use std::sync::Mutex;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor, Tree};
use uuid::Uuid;

/// A language-agnostic parser driven by tree-sitter queries.
///
/// One instance handles a single language, configured via [`LanguageConfig`].
pub struct QueryDrivenParser {
    config: Box<dyn LanguageConfig>,
    parser: Mutex<Parser>,
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

        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| Error::ParseError(format!("Failed to set language: {e}")))?;

        Ok(Self {
            config,
            parser: Mutex::new(parser),
            symbols_query,
            calls_query,
            imports_query,
        })
    }

    // ── Helpers ──

    /// Parse source bytes into a tree-sitter syntax tree.
    ///
    /// Reuses the cached `Parser` instance to avoid repeated allocation
    /// and language setup.
    fn parse_tree(&self, source: &[u8]) -> Result<tree_sitter::Tree> {
        let mut parser = self.parser.lock().unwrap_or_else(|e| e.into_inner());
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
    /// match the configured [`CommentStyle`]. Preserves the original
    /// comment prefix (e.g. `#`, `///`, `//`, `/** */`) so that AST merge
    /// can reconstruct valid source code.
    ///
    /// **Note:** Unlike the old hand-written parsers (which stripped the
    /// prefix), `Symbol.doc_comment` now includes the raw prefix. This is
    /// intentional — AST merge needs the prefix to reconstruct valid
    /// source. Consumers that display doc comments should strip prefixes
    /// at the presentation layer.
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
                // Skip inline comments: if this comment is on the same line
                // as a preceding non-comment sibling, it belongs to that
                // sibling (e.g. `x = 60  # 60 seconds`), not to our node.
                if let Some(before_comment) = prev.prev_sibling() {
                    if before_comment.kind() != "comment"
                        && before_comment.kind() != "line_comment"
                        && before_comment.end_position().row == prev.start_position().row
                    {
                        break;
                    }
                }

                let text = Self::node_text(&prev, source).trim();
                if text.starts_with(comment_prefix) || text.starts_with("/*") {
                    // Preserve the full comment text including prefix.
                    // The `/*` branch captures JSDoc (`/** ... */`) blocks
                    // for languages using CommentStyle::SlashSlash.
                    lines.push(text.to_string());
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
        let named_function_kinds = [
            "function_item",
            "function_definition",
            "function_declaration",
            "method_definition",
        ];
        // Anonymous function forms whose name comes from an enclosing
        // variable_declarator (e.g. `const fn = function() {}`)
        let anonymous_function_kinds = ["arrow_function", "function_expression", "function"];

        let mut current = node.parent();
        while let Some(parent) = current {
            let kind = parent.kind();
            if named_function_kinds.contains(&kind) {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    let name = Self::node_text(&name_node, source);
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            } else if anonymous_function_kinds.contains(&kind) {
                // Check if assigned to a variable: const foo = function() {}
                if let Some(gp) = parent.parent() {
                    if gp.kind() == "variable_declarator" {
                        if let Some(name_node) = gp.child_by_field_name("name") {
                            let name = Self::node_text(&name_node, source);
                            if !name.is_empty() {
                                return name.to_string();
                            }
                        }
                    }
                }
            }
            current = parent.parent();
        }
        "<module>".to_string()
    }
}

impl QueryDrivenParser {
    /// Extract symbols from an already-parsed tree.
    fn symbols_from_tree(
        &self,
        tree: &Tree,
        source: &[u8],
        file_path: &Path,
    ) -> Vec<Symbol> {
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

        symbols
    }

    /// Extract call edges from an already-parsed tree.
    fn calls_from_tree(&self, tree: &Tree, source: &[u8]) -> Vec<RawCallEdge> {
        let calls_query = match &self.calls_query {
            Some(q) => q,
            None => return vec![],
        };

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

        calls
    }

    /// Extract imports from an already-parsed tree.
    fn imports_from_tree(&self, tree: &Tree, source: &[u8]) -> Vec<Import> {
        let imports_query = match &self.imports_query {
            Some(q) => q,
            None => return vec![],
        };

        let root = tree.root_node();
        let capture_names = imports_query.capture_names();

        let mut cursor = QueryCursor::new();
        let mut imports = Vec::new();
        let mut matches = cursor.matches(imports_query, root, source);

        while let Some(m) = { matches.advance(); matches.get() } {
            let mut module_text: Option<String> = None;
            let mut import_name_text: Option<String> = None;
            let mut alias_text: Option<String> = None;
            let mut is_relative_import = false;

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
                    "_relative" => {
                        // Marker capture: the import query flagged this as a
                        // relative/internal import (e.g. Ruby require_relative).
                        is_relative_import = true;
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
                        .rsplit(['/', '.', ':'])
                        .next()
                        .unwrap_or(&module_path)
                        .to_string()
                });

            let alias = alias_text.filter(|s| !s.is_empty());

            // If the query flagged this as a relative import (e.g. Ruby
            // require_relative), it is always internal regardless of path.
            let is_external = if is_relative_import {
                false
            } else {
                self.config.is_external_import(&module_path)
            };

            imports.push(Import {
                module_path,
                imported_name,
                alias,
                is_external,
            });
        }

        imports
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
        Ok(self.symbols_from_tree(&tree, source, file_path))
    }

    fn extract_calls(&self, source: &[u8], _file_path: &Path) -> Result<Vec<RawCallEdge>> {
        if source.is_empty() {
            return Ok(vec![]);
        }
        let tree = self.parse_tree(source)?;
        Ok(self.calls_from_tree(&tree, source))
    }

    fn extract_types(&self, _source: &[u8], _file_path: &Path) -> Result<Vec<TypeInfo>> {
        Ok(vec![])
    }

    fn extract_imports(&self, source: &[u8], _file_path: &Path) -> Result<Vec<Import>> {
        if source.is_empty() {
            return Ok(vec![]);
        }
        let tree = self.parse_tree(source)?;
        Ok(self.imports_from_tree(&tree, source))
    }

    /// Parse once and extract all data — avoids triple-parsing the same source.
    fn parse_file(&self, source: &[u8], file_path: &Path) -> Result<FileAnalysis> {
        if source.is_empty() {
            return Ok(FileAnalysis {
                symbols: vec![],
                calls: vec![],
                types: vec![],
                imports: vec![],
            });
        }
        let tree = self.parse_tree(source)?;
        Ok(FileAnalysis {
            symbols: self.symbols_from_tree(&tree, source, file_path),
            calls: self.calls_from_tree(&tree, source),
            types: vec![],
            imports: self.imports_from_tree(&tree, source),
        })
    }
}

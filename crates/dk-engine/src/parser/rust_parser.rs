use super::LanguageParser;
use dk_core::{CallKind, Import, RawCallEdge, Result, Span, Symbol, SymbolKind, TypeInfo, Visibility};
use std::path::Path;
use tree_sitter::{Node, Parser, TreeCursor};
use uuid::Uuid;

/// Rust parser backed by tree-sitter.
///
/// Extracts symbols, call edges, imports, and (stub) type information from
/// Rust source files.
pub struct RustParser;

impl RustParser {
    pub fn new() -> Self {
        Self
    }

    /// Create a configured tree-sitter parser for Rust.
    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| dk_core::Error::ParseError(format!("Failed to load Rust grammar: {e}")))?;
        Ok(parser)
    }

    /// Parse source bytes into a tree-sitter tree.
    fn parse_tree(source: &[u8]) -> Result<tree_sitter::Tree> {
        let mut parser = Self::create_parser()?;
        parser
            .parse(source, None)
            .ok_or_else(|| dk_core::Error::ParseError("tree-sitter parse returned None".into()))
    }

    /// Determine the visibility of a node by checking for a `visibility_modifier` child.
    fn node_visibility(node: &Node, source: &[u8]) -> Visibility {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "visibility_modifier" {
                let text = &source[child.start_byte()..child.end_byte()];
                let text_str = std::str::from_utf8(text).unwrap_or("");
                if text_str.contains("crate") {
                    return Visibility::Crate;
                }
                if text_str.contains("super") {
                    return Visibility::Super;
                }
                return Visibility::Public;
            }
        }
        Visibility::Private
    }

    /// Extract the name from a node by looking for the `name` field first,
    /// then falling back to looking for specific identifier children.
    fn node_name(node: &Node, source: &[u8]) -> Option<String> {
        // For impl_item, construct a name from type + trait
        if node.kind() == "impl_item" {
            return Self::impl_name(node, source);
        }

        // Try the "name" field (works for function_item, struct_item, enum_item, trait_item, etc.)
        if let Some(name_node) = node.child_by_field_name("name") {
            let text = &source[name_node.start_byte()..name_node.end_byte()];
            return std::str::from_utf8(text).ok().map(|s| s.to_string());
        }

        None
    }

    /// Construct a name for an impl block: "impl Trait for Type" or "impl Type".
    fn impl_name(node: &Node, source: &[u8]) -> Option<String> {
        let mut type_name = None;
        let mut trait_name = None;

        // Look for the type being implemented and optional trait
        if let Some(ty) = node.child_by_field_name("type") {
            let text = &source[ty.start_byte()..ty.end_byte()];
            type_name = std::str::from_utf8(text).ok().map(|s| s.to_string());
        }

        if let Some(tr) = node.child_by_field_name("trait") {
            let text = &source[tr.start_byte()..tr.end_byte()];
            trait_name = std::str::from_utf8(text).ok().map(|s| s.to_string());
        }

        match (trait_name, type_name) {
            (Some(tr), Some(ty)) => Some(format!("impl {tr} for {ty}")),
            (None, Some(ty)) => Some(format!("impl {ty}")),
            _ => Some("impl".to_string()),
        }
    }

    /// Extract the first line of the node's source text as the signature.
    fn node_signature(node: &Node, source: &[u8]) -> Option<String> {
        let text = &source[node.start_byte()..node.end_byte()];
        let text_str = std::str::from_utf8(text).ok()?;
        let first_line = text_str.lines().next()?;
        Some(first_line.trim().to_string())
    }

    /// Collect preceding `///` doc comments for a node.
    fn doc_comments(node: &Node, source: &[u8]) -> Option<String> {
        let mut comments = Vec::new();
        let mut sibling = node.prev_sibling();

        while let Some(prev) = sibling {
            if prev.kind() == "line_comment" {
                let text = &source[prev.start_byte()..prev.end_byte()];
                if let Ok(s) = std::str::from_utf8(text) {
                    let trimmed = s.trim();
                    if trimmed.starts_with("///") {
                        // Strip the `/// ` prefix
                        let content = trimmed.strip_prefix("/// ").unwrap_or(
                            trimmed.strip_prefix("///").unwrap_or(trimmed),
                        );
                        comments.push(content.to_string());
                        sibling = prev.prev_sibling();
                        continue;
                    }
                }
            }
            break;
        }

        if comments.is_empty() {
            None
        } else {
            comments.reverse();
            Some(comments.join("\n"))
        }
    }

    /// Map a tree-sitter node kind to our SymbolKind, if applicable.
    fn map_symbol_kind(kind: &str) -> Option<SymbolKind> {
        match kind {
            "function_item" => Some(SymbolKind::Function),
            "struct_item" => Some(SymbolKind::Struct),
            "enum_item" => Some(SymbolKind::Enum),
            "trait_item" => Some(SymbolKind::Trait),
            "impl_item" => Some(SymbolKind::Impl),
            "type_item" => Some(SymbolKind::TypeAlias),
            "const_item" => Some(SymbolKind::Const),
            "static_item" => Some(SymbolKind::Static),
            "mod_item" => Some(SymbolKind::Module),
            _ => None,
        }
    }

    /// Find the name of the enclosing function for a given node, if any.
    fn enclosing_function_name(node: &Node, source: &[u8]) -> String {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == "function_item" {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    let text = &source[name_node.start_byte()..name_node.end_byte()];
                    if let Ok(name) = std::str::from_utf8(text) {
                        return name.to_string();
                    }
                }
            }
            current = parent.parent();
        }
        "<module>".to_string()
    }

    /// Recursively walk the tree to extract call edges.
    fn walk_calls(cursor: &mut TreeCursor, source: &[u8], calls: &mut Vec<RawCallEdge>) {
        let node = cursor.node();

        match node.kind() {
            "call_expression" => {
                // Direct function call: get the function name from "function" field
                if let Some(func_node) = node.child_by_field_name("function") {
                    let callee = Self::extract_callee_name(&func_node, source);
                    if !callee.is_empty() {
                        let caller = Self::enclosing_function_name(&node, source);
                        calls.push(RawCallEdge {
                            caller_name: caller,
                            callee_name: callee,
                            call_site: Span {
                                start_byte: node.start_byte() as u32,
                                end_byte: node.end_byte() as u32,
                            },
                            kind: CallKind::DirectCall,
                        });
                    }
                }
            }
            "method_call_expression" => {
                // method_call_expression has a "name" field for the method name
                // In tree-sitter-rust, the method name is in the "name" field
                // but it might also be the last identifier child. Let's try field first.
                let method_name = if let Some(name_node) = node.child_by_field_name("name") {
                    let text = &source[name_node.start_byte()..name_node.end_byte()];
                    std::str::from_utf8(text).unwrap_or("").to_string()
                } else {
                    // fallback: scan for identifier children
                    Self::last_identifier_child(&node, source)
                };

                if !method_name.is_empty() {
                    let caller = Self::enclosing_function_name(&node, source);
                    calls.push(RawCallEdge {
                        caller_name: caller,
                        callee_name: method_name,
                        call_site: Span {
                            start_byte: node.start_byte() as u32,
                            end_byte: node.end_byte() as u32,
                        },
                        kind: CallKind::MethodCall,
                    });
                }
            }
            "macro_invocation" => {
                // macro_invocation has a "macro" field for the macro name
                if let Some(macro_node) = node.child_by_field_name("macro") {
                    let text = &source[macro_node.start_byte()..macro_node.end_byte()];
                    if let Ok(name) = std::str::from_utf8(text) {
                        let caller = Self::enclosing_function_name(&node, source);
                        calls.push(RawCallEdge {
                            caller_name: caller,
                            callee_name: name.to_string(),
                            call_site: Span {
                                start_byte: node.start_byte() as u32,
                                end_byte: node.end_byte() as u32,
                            },
                            kind: CallKind::MacroInvocation,
                        });
                    }
                }
            }
            _ => {}
        }

        // Recurse into children
        if cursor.goto_first_child() {
            loop {
                Self::walk_calls(cursor, source, calls);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    /// Extract callee name from a call expression's function node.
    /// Handles identifiers, field expressions (e.g. `module::func`), and scoped identifiers.
    fn extract_callee_name(node: &Node, source: &[u8]) -> String {
        let text = &source[node.start_byte()..node.end_byte()];
        std::str::from_utf8(text).unwrap_or("").to_string()
    }

    /// Get the last identifier child of a node (fallback for method names).
    fn last_identifier_child(node: &Node, source: &[u8]) -> String {
        let mut cursor = node.walk();
        let mut last_ident = String::new();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "field_identifier" {
                let text = &source[child.start_byte()..child.end_byte()];
                if let Ok(name) = std::str::from_utf8(text) {
                    last_ident = name.to_string();
                }
            }
        }
        last_ident
    }

    /// Extract the full path from a use_declaration node.
    fn extract_use_path(node: &Node, source: &[u8]) -> Vec<Import> {
        let mut imports = Vec::new();

        // Get the full text of the use declaration (minus `use` keyword and semicolon)
        // We need to find the use_path/scoped_use_list within the use_declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "use_as_clause" | "scoped_identifier" | "use_wildcard" | "identifier"
                | "scoped_use_list" | "use_list" => {
                    Self::collect_imports_from_node(&child, source, "", &mut imports);
                }
                _ => {}
            }
        }

        // If we didn't extract any imports from structured children, fall back
        // to extracting the full text path
        if imports.is_empty() {
            let text = &source[node.start_byte()..node.end_byte()];
            if let Ok(full_text) = std::str::from_utf8(text) {
                // Strip `use ` prefix and `;` suffix
                let path = full_text
                    .trim()
                    .strip_prefix("use ")
                    .unwrap_or(full_text.trim())
                    .strip_suffix(';')
                    .unwrap_or(full_text.trim())
                    .trim();

                if !path.is_empty() {
                    let is_external = Self::is_external_path(path);
                    let imported_name = path.rsplit("::").next().unwrap_or(path).to_string();
                    imports.push(Import {
                        module_path: path.to_string(),
                        imported_name,
                        alias: None,
                        is_external,
                    });
                }
            }
        }

        imports
    }

    /// Recursively collect imports from a use tree node.
    fn collect_imports_from_node(
        node: &Node,
        source: &[u8],
        prefix: &str,
        imports: &mut Vec<Import>,
    ) {
        let text = &source[node.start_byte()..node.end_byte()];
        let text_str = std::str::from_utf8(text).unwrap_or("");

        match node.kind() {
            "scoped_identifier" | "identifier" | "use_as_clause" | "use_wildcard" => {
                let full_path = if prefix.is_empty() {
                    text_str.to_string()
                } else {
                    format!("{prefix}::{text_str}")
                };

                let is_external = Self::is_external_path(&full_path);
                let imported_name = full_path.rsplit("::").next().unwrap_or(&full_path).to_string();

                // Check for alias in use_as_clause
                let alias = if node.kind() == "use_as_clause" {
                    node.child_by_field_name("alias").and_then(|a| {
                        let a_text = &source[a.start_byte()..a.end_byte()];
                        std::str::from_utf8(a_text).ok().map(|s| s.to_string())
                    })
                } else {
                    None
                };

                imports.push(Import {
                    module_path: full_path,
                    imported_name,
                    alias,
                    is_external,
                });
            }
            "scoped_use_list" => {
                // Has a "path" field (the prefix) and a "list" field (the use_list)
                let path_prefix = node.child_by_field_name("path").map(|p| {
                    let p_text = &source[p.start_byte()..p.end_byte()];
                    std::str::from_utf8(p_text).unwrap_or("").to_string()
                });

                let combined_prefix = match (prefix, path_prefix.as_deref()) {
                    ("", Some(p)) => p.to_string(),
                    (pfx, Some(p)) => format!("{pfx}::{p}"),
                    (pfx, None) => pfx.to_string(),
                };

                if let Some(list) = node.child_by_field_name("list") {
                    let mut cursor = list.walk();
                    for child in list.children(&mut cursor) {
                        Self::collect_imports_from_node(&child, source, &combined_prefix, imports);
                    }
                }
            }
            "use_list" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    Self::collect_imports_from_node(&child, source, prefix, imports);
                }
            }
            _ => {}
        }
    }

    /// Determine if an import path is external (not starting with crate::, super::, self::).
    fn is_external_path(path: &str) -> bool {
        !path.starts_with("crate::")
            && !path.starts_with("crate")
            && !path.starts_with("super::")
            && !path.starts_with("super")
            && !path.starts_with("self::")
            && !path.starts_with("self")
    }
}

impl Default for RustParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for RustParser {
    fn extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn extract_symbols(&self, source: &[u8], file_path: &Path) -> Result<Vec<Symbol>> {
        if source.is_empty() {
            return Ok(vec![]);
        }

        let tree = Self::parse_tree(source)?;
        let root = tree.root_node();
        let mut symbols = Vec::new();
        let mut cursor = root.walk();

        for node in root.children(&mut cursor) {
            if let Some(kind) = Self::map_symbol_kind(node.kind()) {
                let name = Self::node_name(&node, source).unwrap_or_default();
                if name.is_empty() {
                    continue;
                }

                let visibility = Self::node_visibility(&node, source);
                let signature = Self::node_signature(&node, source);
                let doc_comment = Self::doc_comments(&node, source);

                symbols.push(Symbol {
                    id: Uuid::new_v4(),
                    name: name.clone(),
                    qualified_name: name,
                    kind,
                    visibility,
                    file_path: file_path.to_path_buf(),
                    span: Span {
                        start_byte: node.start_byte() as u32,
                        end_byte: node.end_byte() as u32,
                    },
                    signature,
                    doc_comment,
                    parent: None,
                    last_modified_by: None,
                    last_modified_intent: None,
                });
            }
        }

        Ok(symbols)
    }

    fn extract_calls(&self, source: &[u8], _file_path: &Path) -> Result<Vec<RawCallEdge>> {
        if source.is_empty() {
            return Ok(vec![]);
        }

        let tree = Self::parse_tree(source)?;
        let root = tree.root_node();
        let mut calls = Vec::new();
        let mut cursor = root.walk();

        Self::walk_calls(&mut cursor, source, &mut calls);

        Ok(calls)
    }

    fn extract_types(&self, _source: &[u8], _file_path: &Path) -> Result<Vec<TypeInfo>> {
        // Stub: will be enhanced later
        Ok(vec![])
    }

    fn extract_imports(&self, source: &[u8], _file_path: &Path) -> Result<Vec<Import>> {
        if source.is_empty() {
            return Ok(vec![]);
        }

        let tree = Self::parse_tree(source)?;
        let root = tree.root_node();
        let mut imports = Vec::new();
        let mut cursor = root.walk();

        for node in root.children(&mut cursor) {
            if node.kind() == "use_declaration" {
                imports.extend(Self::extract_use_path(&node, source));
            }
        }

        Ok(imports)
    }
}

use super::LanguageParser;
use dk_core::{CallKind, Import, RawCallEdge, Result, Span, Symbol, SymbolKind, TypeInfo, Visibility};
use std::path::Path;
use tree_sitter::{Node, Parser, TreeCursor};
use uuid::Uuid;

/// TypeScript/JavaScript parser backed by tree-sitter.
///
/// Extracts symbols, call edges, imports, and (stub) type information from
/// TypeScript, TSX, JavaScript, and JSX source files.
///
/// Uses the TSX grammar for all files since TSX is a superset of TypeScript.
pub struct TypeScriptParser;

impl TypeScriptParser {
    pub fn new() -> Self {
        Self
    }

    /// Create a configured tree-sitter parser for TypeScript (TSX superset).
    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
            .map_err(|e| {
                dk_core::Error::ParseError(format!("Failed to load TypeScript grammar: {e}"))
            })?;
        Ok(parser)
    }

    /// Parse source bytes into a tree-sitter tree.
    fn parse_tree(source: &[u8]) -> Result<tree_sitter::Tree> {
        let mut parser = Self::create_parser()?;
        parser
            .parse(source, None)
            .ok_or_else(|| dk_core::Error::ParseError("tree-sitter parse returned None".into()))
    }

    /// Get the text of a node as a UTF-8 string.
    fn node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
        let text = &source[node.start_byte()..node.end_byte()];
        std::str::from_utf8(text).unwrap_or("")
    }

    /// Extract the name from a node by looking for the `name` field.
    fn node_name(node: &Node, source: &[u8]) -> Option<String> {
        node.child_by_field_name("name")
            .map(|n| Self::node_text(&n, source).to_string())
    }

    /// Extract the first line of the node's source text as the signature.
    fn node_signature(node: &Node, source: &[u8]) -> Option<String> {
        let text_str = Self::node_text(node, source);
        let first_line = text_str.lines().next()?;
        Some(first_line.trim().to_string())
    }

    /// Collect preceding `//` or `/** */` doc comments for a node.
    fn doc_comments(node: &Node, source: &[u8]) -> Option<String> {
        let mut comments = Vec::new();
        let mut sibling = node.prev_sibling();

        while let Some(prev) = sibling {
            if prev.kind() == "comment" {
                let text = Self::node_text(&prev, source).trim().to_string();
                comments.push(text);
                sibling = prev.prev_sibling();
                continue;
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
            "function_declaration" => Some(SymbolKind::Function),
            "class_declaration" => Some(SymbolKind::Class),
            "interface_declaration" => Some(SymbolKind::Interface),
            "type_alias_declaration" => Some(SymbolKind::TypeAlias),
            "enum_declaration" => Some(SymbolKind::Enum),
            "lexical_declaration" => Some(SymbolKind::Const),
            _ => None,
        }
    }

    /// Extract variable names from a lexical_declaration.
    /// e.g. `const MAX_RETRIES = 3;` yields "MAX_RETRIES".
    fn extract_variable_names(node: &Node, source: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = Self::node_text(&name_node, source).to_string();
                    if !name.is_empty() {
                        names.push(name);
                    }
                }
            }
        }
        names
    }

    /// Extract a symbol from a declaration node.
    fn extract_symbol(
        node: &Node,
        source: &[u8],
        file_path: &Path,
        visibility: Visibility,
    ) -> Vec<Symbol> {
        let kind = match Self::map_symbol_kind(node.kind()) {
            Some(k) => k,
            None => return vec![],
        };

        // For lexical_declaration (const/let/var), extract variable names
        if node.kind() == "lexical_declaration" {
            let names = Self::extract_variable_names(node, source);
            return names
                .into_iter()
                .map(|name| Symbol {
                    id: Uuid::new_v4(),
                    name: name.clone(),
                    qualified_name: name,
                    kind: SymbolKind::Const,
                    visibility: visibility.clone(),
                    file_path: file_path.to_path_buf(),
                    span: Span {
                        start_byte: node.start_byte() as u32,
                        end_byte: node.end_byte() as u32,
                    },
                    signature: Self::node_signature(node, source),
                    doc_comment: Self::doc_comments(node, source),
                    parent: None,
                    last_modified_by: None,
                    last_modified_intent: None,
                })
                .collect();
        }

        let name = match Self::node_name(node, source) {
            Some(n) if !n.is_empty() => n,
            _ => return vec![],
        };

        vec![Symbol {
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
            signature: Self::node_signature(node, source),
            doc_comment: Self::doc_comments(node, source),
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        }]
    }

    /// Find the name of the enclosing function for a given node, if any.
    fn enclosing_function_name(node: &Node, source: &[u8]) -> String {
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                "function_declaration" | "method_definition" => {
                    if let Some(name_node) = parent.child_by_field_name("name") {
                        let name = Self::node_text(&name_node, source);
                        if !name.is_empty() {
                            return name.to_string();
                        }
                    }
                }
                "arrow_function" | "function_expression" | "function" => {
                    // Anonymous function — check if it's assigned to a variable
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
                _ => {}
            }
            current = parent.parent();
        }
        "<module>".to_string()
    }

    /// Extract the callee name from a call_expression or new_expression's function node.
    fn extract_callee_name(node: &Node, source: &[u8]) -> (String, CallKind) {
        match node.kind() {
            "member_expression" => {
                // e.g. console.log, user.save()
                if let Some(prop) = node.child_by_field_name("property") {
                    let name = Self::node_text(&prop, source).to_string();
                    return (name, CallKind::MethodCall);
                }
                let text = Self::node_text(node, source).to_string();
                (text, CallKind::MethodCall)
            }
            "identifier" => {
                let name = Self::node_text(node, source).to_string();
                (name, CallKind::DirectCall)
            }
            _ => {
                let text = Self::node_text(node, source).to_string();
                (text, CallKind::DirectCall)
            }
        }
    }

    /// Recursively walk the tree to extract call edges.
    fn walk_calls(cursor: &mut TreeCursor, source: &[u8], calls: &mut Vec<RawCallEdge>) {
        let node = cursor.node();

        match node.kind() {
            "call_expression" => {
                // Direct or method call: get the function part
                if let Some(func_node) = node.child_by_field_name("function") {
                    let (callee, kind) = Self::extract_callee_name(&func_node, source);
                    if !callee.is_empty() {
                        let caller = Self::enclosing_function_name(&node, source);
                        calls.push(RawCallEdge {
                            caller_name: caller,
                            callee_name: callee,
                            call_site: Span {
                                start_byte: node.start_byte() as u32,
                                end_byte: node.end_byte() as u32,
                            },
                            kind,
                        });
                    }
                }
            }
            "new_expression" => {
                // Constructor call: new ClassName(...)
                if let Some(constructor_node) = node.child_by_field_name("constructor") {
                    let name = Self::node_text(&constructor_node, source).to_string();
                    if !name.is_empty() {
                        let caller = Self::enclosing_function_name(&node, source);
                        calls.push(RawCallEdge {
                            caller_name: caller,
                            callee_name: name,
                            call_site: Span {
                                start_byte: node.start_byte() as u32,
                                end_byte: node.end_byte() as u32,
                            },
                            kind: CallKind::DirectCall,
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

    /// Extract the alias name from a namespace_import node (e.g. `* as utils`).
    fn namespace_import_alias(node: &Node, source: &[u8]) -> Option<String> {
        if let Some(name_node) = node.child_by_field_name("name") {
            return Some(Self::node_text(&name_node, source).to_string());
        }
        // Fallback: look for identifier child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(Self::node_text(&child, source).to_string());
            }
        }
        None
    }

    /// Extract imports from an import_statement node.
    ///
    /// Handles:
    /// - `import { A, B } from 'module'`
    /// - `import * as ns from 'module'`
    /// - `import Default from 'module'`
    /// - `import 'module'` (side-effect import)
    fn extract_import(node: &Node, source: &[u8]) -> Vec<Import> {
        let mut imports = Vec::new();

        // Get the module path (source field of import_statement)
        let module_path = match node.child_by_field_name("source") {
            Some(src_node) => {
                let raw = Self::node_text(&src_node, source);
                // Strip quotes from string literal
                raw.trim_matches(|c| c == '\'' || c == '"').to_string()
            }
            None => return imports,
        };

        let is_external = !module_path.starts_with('.') && !module_path.starts_with('/');

        // Walk children to find imported names
        let mut cursor = node.walk();
        let mut found_names = false;

        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_clause" => {
                    Self::extract_import_clause(&child, source, &module_path, is_external, &mut imports);
                    found_names = true;
                }
                "named_imports" => {
                    Self::extract_named_imports(&child, source, &module_path, is_external, &mut imports);
                    found_names = true;
                }
                "namespace_import" => {
                    // import * as ns from 'module'
                    let alias = Self::namespace_import_alias(&child, source);
                    imports.push(Import {
                        module_path: module_path.clone(),
                        imported_name: "*".to_string(),
                        alias,
                        is_external,
                    });
                    found_names = true;
                }
                "identifier" => {
                    // Default import: import Foo from 'module'
                    let name = Self::node_text(&child, source).to_string();
                    if name != "import" && name != "from" && name != "type" {
                        imports.push(Import {
                            module_path: module_path.clone(),
                            imported_name: name,
                            alias: None,
                            is_external,
                        });
                        found_names = true;
                    }
                }
                _ => {}
            }
        }

        // Side-effect import: import 'module'
        if !found_names {
            imports.push(Import {
                module_path,
                imported_name: "*".to_string(),
                alias: None,
                is_external,
            });
        }

        imports
    }

    /// Extract names from an import_clause node.
    fn extract_import_clause(
        node: &Node,
        source: &[u8],
        module_path: &str,
        is_external: bool,
        imports: &mut Vec<Import>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    // Default import
                    let name = Self::node_text(&child, source).to_string();
                    imports.push(Import {
                        module_path: module_path.to_string(),
                        imported_name: name,
                        alias: None,
                        is_external,
                    });
                }
                "named_imports" => {
                    Self::extract_named_imports(&child, source, module_path, is_external, imports);
                }
                "namespace_import" => {
                    let alias = Self::namespace_import_alias(&child, source);
                    imports.push(Import {
                        module_path: module_path.to_string(),
                        imported_name: "*".to_string(),
                        alias,
                        is_external,
                    });
                }
                _ => {}
            }
        }
    }

    /// Extract individual names from a named_imports node (`{ A, B as C }`).
    fn extract_named_imports(
        node: &Node,
        source: &[u8],
        module_path: &str,
        is_external: bool,
        imports: &mut Vec<Import>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import_specifier" {
                let name_node = child.child_by_field_name("name");
                let alias_node = child.child_by_field_name("alias");

                let imported_name = name_node
                    .map(|n| Self::node_text(&n, source).to_string())
                    .unwrap_or_default();

                let alias = alias_node.map(|n| Self::node_text(&n, source).to_string());

                if !imported_name.is_empty() {
                    imports.push(Import {
                        module_path: module_path.to_string(),
                        imported_name,
                        alias,
                        is_external,
                    });
                }
            }
        }
    }
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for TypeScriptParser {
    fn extensions(&self) -> &[&str] {
        &["ts", "tsx", "js", "jsx"]
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
            match node.kind() {
                "export_statement" => {
                    // Exported declaration: unwrap to find the inner declaration
                    let mut inner_cursor = node.walk();
                    for child in node.children(&mut inner_cursor) {
                        if Self::map_symbol_kind(child.kind()).is_some() {
                            symbols.extend(Self::extract_symbol(
                                &child,
                                source,
                                file_path,
                                Visibility::Public,
                            ));
                        }
                    }
                }
                kind if Self::map_symbol_kind(kind).is_some() => {
                    // Non-exported top-level declaration
                    symbols.extend(Self::extract_symbol(
                        &node,
                        source,
                        file_path,
                        Visibility::Private,
                    ));
                }
                _ => {}
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
            if node.kind() == "import_statement" {
                imports.extend(Self::extract_import(&node, source));
            }
        }

        Ok(imports)
    }
}

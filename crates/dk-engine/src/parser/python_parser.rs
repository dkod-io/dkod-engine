use super::LanguageParser;
use dk_core::{CallKind, Import, RawCallEdge, Result, Span, Symbol, SymbolKind, TypeInfo, Visibility};
use std::path::Path;
use tree_sitter::{Node, Parser, TreeCursor};
use uuid::Uuid;

/// Python parser backed by tree-sitter.
///
/// Extracts symbols, call edges, imports, and (stub) type information from
/// Python source files.
pub struct PythonParser;

impl PythonParser {
    pub fn new() -> Self {
        Self
    }

    /// Create a configured tree-sitter parser for Python.
    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|e| dk_core::Error::ParseError(format!("Failed to load Python grammar: {e}")))?;
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

    /// Determine visibility based on Python naming conventions.
    /// Names starting with `_` are considered private; everything else is public.
    fn name_visibility(name: &str) -> Visibility {
        if name.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    /// Extract the name from a function_definition or class_definition node.
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

    /// Extract docstring from a function or class body.
    ///
    /// In Python, a docstring is the first statement in the body if it is an
    /// `expression_statement` containing a `string` node.
    fn extract_docstring(node: &Node, source: &[u8]) -> Option<String> {
        // Look for the "body" field (block node)
        let body = node.child_by_field_name("body")?;

        // The first child of the block should be the potential docstring
        let first_stmt = body.child(0)?;

        if first_stmt.kind() == "expression_statement" {
            let expr = first_stmt.child(0)?;
            if expr.kind() == "string" {
                let raw = Self::node_text(&expr, source);
                // Strip triple-quote delimiters and clean up
                let content = raw
                    .strip_prefix("\"\"\"")
                    .and_then(|s| s.strip_suffix("\"\"\""))
                    .or_else(|| {
                        raw.strip_prefix("'''")
                            .and_then(|s| s.strip_suffix("'''"))
                    })
                    .unwrap_or(raw);
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }

        None
    }

    /// Collect preceding `#` comments for a node.
    fn doc_comments(node: &Node, source: &[u8]) -> Option<String> {
        let mut comments = Vec::new();
        let mut sibling = node.prev_sibling();

        while let Some(prev) = sibling {
            if prev.kind() == "comment" {
                let text = Self::node_text(&prev, source).trim().to_string();
                // Strip the `# ` prefix
                let content = text
                    .strip_prefix("# ")
                    .unwrap_or(text.strip_prefix('#').unwrap_or(&text));
                comments.push(content.to_string());
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

    /// Extract a symbol from a function_definition or class_definition node.
    fn extract_symbol_from_def(
        node: &Node,
        source: &[u8],
        file_path: &Path,
    ) -> Option<Symbol> {
        let kind = match node.kind() {
            "function_definition" => SymbolKind::Function,
            "class_definition" => SymbolKind::Class,
            _ => return None,
        };

        let name = Self::node_name(node, source)?;
        if name.is_empty() {
            return None;
        }

        let visibility = Self::name_visibility(&name);
        let signature = Self::node_signature(node, source);

        // Try docstring first, fall back to preceding comments
        let doc_comment = Self::extract_docstring(node, source)
            .or_else(|| Self::doc_comments(node, source));

        Some(Symbol {
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
        })
    }

    /// Extract the name from a simple assignment at the top level.
    /// e.g. `MAX_RETRIES = 3` yields "MAX_RETRIES".
    /// Only handles simple identifier = value assignments (not tuple unpacking, etc.).
    fn extract_assignment_name(node: &Node, source: &[u8]) -> Option<String> {
        if node.kind() != "expression_statement" {
            return None;
        }

        // The expression_statement should contain an assignment
        let child = node.child(0)?;
        if child.kind() != "assignment" {
            return None;
        }

        // The left side should be a simple identifier
        let left = child.child_by_field_name("left")?;
        if left.kind() != "identifier" {
            return None;
        }

        let name = Self::node_text(&left, source).to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// Find the name of the enclosing function for a given node, if any.
    fn enclosing_function_name(node: &Node, source: &[u8]) -> String {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == "function_definition" {
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

    /// Extract the callee name and call kind from a call node's function field.
    fn extract_callee_info(node: &Node, source: &[u8]) -> (String, CallKind) {
        match node.kind() {
            "attribute" => {
                // e.g. obj.method — the callee is the attribute (method name)
                if let Some(attr) = node.child_by_field_name("attribute") {
                    let name = Self::node_text(&attr, source).to_string();
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
            "call" => {
                // Python call node has a "function" field
                if let Some(func_node) = node.child_by_field_name("function") {
                    let (callee, kind) = Self::extract_callee_info(&func_node, source);
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
            "decorator" => {
                // A decorator is effectively a call to the decorator function.
                // The decorator node contains the decorator expression (after @).
                // It can be a simple identifier like `@login_required`,
                // a call like `@app.route("/api")`, or an attribute like `@app.middleware`.
                //
                // For `@login_required`, the child is an identifier.
                // For `@app.route("/api")`, the child is a call node (which walk_calls handles).
                // For `@app.middleware`, the child is an attribute.
                //
                // We handle the identifier and attribute cases here; the call case
                // is handled recursively when we descend into children.
                let mut inner_cursor = node.walk();
                for child in node.children(&mut inner_cursor) {
                    match child.kind() {
                        "identifier" => {
                            let name = Self::node_text(&child, source).to_string();
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
                        "attribute" => {
                            if let Some(attr) = child.child_by_field_name("attribute") {
                                let name = Self::node_text(&attr, source).to_string();
                                if !name.is_empty() {
                                    let caller = Self::enclosing_function_name(&node, source);
                                    calls.push(RawCallEdge {
                                        caller_name: caller,
                                        callee_name: name,
                                        call_site: Span {
                                            start_byte: node.start_byte() as u32,
                                            end_byte: node.end_byte() as u32,
                                        },
                                        kind: CallKind::MethodCall,
                                    });
                                }
                            }
                        }
                        _ => {}
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

    /// Extract imports from an `import_statement` node.
    /// e.g. `import os` or `import os, sys`
    fn extract_import_statement(node: &Node, source: &[u8]) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "dotted_name" => {
                    let module = Self::node_text(&child, source).to_string();
                    if !module.is_empty() {
                        imports.push(Import {
                            module_path: module.clone(),
                            imported_name: module,
                            alias: None,
                            is_external: true,
                        });
                    }
                }
                "aliased_import" => {
                    let name_node = child.child_by_field_name("name");
                    let alias_node = child.child_by_field_name("alias");

                    if let Some(name_n) = name_node {
                        let module = Self::node_text(&name_n, source).to_string();
                        let alias = alias_node
                            .map(|a| Self::node_text(&a, source).to_string());
                        imports.push(Import {
                            module_path: module.clone(),
                            imported_name: module,
                            alias,
                            is_external: true,
                        });
                    }
                }
                _ => {}
            }
        }

        imports
    }

    /// Extract imports from an `import_from_statement` node.
    /// e.g. `from os.path import join, exists` or `from .local import helper`
    fn extract_import_from_statement(node: &Node, source: &[u8]) -> Vec<Import> {
        let mut imports = Vec::new();

        // Get the module name. In tree-sitter-python the module is in the
        // "module_name" field. For relative imports it includes the dots.
        let module_path = Self::extract_from_module_path(node, source);
        let is_external = !module_path.starts_with('.');

        // Collect imported names
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "dotted_name" | "identifier" => {
                    // Skip the module name itself (already captured)
                    // The imported names come after the "import" keyword
                    // In tree-sitter-python, the imported names are in the node's
                    // named children that are not the module_name field.
                    // We need to distinguish module from imported names.
                }
                "aliased_import" => {
                    let name_node = child.child_by_field_name("name");
                    let alias_node = child.child_by_field_name("alias");

                    if let Some(name_n) = name_node {
                        let imported_name = Self::node_text(&name_n, source).to_string();
                        let alias = alias_node
                            .map(|a| Self::node_text(&a, source).to_string());
                        imports.push(Import {
                            module_path: module_path.clone(),
                            imported_name,
                            alias,
                            is_external,
                        });
                    }
                }
                "wildcard_import" => {
                    imports.push(Import {
                        module_path: module_path.clone(),
                        imported_name: "*".to_string(),
                        alias: None,
                        is_external,
                    });
                }
                _ => {}
            }
        }

        // If we found no imports from the structured children above, parse
        // the imported names from the node text. The tree-sitter-python grammar
        // places imported names as direct children of import_from_statement.
        if imports.is_empty() {
            Self::extract_from_imported_names(node, source, &module_path, is_external, &mut imports);
        }

        imports
    }

    /// Extract the module path from a `from ... import` statement.
    /// Handles both absolute (`from os.path`) and relative (`from .local`) imports.
    fn extract_from_module_path(node: &Node, source: &[u8]) -> String {
        // The module_name field contains the dotted name (may include leading dots for relative).
        if let Some(module_node) = node.child_by_field_name("module_name") {
            return Self::node_text(&module_node, source).to_string();
        }

        // Fallback: reconstruct from the node text between `from` and `import`.
        let text = Self::node_text(node, source);
        if let Some(from_idx) = text.find("from") {
            let after_from = &text[from_idx + 4..];
            if let Some(import_idx) = after_from.find("import") {
                let module = after_from[..import_idx].trim();
                return module.to_string();
            }
        }

        String::new()
    }

    /// Extract imported names from a from-import statement by walking its children.
    fn extract_from_imported_names(
        node: &Node,
        source: &[u8],
        module_path: &str,
        is_external: bool,
        imports: &mut Vec<Import>,
    ) {
        // Walk through all children looking for imported names.
        // In tree-sitter-python, after the module_name and "import" keyword,
        // the imported identifiers appear as children.
        let mut found_import_keyword = false;
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            let text = Self::node_text(&child, source);

            if text == "import" {
                found_import_keyword = true;
                continue;
            }

            if !found_import_keyword {
                continue;
            }

            match child.kind() {
                "dotted_name" | "identifier" => {
                    let imported_name = text.to_string();
                    if !imported_name.is_empty() && imported_name != "," {
                        imports.push(Import {
                            module_path: module_path.to_string(),
                            imported_name,
                            alias: None,
                            is_external,
                        });
                    }
                }
                "aliased_import" => {
                    let name_node = child.child_by_field_name("name");
                    let alias_node = child.child_by_field_name("alias");

                    if let Some(name_n) = name_node {
                        let imported_name = Self::node_text(&name_n, source).to_string();
                        let alias = alias_node
                            .map(|a| Self::node_text(&a, source).to_string());
                        imports.push(Import {
                            module_path: module_path.to_string(),
                            imported_name,
                            alias,
                            is_external,
                        });
                    }
                }
                "wildcard_import" => {
                    imports.push(Import {
                        module_path: module_path.to_string(),
                        imported_name: "*".to_string(),
                        alias: None,
                        is_external,
                    });
                }
                _ => {}
            }
        }
    }
}

impl Default for PythonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for PythonParser {
    fn extensions(&self) -> &[&str] {
        &["py"]
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
                "function_definition" | "class_definition" => {
                    if let Some(sym) = Self::extract_symbol_from_def(&node, source, file_path) {
                        symbols.push(sym);
                    }
                }
                "decorated_definition" => {
                    // Unwrap the decorated_definition to find the inner function or class
                    if let Some(definition) = node.child_by_field_name("definition") {
                        match definition.kind() {
                            "function_definition" | "class_definition" => {
                                if let Some(mut sym) =
                                    Self::extract_symbol_from_def(&definition, source, file_path)
                                {
                                    // Use the span of the whole decorated definition
                                    sym.span = Span {
                                        start_byte: node.start_byte() as u32,
                                        end_byte: node.end_byte() as u32,
                                    };
                                    // Include the decorator in the signature
                                    sym.signature = Self::node_signature(&node, source);
                                    symbols.push(sym);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "expression_statement" => {
                    // Module-level assignment
                    if let Some(name) = Self::extract_assignment_name(&node, source) {
                        let visibility = Self::name_visibility(&name);
                        symbols.push(Symbol {
                            id: Uuid::new_v4(),
                            name: name.clone(),
                            qualified_name: name,
                            kind: SymbolKind::Variable,
                            visibility,
                            file_path: file_path.to_path_buf(),
                            span: Span {
                                start_byte: node.start_byte() as u32,
                                end_byte: node.end_byte() as u32,
                            },
                            signature: Self::node_signature(&node, source),
                            doc_comment: Self::doc_comments(&node, source),
                            parent: None,
                            last_modified_by: None,
                            last_modified_intent: None,
                        });
                    }
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
            match node.kind() {
                "import_statement" => {
                    imports.extend(Self::extract_import_statement(&node, source));
                }
                "import_from_statement" => {
                    imports.extend(Self::extract_import_from_statement(&node, source));
                }
                _ => {}
            }
        }

        Ok(imports)
    }
}

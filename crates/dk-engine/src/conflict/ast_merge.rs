//! AST-aware smart merge: operates at the SYMBOL level, not the line level.
//!
//! If Agent A modifies `fn_a` and Agent B modifies `fn_b` in the same file,
//! that is NOT a conflict even if line numbers shifted. Only same-symbol
//! modifications across sessions are TRUE conflicts.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use dk_core::{Error, Result, Symbol};

use crate::parser::ParserRegistry;

/// The overall status of a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeStatus {
    /// All symbols merged cleanly — no overlapping edits.
    Clean,
    /// At least one symbol was modified by both sides.
    Conflict,
}

/// The result of a three-way AST-level merge.
#[derive(Debug)]
pub struct MergeResult {
    pub status: MergeStatus,
    pub merged_content: String,
    pub conflicts: Vec<SymbolConflict>,
}

/// A single symbol-level conflict: both sides changed the same symbol.
#[derive(Debug)]
pub struct SymbolConflict {
    pub qualified_name: String,
    pub kind: String,
    pub version_a: String,
    pub version_b: String,
    pub base: String,
}

/// A named span of source text representing a top-level symbol.
#[derive(Debug, Clone)]
struct SymbolSpan {
    qualified_name: String,
    kind: String,
    /// The full source text of the symbol (including doc comments captured by the span).
    text: String,
    /// Original ordering index (reserved for future use).
    _order: usize,
}

/// An import line extracted from source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ImportLine {
    /// The raw text of the import statement (e.g. `use std::io;`).
    text: String,
}

/// Extract the import block (all contiguous use/import statements at the top)
/// and the list of top-level symbol spans from parsed source.
fn extract_spans(
    source: &str,
    symbols: &[Symbol],
) -> (Vec<ImportLine>, Vec<SymbolSpan>) {
    let bytes = source.as_bytes();
    let mut import_lines = Vec::new();
    let mut symbol_spans = Vec::new();

    // Collect symbol byte ranges so we can identify import lines
    // (lines that fall outside any symbol span).
    let mut symbol_ranges: Vec<(usize, usize)> = symbols
        .iter()
        .map(|s| (s.span.start_byte as usize, s.span.end_byte as usize))
        .collect();
    symbol_ranges.sort_by_key(|r| r.0);

    // Extract import lines: lines in the source that are NOT inside any symbol span
    // and look like import/use statements.
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("use ")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
        {
            // Check this line isn't inside a symbol span
            let line_start = line.as_ptr() as usize - bytes.as_ptr() as usize;
            let inside_symbol = symbol_ranges
                .iter()
                .any(|(start, end)| line_start >= *start && line_start < *end);
            if !inside_symbol {
                import_lines.push(ImportLine {
                    text: line.to_string(),
                });
            }
        }
    }

    // Extract symbol spans. Prepend doc comments to the symbol text so
    // that comment changes are tracked per-symbol (e.g. adding a doc
    // comment to a route handler is attributed to that handler's symbol,
    // not silently dropped during AST merge).
    for (order, sym) in symbols.iter().enumerate() {
        let start = sym.span.start_byte as usize;
        let end = sym.span.end_byte as usize;
        if end <= bytes.len() {
            let body = String::from_utf8_lossy(&bytes[start..end]).to_string();
            // Only prepend when the doc text is outside the symbol byte span.
            // TypeScript stores the full "// …" text as a sibling; Rust strips
            // the "///" prefix; Python embeds the docstring inside the body.
            let text = match &sym.doc_comment {
                Some(doc) if !doc.is_empty() && !body.contains(doc.as_str()) => {
                    format!("{doc}\n{body}")
                }
                _ => body,
            };
            symbol_spans.push(SymbolSpan {
                qualified_name: sym.qualified_name.clone(),
                kind: sym.kind.to_string(),
                text,
                _order: order,
            });
        }
    }

    (import_lines, symbol_spans)
}

/// Perform a three-way AST-level merge.
///
/// - `file_path`: used to select the tree-sitter parser by extension.
/// - `base`: the common ancestor content.
/// - `version_a`: one agent's modified content.
/// - `version_b`: another agent's modified content.
///
/// Returns an error if the file extension is not supported by any parser.
pub fn ast_merge(
    registry: &ParserRegistry,
    file_path: &str,
    base: &str,
    version_a: &str,
    version_b: &str,
) -> Result<MergeResult> {
    let path = Path::new(file_path);

    if !registry.supports_file(path) {
        return Err(Error::UnsupportedLanguage(format!(
            "AST merge not supported for file: {file_path}"
        )));
    }

    // Parse all three versions
    let base_analysis = registry.parse_file(path, base.as_bytes())?;
    let a_analysis = registry.parse_file(path, version_a.as_bytes())?;
    let b_analysis = registry.parse_file(path, version_b.as_bytes())?;

    // Extract spans
    let (base_imports, base_spans) = extract_spans(base, &base_analysis.symbols);
    let (a_imports, a_spans) = extract_spans(version_a, &a_analysis.symbols);
    let (b_imports, b_spans) = extract_spans(version_b, &b_analysis.symbols);

    // Build lookup maps: qualified_name -> SymbolSpan
    let base_map: BTreeMap<&str, &SymbolSpan> =
        base_spans.iter().map(|s| (s.qualified_name.as_str(), s)).collect();
    let a_map: BTreeMap<&str, &SymbolSpan> =
        a_spans.iter().map(|s| (s.qualified_name.as_str(), s)).collect();
    let b_map: BTreeMap<&str, &SymbolSpan> =
        b_spans.iter().map(|s| (s.qualified_name.as_str(), s)).collect();

    // Build ordered name list: base order first, then new symbols from A and B.
    // This preserves the original file layout instead of alphabetizing.
    let mut all_names: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();

    // Base symbols in their original order
    for span in &base_spans {
        let name = span.qualified_name.as_str();
        if seen.insert(name) {
            all_names.push(name);
        }
    }
    // New symbols from A (in their file order)
    for span in &a_spans {
        let name = span.qualified_name.as_str();
        if seen.insert(name) {
            all_names.push(name);
        }
    }
    // New symbols from B (in their file order)
    for span in &b_spans {
        let name = span.qualified_name.as_str();
        if seen.insert(name) {
            all_names.push(name);
        }
    }

    let mut merged_symbols: Vec<SymbolSpan> = Vec::new();
    let mut conflicts: Vec<SymbolConflict> = Vec::new();
    let mut order_counter: usize = 0;

    for name in &all_names {
        let in_base = base_map.get(name);
        let in_a = a_map.get(name);
        let in_b = b_map.get(name);

        let a_modified = match (in_base, in_a) {
            (Some(base_s), Some(a_s)) => base_s.text != a_s.text,
            (None, Some(_)) => true,  // new in A
            (Some(_), None) => true,  // deleted by A
            (None, None) => false,
        };

        let b_modified = match (in_base, in_b) {
            (Some(base_s), Some(b_s)) => base_s.text != b_s.text,
            (None, Some(_)) => true,  // new in B
            (Some(_), None) => true,  // deleted by B
            (None, None) => false,
        };

        match (a_modified, b_modified) {
            (false, false) => {
                // Neither modified — take base version
                if let Some(base_s) = in_base {
                    merged_symbols.push(SymbolSpan {
                        qualified_name: base_s.qualified_name.clone(),
                        kind: base_s.kind.clone(),
                        text: base_s.text.clone(),
                        _order: order_counter,
                    });
                    order_counter += 1;
                }
            }
            (true, false) => {
                // Only A modified
                if let Some(a_s) = in_a {
                    // A modified or added
                    merged_symbols.push(SymbolSpan {
                        qualified_name: a_s.qualified_name.clone(),
                        kind: a_s.kind.clone(),
                        text: a_s.text.clone(),
                        _order: order_counter,
                    });
                    order_counter += 1;
                }
                // else: A deleted — don't include
            }
            (false, true) => {
                // Only B modified
                if let Some(b_s) = in_b {
                    // B modified or added
                    merged_symbols.push(SymbolSpan {
                        qualified_name: b_s.qualified_name.clone(),
                        kind: b_s.kind.clone(),
                        text: b_s.text.clone(),
                        _order: order_counter,
                    });
                    order_counter += 1;
                }
                // else: B deleted — don't include
            }
            (true, true) => {
                // Both modified — check specifics
                match (in_base, in_a, in_b) {
                    (None, Some(a_s), Some(b_s)) => {
                        // Both added a symbol with the same name → CONFLICT
                        conflicts.push(SymbolConflict {
                            qualified_name: name.to_string(),
                            kind: a_s.kind.clone(),
                            version_a: a_s.text.clone(),
                            version_b: b_s.text.clone(),
                            base: String::new(),
                        });
                        // Include A's version as placeholder in merged output
                        merged_symbols.push(SymbolSpan {
                            qualified_name: a_s.qualified_name.clone(),
                            kind: a_s.kind.clone(),
                            text: a_s.text.clone(),
                            _order: order_counter,
                        });
                        order_counter += 1;
                    }
                    (Some(base_s), Some(a_s), Some(b_s)) => {
                        if a_s.text == b_s.text {
                            // Both made the same change — no conflict
                            merged_symbols.push(SymbolSpan {
                                qualified_name: a_s.qualified_name.clone(),
                                kind: a_s.kind.clone(),
                                text: a_s.text.clone(),
                                _order: order_counter,
                            });
                            order_counter += 1;
                        } else {
                            // Both modified differently → TRUE CONFLICT
                            conflicts.push(SymbolConflict {
                                qualified_name: name.to_string(),
                                kind: base_s.kind.clone(),
                                version_a: a_s.text.clone(),
                                version_b: b_s.text.clone(),
                                base: base_s.text.clone(),
                            });
                            // Include A's version as placeholder
                            merged_symbols.push(SymbolSpan {
                                qualified_name: a_s.qualified_name.clone(),
                                kind: a_s.kind.clone(),
                                text: a_s.text.clone(),
                                _order: order_counter,
                            });
                            order_counter += 1;
                        }
                    }
                    (Some(base_s), None, Some(b_s)) => {
                        // A deleted, B modified → CONFLICT
                        conflicts.push(SymbolConflict {
                            qualified_name: name.to_string(),
                            kind: base_s.kind.clone(),
                            version_a: String::new(),
                            version_b: b_s.text.clone(),
                            base: base_s.text.clone(),
                        });
                        // Include B's version as placeholder
                        merged_symbols.push(SymbolSpan {
                            qualified_name: b_s.qualified_name.clone(),
                            kind: b_s.kind.clone(),
                            text: b_s.text.clone(),
                            _order: order_counter,
                        });
                        order_counter += 1;
                    }
                    (Some(base_s), Some(a_s), None) => {
                        // B deleted, A modified → CONFLICT
                        conflicts.push(SymbolConflict {
                            qualified_name: name.to_string(),
                            kind: base_s.kind.clone(),
                            version_a: a_s.text.clone(),
                            version_b: String::new(),
                            base: base_s.text.clone(),
                        });
                        // Include A's version as placeholder
                        merged_symbols.push(SymbolSpan {
                            qualified_name: a_s.qualified_name.clone(),
                            kind: a_s.kind.clone(),
                            text: a_s.text.clone(),
                            _order: order_counter,
                        });
                        order_counter += 1;
                    }
                    (Some(_), None, None) => {
                        // Both deleted — agree, don't include
                    }
                    _ => {}
                }
            }
        }
    }

    // Merge imports additively (union, deduplicated, preserving base order)
    let mut merged_imports: Vec<String> = Vec::new();
    let mut import_seen: HashSet<String> = HashSet::new();
    // Base imports first (original order), then new imports from A and B
    for imp in base_imports.iter().chain(a_imports.iter()).chain(b_imports.iter()) {
        if import_seen.insert(imp.text.clone()) {
            merged_imports.push(imp.text.clone());
        }
    }

    // Reconstruct the file
    let mut output = String::new();

    // Imports first (in preserved order)
    if !merged_imports.is_empty() {
        for imp in &merged_imports {
            output.push_str(imp);
            output.push('\n');
        }
        output.push('\n');
    }

    // Then symbols, joined with double newlines
    let symbol_texts: Vec<&str> = merged_symbols.iter().map(|s| s.text.as_str()).collect();
    output.push_str(&symbol_texts.join("\n\n"));

    // Ensure trailing newline
    if !output.ends_with('\n') {
        output.push('\n');
    }

    let status = if conflicts.is_empty() {
        MergeStatus::Clean
    } else {
        MergeStatus::Conflict
    };

    Ok(MergeResult {
        status,
        merged_content: output,
        conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_status_eq() {
        assert_eq!(MergeStatus::Clean, MergeStatus::Clean);
        assert_eq!(MergeStatus::Conflict, MergeStatus::Conflict);
        assert_ne!(MergeStatus::Clean, MergeStatus::Conflict);
    }
}

//! Semantic conflict detection for three-way merge.
//!
//! Instead of purely textual diff3, this module parses all three versions
//! of a file (base, head, overlay) with tree-sitter and compares the
//! resulting symbol tables. Conflicts arise when both sides modify,
//! add, or remove the *same* symbol.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use dk_core::{FileAnalysis, Symbol};

use crate::parser::ParserRegistry;

// ── Types ────────────────────────────────────────────────────────────

/// Describes a single semantic conflict within a file.
#[derive(Debug, Clone)]
pub struct SemanticConflict {
    /// Path of the conflicting file.
    pub file_path: String,
    /// Qualified name of the symbol that conflicts.
    pub symbol_name: String,
    /// What our side (overlay) did to this symbol.
    pub our_change: SymbolChangeKind,
    /// What their side (head) did to this symbol.
    pub their_change: SymbolChangeKind,
}

/// Classification of a symbol change relative to the base version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolChangeKind {
    Added,
    Modified,
    Removed,
}

/// Result of analyzing a file for three-way merge.
#[derive(Debug)]
pub enum MergeAnalysis {
    /// No overlapping symbol changes — the file can be auto-merged.
    AutoMerge {
        /// The merged content (overlay content wins for non-overlapping changes).
        merged_content: Vec<u8>,
    },
    /// Overlapping symbol changes that require manual resolution.
    Conflict {
        conflicts: Vec<SemanticConflict>,
    },
}

// ── Analysis ─────────────────────────────────────────────────────────

/// Analyze a single file for semantic conflicts across three versions.
///
/// - `base_content` — the file at the merge base (common ancestor).
/// - `head_content` — the file at the current HEAD (their changes).
/// - `overlay_content` — the file in the session overlay (our changes).
///
/// If parsing fails for any version (e.g. unsupported language), the
/// function falls back to byte-level comparison: if both sides changed
/// the file and produced different bytes, it's a conflict.
pub fn analyze_file_conflict(
    file_path: &str,
    base_content: &[u8],
    head_content: &[u8],
    overlay_content: &[u8],
    parser: &ParserRegistry,
) -> MergeAnalysis {
    let path = Path::new(file_path);

    // Attempt to parse all three versions.
    let base_parse = parser.parse_file(path, base_content);
    let head_parse = parser.parse_file(path, head_content);
    let overlay_parse = parser.parse_file(path, overlay_content);

    match (base_parse, head_parse, overlay_parse) {
        (Ok(base_fa), Ok(head_fa), Ok(overlay_fa)) => {
            semantic_analysis(file_path, &base_fa, &head_fa, &overlay_fa, overlay_content)
        }
        _ => {
            // Fallback: byte-level comparison.
            byte_level_analysis(file_path, base_content, head_content, overlay_content)
        }
    }
}

/// Semantic three-way merge analysis using parsed symbol tables.
fn semantic_analysis(
    file_path: &str,
    base: &FileAnalysis,
    head: &FileAnalysis,
    overlay: &FileAnalysis,
    overlay_content: &[u8],
) -> MergeAnalysis {
    let base_syms = symbol_map(&base.symbols);
    let head_syms = symbol_map(&head.symbols);
    let overlay_syms = symbol_map(&overlay.symbols);

    // Collect all symbol names across all three versions.
    let all_names: HashSet<&str> = base_syms
        .keys()
        .chain(head_syms.keys())
        .chain(overlay_syms.keys())
        .copied()
        .collect();

    let mut conflicts = Vec::new();

    for name in all_names {
        let base_sym = base_syms.get(name);
        let head_sym = head_syms.get(name);
        let overlay_sym = overlay_syms.get(name);

        let head_change = classify_change(base_sym, head_sym);
        let overlay_change = classify_change(base_sym, overlay_sym);

        // If both sides made a change to the same symbol, it's a conflict
        // (unless both changes are identical).
        if let (Some(their), Some(ours)) = (&head_change, &overlay_change) {
            // Same kind of change — check if the results are actually identical.
            if their == ours {
                // Both added or both modified to the same thing — check content.
                let identical = match (head_sym, overlay_sym) {
                    (Some(h), Some(o)) => symbols_equivalent(h, o),
                    (None, None) => true, // both removed
                    _ => false,
                };
                if identical {
                    continue; // No conflict — same change on both sides.
                }
            }

            conflicts.push(SemanticConflict {
                file_path: file_path.to_string(),
                symbol_name: name.to_string(),
                our_change: ours.clone(),
                their_change: their.clone(),
            });
        }
    }

    if conflicts.is_empty() {
        MergeAnalysis::AutoMerge {
            merged_content: overlay_content.to_vec(),
        }
    } else {
        MergeAnalysis::Conflict { conflicts }
    }
}

/// Byte-level fallback when parsing is not available.
fn byte_level_analysis(
    file_path: &str,
    base_content: &[u8],
    head_content: &[u8],
    overlay_content: &[u8],
) -> MergeAnalysis {
    let head_changed = base_content != head_content;
    let overlay_changed = base_content != overlay_content;

    if head_changed && overlay_changed && head_content != overlay_content {
        // Both sides changed the same file to different content.
        MergeAnalysis::Conflict {
            conflicts: vec![SemanticConflict {
                file_path: file_path.to_string(),
                symbol_name: "<entire file>".to_string(),
                our_change: SymbolChangeKind::Modified,
                their_change: SymbolChangeKind::Modified,
            }],
        }
    } else {
        // Either only one side changed, or both changed identically.
        // Use overlay content (our changes take precedence for non-conflicts).
        MergeAnalysis::AutoMerge {
            merged_content: if overlay_changed {
                overlay_content.to_vec()
            } else {
                head_content.to_vec()
            },
        }
    }
}

/// Build a map of qualified_name -> Symbol for quick lookup.
fn symbol_map(symbols: &[Symbol]) -> HashMap<&str, &Symbol> {
    symbols
        .iter()
        .map(|s| (s.qualified_name.as_str(), s))
        .collect()
}

/// Classify how a symbol changed from base to the given version.
fn classify_change(
    base: Option<&&Symbol>,
    current: Option<&&Symbol>,
) -> Option<SymbolChangeKind> {
    match (base, current) {
        (None, None) => None,
        (None, Some(_)) => Some(SymbolChangeKind::Added),
        (Some(_), None) => Some(SymbolChangeKind::Removed),
        (Some(b), Some(c)) => {
            if symbols_equivalent(b, c) {
                None // unchanged
            } else {
                Some(SymbolChangeKind::Modified)
            }
        }
    }
}

/// Check whether two symbols are semantically equivalent for merge purposes.
/// We compare span, kind, visibility, and signature — NOT the symbol ID.
fn symbols_equivalent(a: &Symbol, b: &Symbol) -> bool {
    a.qualified_name == b.qualified_name
        && a.kind == b.kind
        && a.visibility == b.visibility
        && a.span == b.span
        && a.signature == b.signature
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_level_no_conflict_when_only_overlay_changed() {
        let base = b"base content";
        let head = b"base content"; // unchanged
        let overlay = b"overlay content";

        match byte_level_analysis("test.txt", base, head, overlay) {
            MergeAnalysis::AutoMerge { merged_content } => {
                assert_eq!(merged_content, overlay.to_vec());
            }
            MergeAnalysis::Conflict { .. } => panic!("expected auto-merge"),
        }
    }

    #[test]
    fn byte_level_no_conflict_when_only_head_changed() {
        let base = b"base content";
        let head = b"head content";
        let overlay = b"base content"; // unchanged

        match byte_level_analysis("test.txt", base, head, overlay) {
            MergeAnalysis::AutoMerge { merged_content } => {
                assert_eq!(merged_content, head.to_vec());
            }
            MergeAnalysis::Conflict { .. } => panic!("expected auto-merge"),
        }
    }

    #[test]
    fn byte_level_conflict_when_both_changed_differently() {
        let base = b"base content";
        let head = b"head content";
        let overlay = b"overlay content";

        match byte_level_analysis("test.txt", base, head, overlay) {
            MergeAnalysis::Conflict { conflicts } => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].symbol_name, "<entire file>");
            }
            MergeAnalysis::AutoMerge { .. } => panic!("expected conflict"),
        }
    }

    #[test]
    fn byte_level_no_conflict_when_both_changed_identically() {
        let base = b"base content";
        let same = b"same content";

        match byte_level_analysis("test.txt", base, same, same) {
            MergeAnalysis::AutoMerge { .. } => {} // OK
            MergeAnalysis::Conflict { .. } => panic!("expected auto-merge"),
        }
    }

    #[test]
    fn classify_change_cases() {
        use dk_core::{Span, SymbolKind, Visibility};
        use std::path::PathBuf;
        use uuid::Uuid;

        let sym = Symbol {
            id: Uuid::new_v4(),
            name: "f".into(),
            qualified_name: "f".into(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            file_path: PathBuf::from("t.rs"),
            span: Span {
                start_byte: 0,
                end_byte: 10,
            },
            signature: None,
            doc_comment: None,
            parent: None,
            last_modified_by: None,
            last_modified_intent: None,
        };

        // None -> None => no change
        assert!(classify_change(None, None).is_none());

        // None -> Some => Added
        assert_eq!(
            classify_change(None, Some(&&sym)),
            Some(SymbolChangeKind::Added)
        );

        // Some -> None => Removed
        assert_eq!(
            classify_change(Some(&&sym), None),
            Some(SymbolChangeKind::Removed)
        );

        // Some -> Some (identical) => no change
        assert!(classify_change(Some(&&sym), Some(&&sym)).is_none());
    }
}

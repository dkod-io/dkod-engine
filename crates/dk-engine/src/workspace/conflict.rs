//! Semantic conflict detection for three-way merge.
//!
//! Instead of purely textual diff3, this module parses all three versions
//! of a file (base, head, overlay) with tree-sitter and compares the
//! resulting symbol tables. Conflicts arise when both sides modify,
//! add, or remove the *same* symbol.

use crate::conflict::ast_merge;
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
    // Try AST-level three-way merge first. This produces proper merged
    // content that combines non-overlapping symbol changes from both sides,
    // instead of returning only one side's content.
    let base_str = std::str::from_utf8(base_content).ok();
    let head_str = std::str::from_utf8(head_content).ok();
    let overlay_str = std::str::from_utf8(overlay_content).ok();

    if let (Some(base), Some(head), Some(overlay)) = (base_str, head_str, overlay_str) {
        if let Ok(result) = ast_merge::ast_merge(parser, file_path, base, head, overlay) {
            return match result.status {
                ast_merge::MergeStatus::Clean => {
                    // Guard: ast_merge reconstructs files from imports + symbols only.
                    // Top-level items the parser doesn't classify as symbols (const,
                    // static, type aliases, mod declarations, crate attributes) are
                    // silently dropped.  Detect content loss by checking that the
                    // merged output is at least 80% of the longer input's size and
                    // fall back to byte-level analysis when it isn't.
                    let merged_len = result.merged_content.len();
                    let max_input_len = head.len().max(overlay.len());
                    if max_input_len > 0 && merged_len * 100 / max_input_len < 80 {
                        byte_level_analysis(file_path, base_content, head_content, overlay_content)
                    } else {
                        MergeAnalysis::AutoMerge {
                            merged_content: result.merged_content.into_bytes(),
                        }
                    }
                }
                ast_merge::MergeStatus::Conflict => MergeAnalysis::Conflict {
                    conflicts: result
                        .conflicts
                        .into_iter()
                        .map(|c| {
                            // Infer change kinds from the three-way symbol versions:
                            // - version_a = head (their), version_b = overlay (our)
                            // - empty string means the symbol does not exist in that version
                            let their_change = infer_change_kind(&c.base, &c.version_a);
                            let our_change = infer_change_kind(&c.base, &c.version_b);
                            SemanticConflict {
                                file_path: file_path.to_string(),
                                symbol_name: c.qualified_name,
                                our_change,
                                their_change,
                            }
                        })
                        .collect(),
                },
            };
        }
    }

    // Fallback: byte-level comparison when AST merge is not available
    // (binary files, unsupported languages, or UTF-8 decode failure).
    byte_level_analysis(file_path, base_content, head_content, overlay_content)
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

/// Infer the [`SymbolChangeKind`] by comparing a symbol's base version to its
/// current version.  An empty string means the symbol does not exist in that
/// version of the file.
fn infer_change_kind(base: &str, current: &str) -> SymbolChangeKind {
    match (base.is_empty(), current.is_empty()) {
        // Symbol absent in base, present now — added
        (true, false) => SymbolChangeKind::Added,
        // Symbol present in base, absent now — removed
        (false, true) => SymbolChangeKind::Removed,
        // Both present — a genuine modification (content must differ; ast_merge
        // only emits conflicts when at least one side changed).
        (false, false) => SymbolChangeKind::Modified,
        // Both absent — ast_merge never generates this as a conflict; treat as
        // Modified defensively but this branch should be unreachable.
        (true, true) => SymbolChangeKind::Modified,
    }
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
    fn infer_change_kind_added() {
        assert_eq!(infer_change_kind("", "fn new() {}"), SymbolChangeKind::Added);
    }

    #[test]
    fn infer_change_kind_removed() {
        assert_eq!(infer_change_kind("fn old() {}", ""), SymbolChangeKind::Removed);
    }

    #[test]
    fn infer_change_kind_modified() {
        assert_eq!(
            infer_change_kind("fn foo() { 1 }", "fn foo() { 2 }"),
            SymbolChangeKind::Modified
        );
    }

    #[test]
    fn infer_change_kind_both_empty() {
        // Edge case: both empty — ast_merge never emits this, but return Modified defensively
        assert_eq!(infer_change_kind("", ""), SymbolChangeKind::Modified);
    }
}

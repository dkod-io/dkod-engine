//! Workspace merge with fast-path and rebase strategies.
//!
//! When the repository HEAD still equals the workspace's base commit, a
//! fast-forward merge applies the overlay directly via
//! `commit_tree_overlay`. When HEAD has advanced, a rebase path checks
//! each modified file for semantic conflicts and auto-merges where
//! possible.

use std::collections::HashMap;

use dk_core::Result;

use crate::git::GitRepository;
use crate::parser::ParserRegistry;
use crate::workspace::conflict::{analyze_file_conflict, MergeAnalysis, SemanticConflict};
use crate::workspace::session_workspace::SessionWorkspace;

// ── Result types ─────────────────────────────────────────────────────

/// Outcome of attempting to merge a workspace into the repository.
#[derive(Debug)]
pub enum WorkspaceMergeResult {
    /// HEAD == base_commit: overlay was committed directly.
    FastMerge { commit_hash: String },

    /// HEAD != base_commit, but all files auto-rebased successfully.
    RebaseMerge {
        commit_hash: String,
        /// File paths that were automatically rebased.
        auto_rebased_files: Vec<String>,
    },

    /// At least one file has semantic conflicts that need resolution.
    Conflicts {
        conflicts: Vec<SemanticConflict>,
    },
}

// ── Merge function ───────────────────────────────────────────────────

/// Attempt to merge a workspace's overlay into the repository.
///
/// Strategy:
/// 1. **Fast path** — If the repo HEAD equals the workspace's
///    `base_commit`, commit the overlay directly on top.
/// 2. **Rebase path** — If HEAD has advanced, compare each overlay file
///    against the HEAD version using three-way semantic conflict analysis.
///    Files with non-overlapping changes are auto-merged; files with
///    overlapping symbol changes produce conflicts.
pub fn merge_workspace(
    workspace: &SessionWorkspace,
    git_repo: &GitRepository,
    parser: &ParserRegistry,
    commit_message: &str,
    author_name: &str,
    author_email: &str,
) -> Result<WorkspaceMergeResult> {
    let head_hash = git_repo
        .head_hash()?
        .ok_or_else(|| dk_core::Error::Git("repository has no HEAD".into()))?;

    let overlay = workspace.overlay_for_tree();

    if overlay.is_empty() {
        return Err(dk_core::Error::Internal(
            "workspace has no changes to merge".into(),
        ));
    }

    // ── Fast path ────────────────────────────────────────────────
    if head_hash == workspace.base_commit {
        let commit_hash = git_repo.commit_tree_overlay(
            &workspace.base_commit,
            &overlay,
            &workspace.base_commit,
            commit_message,
            author_name,
            author_email,
        )?;

        return Ok(WorkspaceMergeResult::FastMerge { commit_hash });
    }

    // ── Rebase path ──────────────────────────────────────────────
    //
    // Batch-read all tree entries upfront so that consecutive lookups
    // against the same commit let gitoxide reuse the resolved tree
    // object from its internal cache, instead of re-resolving
    // commit→tree on every interleaved per-file call.
    let paths: Vec<&String> = overlay.iter().map(|(p, _)| p).collect();

    let mut base_entries: HashMap<&str, Option<Vec<u8>>> = HashMap::with_capacity(paths.len());
    for path in &paths {
        base_entries.insert(path.as_str(), git_repo.read_tree_entry(&workspace.base_commit, path).ok());
    }

    let mut head_entries: HashMap<&str, Option<Vec<u8>>> = HashMap::with_capacity(paths.len());
    for path in &paths {
        head_entries.insert(path.as_str(), git_repo.read_tree_entry(&head_hash, path).ok());
    }

    let mut all_conflicts = Vec::new();
    let mut auto_rebased = Vec::new();
    let mut rebased_overlay: Vec<(String, Option<Vec<u8>>)> = Vec::new();

    for (path, maybe_content) in &overlay {
        let base_content = base_entries.get(path.as_str()).and_then(|v| v.as_ref());
        let head_content = head_entries.get(path.as_str()).and_then(|v| v.as_ref());

        match maybe_content {
            None => {
                // Deletion — check if the file was also modified in head.
                match (base_content, head_content) {
                    (Some(base), Some(head)) => {
                        if base == head {
                            rebased_overlay.push((path.clone(), None));
                        } else {
                            all_conflicts.push(SemanticConflict {
                                file_path: path.clone(),
                                symbol_name: "<entire file>".to_string(),
                                our_change: crate::workspace::conflict::SymbolChangeKind::Removed,
                                their_change: crate::workspace::conflict::SymbolChangeKind::Modified,
                            });
                        }
                    }
                    _ => {
                        rebased_overlay.push((path.clone(), None));
                    }
                }
            }
            Some(overlay_content) => {
                match (base_content, head_content) {
                    (Some(base), Some(head)) => {
                        if base == head {
                            rebased_overlay.push((path.clone(), Some(overlay_content.clone())));
                        } else {
                            let analysis = analyze_file_conflict(
                                path,
                                base,
                                head,
                                overlay_content,
                                parser,
                            );

                            match analysis {
                                MergeAnalysis::AutoMerge { merged_content } => {
                                    rebased_overlay
                                        .push((path.clone(), Some(merged_content)));
                                    auto_rebased.push(path.clone());
                                }
                                MergeAnalysis::Conflict { conflicts } => {
                                    all_conflicts.extend(conflicts);
                                }
                            }
                        }
                    }
                    (None, Some(head_blob)) => {
                        if *head_blob == *overlay_content {
                            rebased_overlay.push((path.clone(), Some(overlay_content.clone())));
                        } else {
                            all_conflicts.push(SemanticConflict {
                                file_path: path.clone(),
                                symbol_name: "<entire file>".to_string(),
                                our_change: crate::workspace::conflict::SymbolChangeKind::Added,
                                their_change: crate::workspace::conflict::SymbolChangeKind::Added,
                            });
                        }
                    }
                    (None, None) => {
                        rebased_overlay.push((path.clone(), Some(overlay_content.clone())));
                    }
                    (Some(_), None) => {
                        all_conflicts.push(SemanticConflict {
                            file_path: path.clone(),
                            symbol_name: "<entire file>".to_string(),
                            our_change: crate::workspace::conflict::SymbolChangeKind::Modified,
                            their_change: crate::workspace::conflict::SymbolChangeKind::Removed,
                        });
                    }
                }
            }
        }
    }

    if !all_conflicts.is_empty() {
        return Ok(WorkspaceMergeResult::Conflicts {
            conflicts: all_conflicts,
        });
    }

    // All files rebased successfully — commit on top of HEAD.
    let commit_hash = git_repo.commit_tree_overlay(
        &head_hash,
        &rebased_overlay,
        &head_hash,
        commit_message,
        author_name,
        author_email,
    )?;

    Ok(WorkspaceMergeResult::RebaseMerge {
        commit_hash,
        auto_rebased_files: auto_rebased,
    })
}

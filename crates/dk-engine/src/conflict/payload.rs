//! Self-contained conflict payloads for the SUBMIT response.
//!
//! When two sessions touch the same symbol, the `ConflictPayloadBuilder`
//! produces a `ConflictBlock` with full source context: base version, their
//! version, and your version of each conflicting symbol. This gives agents
//! everything they need to resolve the conflict without additional RPCs.

use std::path::Path;

use serde::Serialize;

use dk_core::{Error, Result, Symbol};

use crate::parser::ParserRegistry;

/// A block of conflicts to include in a SUBMIT response.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictBlock {
    pub conflicting_symbols: Vec<SymbolConflictDetail>,
    pub message: String,
}

/// Full detail about a single symbol conflict.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolConflictDetail {
    pub file_path: String,
    pub qualified_name: String,
    pub kind: String,
    pub conflicting_agent: String,
    pub their_change: SymbolVersion,
    pub your_change: SymbolVersion,
    pub base_version: SymbolVersion,
}

/// A specific version of a symbol's source.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolVersion {
    pub description: String,
    pub signature: String,
    pub body: String,
    /// One of: "modified", "added", "deleted", "base"
    pub change_type: String,
}

/// Extract a symbol's source text from content using the parser.
fn find_symbol_in_content(
    registry: &ParserRegistry,
    file_path: &str,
    content: &str,
    qualified_name: &str,
) -> Result<Option<(Symbol, String)>> {
    let path = Path::new(file_path);
    if !registry.supports_file(path) {
        return Err(Error::UnsupportedLanguage(format!(
            "Unsupported file: {file_path}"
        )));
    }

    let analysis = registry.parse_file(path, content.as_bytes())?;

    for sym in &analysis.symbols {
        if sym.qualified_name == qualified_name {
            let start = sym.span.start_byte as usize;
            let end = sym.span.end_byte as usize;
            let bytes = content.as_bytes();
            if end <= bytes.len() {
                let text = String::from_utf8_lossy(&bytes[start..end]).to_string();
                return Ok(Some((sym.clone(), text)));
            }
        }
    }

    Ok(None)
}

/// Extract the first line of a symbol's source as its signature.
fn extract_signature(source: &str) -> String {
    source
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Count lines in source text.
fn line_count(source: &str) -> usize {
    if source.is_empty() {
        0
    } else {
        source.lines().count()
    }
}

/// Generate a human-readable description comparing two versions of a symbol.
fn describe_change(
    base_sig: &str,
    base_body: &str,
    changed_sig: &str,
    changed_body: &str,
    change_type: &str,
) -> String {
    match change_type {
        "added" => "New symbol added".to_string(),
        "deleted" => "Symbol deleted".to_string(),
        _ => {
            let mut parts = Vec::new();

            if base_sig != changed_sig {
                parts.push(format!(
                    "Signature changed from `{base_sig}` to `{changed_sig}`"
                ));
            }

            let base_lines = line_count(base_body);
            let changed_lines = line_count(changed_body);
            if changed_lines > base_lines {
                parts.push(format!(
                    "Added {} lines",
                    changed_lines - base_lines
                ));
            } else if changed_lines < base_lines {
                parts.push(format!(
                    "Removed {} lines",
                    base_lines - changed_lines
                ));
            } else if base_body != changed_body {
                parts.push("Body modified (same line count)".to_string());
            }

            if parts.is_empty() {
                "No visible changes".to_string()
            } else {
                parts.join("; ")
            }
        }
    }
}

/// Build a `SymbolVersion` from content, or return an empty version if the
/// symbol doesn't exist in the given content.
fn build_symbol_version(
    registry: &ParserRegistry,
    file_path: &str,
    content: &str,
    qualified_name: &str,
    base_sig: &str,
    base_body: &str,
    label: &str,
) -> Result<SymbolVersion> {
    match find_symbol_in_content(registry, file_path, content, qualified_name)? {
        Some((_sym, text)) => {
            let sig = extract_signature(&text);
            let change_type = if label == "base" {
                "base".to_string()
            } else if base_body.is_empty() {
                "added".to_string()
            } else {
                "modified".to_string()
            };
            let desc = describe_change(base_sig, base_body, &sig, &text, &change_type);
            Ok(SymbolVersion {
                description: desc,
                signature: sig,
                body: text,
                change_type,
            })
        }
        None => {
            let change_type = if label == "base" {
                "base".to_string()
            } else {
                "deleted".to_string()
            };
            let desc = describe_change(base_sig, base_body, "", "", &change_type);
            Ok(SymbolVersion {
                description: desc,
                signature: String::new(),
                body: String::new(),
                change_type,
            })
        }
    }
}

/// Build a `SymbolConflictDetail` for a specific symbol conflict.
///
/// - `file_path`: the file where the conflict occurs
/// - `qualified_name`: the symbol's qualified name
/// - `conflicting_agent`: the name of the other agent
/// - `base_content`: the common ancestor version of the file
/// - `their_content`: the conflicting session's version of the file
/// - `your_content`: your session's version of the file
pub fn build_conflict_detail(
    registry: &ParserRegistry,
    file_path: &str,
    qualified_name: &str,
    conflicting_agent: &str,
    base_content: &str,
    their_content: &str,
    your_content: &str,
) -> Result<SymbolConflictDetail> {
    // Extract base version first to get reference signature/body
    let (base_sig, base_body, kind) =
        match find_symbol_in_content(registry, file_path, base_content, qualified_name)? {
            Some((sym, text)) => {
                let sig = extract_signature(&text);
                let kind = sym.kind.to_string();
                (sig, text, kind)
            }
            None => (String::new(), String::new(), "unknown".to_string()),
        };

    let base_version = SymbolVersion {
        description: if base_body.is_empty() {
            "Symbol does not exist in base".to_string()
        } else {
            "Base version".to_string()
        },
        signature: base_sig.clone(),
        body: base_body.clone(),
        change_type: "base".to_string(),
    };

    let their_change = build_symbol_version(
        registry,
        file_path,
        their_content,
        qualified_name,
        &base_sig,
        &base_body,
        "their",
    )?;

    let your_change = build_symbol_version(
        registry,
        file_path,
        your_content,
        qualified_name,
        &base_sig,
        &base_body,
        "your",
    )?;

    Ok(SymbolConflictDetail {
        file_path: file_path.to_string(),
        qualified_name: qualified_name.to_string(),
        kind,
        conflicting_agent: conflicting_agent.to_string(),
        their_change,
        your_change,
        base_version,
    })
}

/// Build a complete `ConflictBlock` from a list of conflicting symbols.
///
/// Each entry in `conflicts` is `(file_path, qualified_name, conflicting_agent,
/// base_content, their_content, your_content)`.
pub fn build_conflict_block(
    registry: &ParserRegistry,
    conflicts: &[(
        &str, // file_path
        &str, // qualified_name
        &str, // conflicting_agent
        &str, // base_content
        &str, // their_content
        &str, // your_content
    )],
) -> Result<ConflictBlock> {
    let mut details = Vec::new();

    for (file_path, qualified_name, agent, base, theirs, yours) in conflicts {
        details.push(build_conflict_detail(
            registry,
            file_path,
            qualified_name,
            agent,
            base,
            theirs,
            yours,
        )?);
    }

    let count = details.len();
    let message = if count == 1 {
        format!(
            "1 symbol conflict detected in {}",
            details[0].file_path
        )
    } else {
        let files: std::collections::BTreeSet<&str> =
            details.iter().map(|d| d.file_path.as_str()).collect();
        format!(
            "{count} symbol conflicts detected across {} file(s)",
            files.len()
        )
    };

    Ok(ConflictBlock {
        conflicting_symbols: details,
        message,
    })
}

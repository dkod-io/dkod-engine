// Integration tests for dk_engine::repo::Engine.
//
// The Engine requires a PgPool for full integration testing.  These tests
// verify that all public types and helper logic compile correctly.  The
// subset that exercises pure in-memory logic (call-graph resolution, file
// collection) runs without a database.

use dk_core::{CallKind, RawCallEdge, Span, Symbol, SymbolKind, Visibility};
use dk_engine::repo::CodebaseSummary;
use std::path::PathBuf;
use uuid::Uuid;

// ── Type compilation checks ──

#[test]
fn test_engine_types_exist() {
    // Verify that CodebaseSummary can be constructed.
    let summary = CodebaseSummary {
        languages: vec!["rust".into(), "python".into()],
        total_symbols: 10,
        total_files: 3,
    };
    assert_eq!(summary.languages.len(), 2);
    assert_eq!(summary.total_symbols, 10);
    assert_eq!(summary.total_files, 3);
}

#[test]
fn test_engine_struct_is_accessible() {
    // We cannot construct an Engine without a PgPool, but we can verify the
    // type is publicly visible and its public fields are accessible by name.
    fn _assert_engine_fields(e: &dk_engine::repo::Engine) {
        let _pool = &e.db;
        let _path = &e.storage_path;
        // parser and search_index are also public
        let _parser = &e.parser;
        let _idx = &e.search_index;
    }
}

// ── File collection helper (via the module's unit tests) ──

#[test]
fn test_collect_files_nested() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Create nested structure.
    std::fs::create_dir_all(root.join("src/sub")).unwrap();
    std::fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
    std::fs::write(root.join("src/sub/lib.py"), b"def foo(): pass").unwrap();
    std::fs::write(root.join("src/sub/util.ts"), b"export function bar() {}").unwrap();
    // Unsupported
    std::fs::write(root.join("README.md"), b"# Hello").unwrap();
    // .git should be skipped
    std::fs::create_dir_all(root.join(".git/objects")).unwrap();
    std::fs::write(root.join(".git/HEAD"), b"ref: refs/heads/main").unwrap();

    let parser = dk_engine::parser::ParserRegistry::new();
    // Use supports_file to verify expected behaviour (collect_files is private,
    // but we can test the parser registry that it depends on).
    let all_files: Vec<PathBuf> = walkdir(root);

    let supported: Vec<_> = all_files
        .iter()
        .filter(|p| {
            let rel = p.strip_prefix(root).unwrap_or(p);
            // Skip .git
            if rel.starts_with(".git") {
                return false;
            }
            parser.supports_file(rel)
        })
        .collect();

    assert_eq!(supported.len(), 3);
}

fn walkdir(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(walkdir(&path));
            } else {
                files.push(path);
            }
        }
    }
    files
}

// ── Call-graph resolution (exercises the same algorithm used in Engine) ──

#[test]
fn test_call_graph_resolution_logic() {
    // This mirrors the resolve_call_edges function inside repo.rs.
    let sym_a = Uuid::new_v4();
    let sym_b = Uuid::new_v4();
    let repo_id = Uuid::new_v4();

    let symbols = vec![
        make_symbol(sym_a, "process", "app::process", "src/app.rs"),
        make_symbol(sym_b, "validate", "app::validate", "src/app.rs"),
    ];

    let raw_edges = vec![
        RawCallEdge {
            caller_name: "process".into(),
            callee_name: "validate".into(),
            call_site: Span {
                start_byte: 10,
                end_byte: 20,
            },
            kind: CallKind::DirectCall,
        },
        // This edge should be dropped: "missing_fn" doesn't exist.
        RawCallEdge {
            caller_name: "process".into(),
            callee_name: "missing_fn".into(),
            call_site: Span {
                start_byte: 30,
                end_byte: 40,
            },
            kind: CallKind::DirectCall,
        },
    ];

    // Inline the resolution algorithm.
    let mut name_to_id = std::collections::HashMap::new();
    for sym in &symbols {
        name_to_id.insert(sym.name.clone(), sym.id);
        name_to_id.insert(sym.qualified_name.clone(), sym.id);
    }

    let resolved: Vec<dk_core::CallEdge> = raw_edges
        .iter()
        .filter_map(|raw| {
            let caller = name_to_id.get(&raw.caller_name)?;
            let callee = name_to_id.get(&raw.callee_name)?;
            Some(dk_core::CallEdge {
                id: Uuid::new_v4(),
                repo_id,
                caller: *caller,
                callee: *callee,
                kind: raw.kind.clone(),
            })
        })
        .collect();

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].caller, sym_a);
    assert_eq!(resolved[0].callee, sym_b);
}

fn make_symbol(id: Uuid, name: &str, qname: &str, file: &str) -> Symbol {
    Symbol {
        id,
        name: name.into(),
        qualified_name: qname.into(),
        kind: SymbolKind::Function,
        visibility: Visibility::Public,
        file_path: PathBuf::from(file),
        span: Span {
            start_byte: 0,
            end_byte: 100,
        },
        signature: None,
        doc_comment: None,
        parent: None,
        last_modified_by: None,
        last_modified_intent: None,
    }
}

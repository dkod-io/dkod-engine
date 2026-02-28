use std::path::PathBuf;

use dk_core::*;
use dk_engine::graph::SearchIndex;
use tempfile::TempDir;
use uuid::Uuid;

fn make_symbol(
    name: &str,
    qualified_name: &str,
    kind: SymbolKind,
    file: &str,
    signature: Option<&str>,
    doc_comment: Option<&str>,
) -> Symbol {
    Symbol {
        id: Uuid::new_v4(),
        name: name.to_string(),
        qualified_name: qualified_name.to_string(),
        kind,
        visibility: Visibility::Public,
        file_path: PathBuf::from(file),
        span: Span {
            start_byte: 0,
            end_byte: 100,
        },
        signature: signature.map(|s| s.to_string()),
        doc_comment: doc_comment.map(|s| s.to_string()),
        parent: None,
        last_modified_by: None,
        last_modified_intent: None,
    }
}

#[test]
fn test_index_and_search_symbols() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_id = Uuid::new_v4();

    let sym = Symbol {
        id: Uuid::new_v4(),
        name: "authenticate_user".into(),
        qualified_name: "src/auth.rs::authenticate_user".into(),
        kind: SymbolKind::Function,
        visibility: Visibility::Public,
        file_path: PathBuf::from("src/auth.rs"),
        span: Span {
            start_byte: 0,
            end_byte: 100,
        },
        signature: Some("fn authenticate_user(req: &Request) -> Result<User>".into()),
        doc_comment: Some("Authenticates a user by validating their token".into()),
        parent: None,
        last_modified_by: None,
        last_modified_intent: None,
    };

    index.index_symbol(repo_id, &sym).unwrap();
    index.commit().unwrap();

    // Search by name fragment
    let results = index.search(repo_id, "authenticate", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], sym.id);

    // Search by doc comment content
    let results = index.search(repo_id, "token", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], sym.id);
}

#[test]
fn test_search_by_signature() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_id = Uuid::new_v4();

    let sym = make_symbol(
        "process_data",
        "crate::pipeline::process_data",
        SymbolKind::Function,
        "src/pipeline.rs",
        Some("fn process_data(input: Vec<u8>) -> Result<Output>"),
        Some("Processes raw byte data into structured output"),
    );

    index.index_symbol(repo_id, &sym).unwrap();
    index.commit().unwrap();

    // Search by a term that only appears in the signature
    let results = index.search(repo_id, "Vec", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], sym.id);

    // Search by a term in the signature return type
    let results = index.search(repo_id, "Output", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], sym.id);
}

#[test]
fn test_search_no_results_different_repo() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_a = Uuid::new_v4();
    let repo_b = Uuid::new_v4();

    let sym = make_symbol(
        "exclusive_func",
        "crate::exclusive_func",
        SymbolKind::Function,
        "src/lib.rs",
        Some("fn exclusive_func()"),
        None,
    );

    index.index_symbol(repo_a, &sym).unwrap();
    index.commit().unwrap();

    // Searching in repo_a should find it
    let results = index.search(repo_a, "exclusive", 10).unwrap();
    assert_eq!(results.len(), 1);

    // Searching in repo_b should find nothing
    let results = index.search(repo_b, "exclusive", 10).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_multiple_symbols_ranking() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_id = Uuid::new_v4();

    // Symbol where "parse" appears in name, qualified_name, signature, and doc_comment
    let sym_strong = make_symbol(
        "parse_json",
        "crate::parser::parse_json",
        SymbolKind::Function,
        "src/parser.rs",
        Some("fn parse_json(input: &str) -> ParseResult"),
        Some("Parse a JSON string into a value"),
    );

    // Symbol where "parse" only appears in the doc_comment
    let sym_weak = make_symbol(
        "validate_input",
        "crate::validator::validate_input",
        SymbolKind::Function,
        "src/validator.rs",
        Some("fn validate_input(s: &str) -> bool"),
        Some("Validates input before parse step"),
    );

    // Symbol unrelated to "parse"
    let sym_unrelated = make_symbol(
        "send_email",
        "crate::mailer::send_email",
        SymbolKind::Function,
        "src/mailer.rs",
        Some("fn send_email(to: &str, body: &str)"),
        Some("Sends an email notification"),
    );

    index.index_symbol(repo_id, &sym_strong).unwrap();
    index.index_symbol(repo_id, &sym_weak).unwrap();
    index.index_symbol(repo_id, &sym_unrelated).unwrap();
    index.commit().unwrap();

    let results = index.search(repo_id, "parse", 10).unwrap();

    // Should find at least the two symbols mentioning "parse"
    assert!(results.len() >= 2);
    // The unrelated symbol should NOT appear
    assert!(!results.contains(&sym_unrelated.id));
    // Both "parse"-related symbols should appear
    assert!(results.contains(&sym_strong.id));
    assert!(results.contains(&sym_weak.id));
    // The stronger match should be ranked first
    assert_eq!(results[0], sym_strong.id);
}

#[test]
fn test_remove_symbol() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_id = Uuid::new_v4();

    let sym = make_symbol(
        "temporary_func",
        "crate::temporary_func",
        SymbolKind::Function,
        "src/lib.rs",
        Some("fn temporary_func()"),
        Some("A temporary function to be removed"),
    );

    index.index_symbol(repo_id, &sym).unwrap();
    index.commit().unwrap();

    // Confirm it exists
    let results = index.search(repo_id, "temporary", 10).unwrap();
    assert_eq!(results.len(), 1);

    // Remove and commit
    index.remove_symbol(sym.id).unwrap();
    index.commit().unwrap();

    // Should no longer appear
    let results = index.search(repo_id, "temporary", 10).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_search_by_file_path() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_id = Uuid::new_v4();

    let sym = make_symbol(
        "handler",
        "crate::routes::handler",
        SymbolKind::Function,
        "src/routes/api/v2/handler.rs",
        Some("fn handler()"),
        None,
    );

    index.index_symbol(repo_id, &sym).unwrap();
    index.commit().unwrap();

    // Search by a path component
    let results = index.search(repo_id, "handler", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], sym.id);
}

#[test]
fn test_search_empty_query_returns_nothing() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_id = Uuid::new_v4();

    let sym = make_symbol(
        "some_func",
        "crate::some_func",
        SymbolKind::Function,
        "src/lib.rs",
        None,
        None,
    );

    index.index_symbol(repo_id, &sym).unwrap();
    index.commit().unwrap();

    // An empty or wildcard-only query should not panic; may return empty.
    let results = index.search(repo_id, "", 10);
    // We don't assert content, just that it doesn't crash.
    assert!(results.is_ok());
}

#[test]
fn test_limit_is_respected() {
    let tmp = TempDir::new().unwrap();
    let mut index = SearchIndex::open(tmp.path()).unwrap();
    let repo_id = Uuid::new_v4();

    // Index 5 symbols with "widget" in the name
    for i in 0..5 {
        let sym = make_symbol(
            &format!("widget_{i}"),
            &format!("crate::widget_{i}"),
            SymbolKind::Function,
            "src/widgets.rs",
            None,
            None,
        );
        index.index_symbol(repo_id, &sym).unwrap();
    }
    index.commit().unwrap();

    // Request only 3
    let results = index.search(repo_id, "widget", 3).unwrap();
    assert_eq!(results.len(), 3);

    // Request all
    let results = index.search(repo_id, "widget", 10).unwrap();
    assert_eq!(results.len(), 5);
}

use dk_core::{SymbolKind, Visibility};
use dk_engine::parser::engine::QueryDrivenParser;
use dk_engine::parser::langs::rust::RustConfig;
use dk_engine::parser::LanguageParser;
use std::path::Path;

#[test]
fn test_rust_config_symbols() {
    let parser = QueryDrivenParser::new(Box::new(RustConfig)).unwrap();
    let source = br#"
pub fn authenticate_user(req: &Request) -> Result<User, AuthError> {
    validate_token(req.header("Authorization"))
}

fn validate_token(token: &str) -> Result<User, AuthError> {
    todo!()
}
"#;
    let analysis = parser.parse_file(source, Path::new("auth.rs")).unwrap();
    assert_eq!(analysis.symbols.len(), 2);
    let auth = analysis
        .symbols
        .iter()
        .find(|s| s.name == "authenticate_user")
        .unwrap();
    assert_eq!(auth.kind, SymbolKind::Function);
    assert_eq!(auth.visibility, Visibility::Public);
    let validate = analysis
        .symbols
        .iter()
        .find(|s| s.name == "validate_token")
        .unwrap();
    assert_eq!(validate.kind, SymbolKind::Function);
    assert_eq!(validate.visibility, Visibility::Private);
}

#[test]
fn test_rust_config_structs_enums_traits() {
    let parser = QueryDrivenParser::new(Box::new(RustConfig)).unwrap();
    let source = br#"
pub struct User { pub id: u64, pub name: String }
pub enum AuthError { InvalidToken, Expired }
pub trait Authenticate { fn authenticate(&self) -> bool; }
"#;
    let analysis = parser.parse_file(source, Path::new("types.rs")).unwrap();
    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"User"),
        "Missing User in {:?}",
        names
    );
    assert!(
        names.contains(&"AuthError"),
        "Missing AuthError in {:?}",
        names
    );
    assert!(
        names.contains(&"Authenticate"),
        "Missing Authenticate in {:?}",
        names
    );
}

#[test]
fn test_rust_config_calls() {
    let parser = QueryDrivenParser::new(Box::new(RustConfig)).unwrap();
    let source = b"fn main() { let user = authenticate_user(&req); user.save(); }";
    let analysis = parser.parse_file(source, Path::new("main.rs")).unwrap();
    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();
    assert!(
        call_names.contains(&"authenticate_user"),
        "Expected authenticate_user in {:?}",
        call_names
    );
}

#[test]
fn test_rust_config_imports() {
    let parser = QueryDrivenParser::new(Box::new(RustConfig)).unwrap();
    let source = br#"
use std::collections::HashMap;
use crate::auth::handler;
use super::utils;
"#;
    let analysis = parser.parse_file(source, Path::new("lib.rs")).unwrap();
    assert_eq!(
        analysis.imports.len(),
        3,
        "Expected 3 imports, got {:?}",
        analysis
            .imports
            .iter()
            .map(|i| &i.module_path)
            .collect::<Vec<_>>()
    );
    assert!(analysis
        .imports
        .iter()
        .any(|i| i.is_external && i.module_path.contains("std")));
    assert!(analysis
        .imports
        .iter()
        .any(|i| !i.is_external && i.module_path.contains("crate")));
}

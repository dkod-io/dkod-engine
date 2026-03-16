use dk_core::{SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_rust_functions() {
    let registry = ParserRegistry::new();
    let source = br#"
pub fn authenticate_user(req: &Request) -> Result<User, AuthError> {
    let token = req.header("Authorization");
    validate_token(token)
}

fn validate_token(token: &str) -> Result<User, AuthError> {
    todo!()
}
"#;
    let analysis = registry.parse_file(Path::new("auth.rs"), source).unwrap();
    assert_eq!(analysis.symbols.len(), 2);
    let auth_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "authenticate_user")
        .unwrap();
    assert_eq!(auth_fn.kind, SymbolKind::Function);
    assert_eq!(auth_fn.visibility, Visibility::Public);
    let validate_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "validate_token")
        .unwrap();
    assert_eq!(validate_fn.kind, SymbolKind::Function);
    assert_eq!(validate_fn.visibility, Visibility::Private);
}

#[test]
fn test_extract_rust_structs_and_enums() {
    let registry = ParserRegistry::new();
    let source = br#"
pub struct User {
    pub id: u64,
    pub name: String,
}

pub enum AuthError {
    InvalidToken,
    Expired,
}

pub trait Authenticate {
    fn authenticate(&self) -> bool;
}
"#;
    let analysis = registry.parse_file(Path::new("types.rs"), source).unwrap();
    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"User"));
    assert!(names.contains(&"AuthError"));
    assert!(names.contains(&"Authenticate"));
}

#[test]
fn test_extract_rust_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
fn main() {
    let user = authenticate_user(&req);
    user.save();
}
"#;
    let analysis = registry.parse_file(Path::new("main.rs"), source).unwrap();
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
fn test_extract_rust_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
use std::collections::HashMap;
use crate::auth::handler;
use super::utils;
"#;
    let analysis = registry.parse_file(Path::new("lib.rs"), source).unwrap();
    assert_eq!(analysis.imports.len(), 3);
    // std is external, crate:: and super:: are internal
    assert!(analysis
        .imports
        .iter()
        .any(|i| i.is_external && i.module_path.contains("std")));
    assert!(analysis
        .imports
        .iter()
        .any(|i| !i.is_external && i.module_path.contains("crate")));
}

use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::langs::typescript::TypeScriptParser;
use dk_engine::parser::LanguageParser;
use std::path::Path;

#[test]
fn test_extract_ts_functions_and_classes() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
export function authenticateUser(req: Request): Promise<User> {
    const token = req.headers.get("Authorization");
    return validateToken(token);
}

export class AuthService {
    private secret: string;
    constructor(secret: string) {
        this.secret = secret;
    }
    async validate(token: string): Promise<boolean> {
        return true;
    }
}

export interface User {
    id: number;
    name: string;
}

export type AuthResult = User | null;

const MAX_RETRIES = 3;
"#;
    let analysis = parser.parse_file(source, Path::new("auth.ts")).unwrap();
    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"authenticateUser"),
        "Missing authenticateUser, got: {:?}",
        names
    );
    assert!(
        names.contains(&"AuthService"),
        "Missing AuthService, got: {:?}",
        names
    );
    assert!(
        names.contains(&"User"),
        "Missing User, got: {:?}",
        names
    );
    assert!(
        names.contains(&"AuthResult"),
        "Missing AuthResult, got: {:?}",
        names
    );
    assert!(
        names.contains(&"MAX_RETRIES"),
        "Missing MAX_RETRIES, got: {:?}",
        names
    );
}

#[test]
fn test_ts_visibility() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
export function publicFn() {}
function privateFn() {}
"#;
    let analysis = parser.parse_file(source, Path::new("test.ts")).unwrap();
    let public_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "publicFn")
        .unwrap();
    assert_eq!(public_fn.visibility, Visibility::Public);
    let private_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "privateFn")
        .unwrap();
    assert_eq!(private_fn.visibility, Visibility::Private);
}

#[test]
fn test_ts_symbol_kinds() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
export function authenticateUser(req: Request): Promise<User> {
    return validateToken(req);
}

export class AuthService {
    private secret: string;
}

export interface User {
    id: number;
}

export type AuthResult = User | null;

const MAX_RETRIES = 3;
"#;
    let analysis = parser.parse_file(source, Path::new("auth.ts")).unwrap();

    let auth_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "authenticateUser")
        .unwrap();
    assert_eq!(auth_fn.kind, SymbolKind::Function);
    assert_eq!(auth_fn.visibility, Visibility::Public);

    let auth_svc = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AuthService")
        .unwrap();
    assert_eq!(auth_svc.kind, SymbolKind::Class);
    assert_eq!(auth_svc.visibility, Visibility::Public);

    let user = analysis
        .symbols
        .iter()
        .find(|s| s.name == "User")
        .unwrap();
    assert_eq!(user.kind, SymbolKind::Interface);
    assert_eq!(user.visibility, Visibility::Public);

    let auth_result = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AuthResult")
        .unwrap();
    assert_eq!(auth_result.kind, SymbolKind::TypeAlias);
    assert_eq!(auth_result.visibility, Visibility::Public);

    let max_retries = analysis
        .symbols
        .iter()
        .find(|s| s.name == "MAX_RETRIES")
        .unwrap();
    assert_eq!(max_retries.kind, SymbolKind::Const);
    assert_eq!(max_retries.visibility, Visibility::Private);
}

#[test]
fn test_extract_ts_calls() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
function main() {
    const user = authenticateUser(req);
    console.log(user.name);
}
"#;
    let analysis = parser.parse_file(source, Path::new("main.ts")).unwrap();
    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();
    assert!(
        call_names.contains(&"authenticateUser"),
        "Expected authenticateUser in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"log"),
        "Expected log (method call on console) in {:?}",
        call_names
    );

    // Check call kinds
    let auth_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "authenticateUser")
        .unwrap();
    assert_eq!(auth_call.kind, CallKind::DirectCall);
    assert_eq!(auth_call.caller_name, "main");

    let log_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "log")
        .unwrap();
    assert_eq!(log_call.kind, CallKind::MethodCall);
    assert_eq!(log_call.caller_name, "main");
}

#[test]
fn test_extract_ts_constructor_calls() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
function setup() {
    const svc = new AuthService("secret");
}
"#;
    let analysis = parser.parse_file(source, Path::new("setup.ts")).unwrap();
    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();
    assert!(
        call_names.contains(&"AuthService"),
        "Expected AuthService constructor call in {:?}",
        call_names
    );
}

#[test]
fn test_extract_ts_imports() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
import { Router } from 'express';
import { handler } from './auth/handler';
import * as utils from '../utils';
"#;
    let analysis = parser.parse_file(source, Path::new("app.ts")).unwrap();
    assert!(
        analysis.imports.len() >= 2,
        "Expected at least 2 imports, got: {:?}",
        analysis.imports.len()
    );
    assert!(
        analysis.imports.iter().any(|i| i.is_external),
        "Should have external import (express)"
    );
    assert!(
        analysis.imports.iter().any(|i| !i.is_external),
        "Should have internal import (./auth)"
    );

    // Check specific imports
    let router_import = analysis
        .imports
        .iter()
        .find(|i| i.imported_name == "Router")
        .expect("Should find Router import");
    assert_eq!(router_import.module_path, "express");
    assert!(router_import.is_external);

    let handler_import = analysis
        .imports
        .iter()
        .find(|i| i.imported_name == "handler")
        .expect("Should find handler import");
    assert_eq!(handler_import.module_path, "./auth/handler");
    assert!(!handler_import.is_external);

    // Namespace import: import * as utils from '../utils'
    let utils_import = analysis
        .imports
        .iter()
        .find(|i| i.alias.as_deref() == Some("utils"))
        .expect("Should find utils namespace import");
    assert_eq!(utils_import.module_path, "../utils");
    assert!(!utils_import.is_external);
}

#[test]
fn test_ts_dedup_qualified_names() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
const a = 1;
const a = 2;
const a = 3;
"#;
    let analysis = parser.parse_file(source, Path::new("dedup.ts")).unwrap();
    let names: Vec<&str> = analysis
        .symbols
        .iter()
        .map(|s| s.qualified_name.as_str())
        .collect();

    // First occurrence keeps original name, subsequent get #N suffix
    assert!(names.contains(&"a"), "Expected 'a' in {:?}", names);
    assert!(names.contains(&"a#2"), "Expected 'a#2' in {:?}", names);
    assert!(names.contains(&"a#3"), "Expected 'a#3' in {:?}", names);
}

#[test]
fn test_parse_file_full() {
    let parser = TypeScriptParser::new().unwrap();
    let source = br#"
import { Router } from 'express';

export function handleRequest(req: Request) {
    const result = processData(req.body);
    console.log(result);
    return result;
}

const TIMEOUT = 5000;
"#;
    let analysis = parser.parse_file(source, Path::new("handler.ts")).unwrap();

    // Symbols
    let symbol_names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(symbol_names.contains(&"handleRequest"));
    assert!(symbol_names.contains(&"TIMEOUT"));

    // Calls
    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();
    assert!(
        call_names.contains(&"processData"),
        "Expected processData in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"log"),
        "Expected log in {:?}",
        call_names
    );

    // Imports
    assert!(
        !analysis.imports.is_empty(),
        "Expected at least one import"
    );

    // Types: stub, should be empty
    assert!(analysis.types.is_empty(), "Types should be empty (stub)");
}

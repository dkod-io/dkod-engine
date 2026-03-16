use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_ts_functions_and_classes() {
    let registry = ParserRegistry::new();
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
    let analysis = registry.parse_file(Path::new("auth.ts"), source).unwrap();
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
    let registry = ParserRegistry::new();
    let source = br#"
export function publicFn() {}
function privateFn() {}
"#;
    let analysis = registry.parse_file(Path::new("test.ts"), source).unwrap();
    let public_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "publicFn")
        .unwrap();
    assert_eq!(public_fn.visibility, dk_core::Visibility::Public);
    let private_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "privateFn")
        .unwrap();
    assert_eq!(private_fn.visibility, dk_core::Visibility::Private);
}

#[test]
fn test_extract_ts_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
function main() {
    const user = authenticateUser(req);
    console.log(user.name);
}
"#;
    let analysis = registry.parse_file(Path::new("main.ts"), source).unwrap();
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
}

#[test]
fn test_extract_ts_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
import { Router } from 'express';
import { handler } from './auth/handler';
import * as utils from '../utils';
"#;
    let analysis = registry.parse_file(Path::new("app.ts"), source).unwrap();
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
}

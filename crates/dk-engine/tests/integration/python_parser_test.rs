use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_symbols() {
    let registry = ParserRegistry::new();
    let source = br#"
MAX_RETRIES = 3
_internal_flag = True

def authenticate_user(request):
    """Authenticate a user from the request."""
    token = request.headers.get("Authorization")
    return validate_token(token)

def _private_helper():
    pass

class AuthService:
    """Service for authentication."""
    def __init__(self, secret):
        self.secret = secret

    def validate(self, token):
        return True

@login_required
def protected_view(request):
    return render(request, "home.html")

@app.route("/api")
class ApiController:
    pass
"#;
    let analysis = registry
        .parse_file(Path::new("auth.py"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();

    // Functions
    assert!(
        names.contains(&"authenticate_user"),
        "Missing authenticate_user, got: {:?}",
        names
    );
    assert!(
        names.contains(&"_private_helper"),
        "Missing _private_helper, got: {:?}",
        names
    );

    // Classes
    assert!(
        names.contains(&"AuthService"),
        "Missing AuthService, got: {:?}",
        names
    );

    // Decorated definitions
    assert!(
        names.contains(&"protected_view"),
        "Missing protected_view, got: {:?}",
        names
    );
    assert!(
        names.contains(&"ApiController"),
        "Missing ApiController, got: {:?}",
        names
    );

    // Module-level variables
    assert!(
        names.contains(&"MAX_RETRIES"),
        "Missing MAX_RETRIES, got: {:?}",
        names
    );
    assert!(
        names.contains(&"_internal_flag"),
        "Missing _internal_flag, got: {:?}",
        names
    );

    // Check kinds
    let auth_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "authenticate_user")
        .unwrap();
    assert_eq!(auth_fn.kind, SymbolKind::Function);
    assert_eq!(auth_fn.visibility, Visibility::Public);

    let private_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "_private_helper")
        .unwrap();
    assert_eq!(private_fn.kind, SymbolKind::Function);
    assert_eq!(private_fn.visibility, Visibility::Private);

    let auth_class = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AuthService")
        .unwrap();
    assert_eq!(auth_class.kind, SymbolKind::Class);
    assert_eq!(auth_class.visibility, Visibility::Public);

    let max_retries = analysis
        .symbols
        .iter()
        .find(|s| s.name == "MAX_RETRIES")
        .unwrap();
    assert_eq!(max_retries.kind, SymbolKind::Variable);
    assert_eq!(max_retries.visibility, Visibility::Public);

    let internal_flag = analysis
        .symbols
        .iter()
        .find(|s| s.name == "_internal_flag")
        .unwrap();
    assert_eq!(internal_flag.kind, SymbolKind::Variable);
    assert_eq!(internal_flag.visibility, Visibility::Private);

    // Check docstring extraction
    assert!(
        auth_fn.doc_comment.is_some(),
        "authenticate_user should have a docstring"
    );
    assert!(
        auth_fn
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("Authenticate a user"),
        "Docstring should contain 'Authenticate a user', got: {:?}",
        auth_fn.doc_comment
    );
}

#[test]
fn test_extract_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
def main():
    user = authenticate_user(request)
    user.save()
    result = MyClass()
    print("hello")
    os.path.join("/tmp", "file")
"#;
    let analysis = registry
        .parse_file(Path::new("main.py"), source)
        .unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    // Direct calls
    assert!(
        call_names.contains(&"authenticate_user"),
        "Expected authenticate_user in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"print"),
        "Expected print in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"MyClass"),
        "Expected MyClass in {:?}",
        call_names
    );

    // Method calls
    assert!(
        call_names.contains(&"save"),
        "Expected save (method call) in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"join"),
        "Expected join (method call) in {:?}",
        call_names
    );

    // Check call kinds
    let save_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "save")
        .unwrap();
    assert_eq!(save_call.kind, CallKind::MethodCall);

    let auth_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "authenticate_user")
        .unwrap();
    assert_eq!(auth_call.kind, CallKind::DirectCall);

    // Check caller names
    assert_eq!(auth_call.caller_name, "main");
    assert_eq!(save_call.caller_name, "main");
}

#[test]
fn test_extract_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
import os
import sys
from os.path import join, exists
from collections import OrderedDict
from .local_module import helper
from ..parent import utils
"#;
    let analysis = registry
        .parse_file(Path::new("app.py"), source)
        .unwrap();

    assert!(
        analysis.imports.len() >= 6,
        "Expected at least 6 imports, got: {} => {:?}",
        analysis.imports.len(),
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:{}", i.module_path, i.imported_name))
            .collect::<Vec<_>>()
    );

    // `import os` → external
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "os" && i.imported_name == "os" && i.is_external),
        "Should have external import 'os'"
    );

    // `import sys` → external
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "sys" && i.imported_name == "sys" && i.is_external),
        "Should have external import 'sys'"
    );

    // `from os.path import join` → external
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "os.path"
                && i.imported_name == "join"
                && i.is_external),
        "Should have external import 'join' from 'os.path'"
    );

    // `from os.path import exists` → external
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "os.path"
                && i.imported_name == "exists"
                && i.is_external),
        "Should have external import 'exists' from 'os.path'"
    );

    // `from .local_module import helper` → internal (relative)
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == ".local_module"
                && i.imported_name == "helper"
                && !i.is_external),
        "Should have internal import 'helper' from '.local_module'"
    );

    // `from ..parent import utils` → internal (relative)
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "..parent"
                && i.imported_name == "utils"
                && !i.is_external),
        "Should have internal import 'utils' from '..parent'"
    );
}

#[test]
fn test_parse_file() {
    let registry = ParserRegistry::new();
    let source = br#"
import json
from pathlib import Path

MAX_TIMEOUT = 30

class Config:
    """Application configuration."""
    def __init__(self, path):
        self.path = path

    def load(self):
        with open(self.path) as f:
            return json.load(f)

def create_config(path):
    cfg = Config(path)
    cfg.load()
    return cfg
"#;
    let analysis = registry
        .parse_file(Path::new("config.py"), source)
        .unwrap();

    // Symbols: Config class, create_config function, MAX_TIMEOUT variable
    assert!(
        analysis.symbols.len() >= 3,
        "Expected at least 3 symbols, got: {} => {:?}",
        analysis.symbols.len(),
        analysis
            .symbols
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>()
    );

    let symbol_names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(symbol_names.contains(&"Config"));
    assert!(symbol_names.contains(&"create_config"));
    assert!(symbol_names.contains(&"MAX_TIMEOUT"));

    // Calls: Config(), cfg.load(), json.load(), open()
    assert!(
        !analysis.calls.is_empty(),
        "Expected at least some calls"
    );

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();
    assert!(
        call_names.contains(&"Config"),
        "Expected Config constructor call in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"load"),
        "Expected load method call in {:?}",
        call_names
    );

    // Imports: json, Path from pathlib
    assert!(
        analysis.imports.len() >= 2,
        "Expected at least 2 imports, got: {}",
        analysis.imports.len()
    );

    // Types: stub, should be empty
    assert!(
        analysis.types.is_empty(),
        "Types should be empty (stub)"
    );
}

#[test]
fn test_decorator_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
@app.route("/api")
def api_handler():
    pass

@login_required
def protected():
    pass
"#;
    let analysis = registry
        .parse_file(Path::new("views.py"), source)
        .unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    // @login_required is a direct call to the decorator
    assert!(
        call_names.contains(&"login_required"),
        "Expected login_required decorator call in {:?}",
        call_names
    );

    // @app.route("/api") is a method call to route
    assert!(
        call_names.contains(&"route"),
        "Expected route decorator method call in {:?}",
        call_names
    );
}

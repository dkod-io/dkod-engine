use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_php_classes_and_interfaces() {
    let registry = ParserRegistry::new();
    let source = br#"<?php
namespace App\Models;

use Illuminate\Database\Eloquent\Model;

// UserService handles user operations.
class UserService {
    public function processRequest($req) { }
    private function validate($req) { }
}

interface AuthProvider {
    public function authenticate($token);
}

enum Status {
    case Active;
    case Inactive;
}

function helperFunction() { }
"#;
    let analysis = registry
        .parse_file(Path::new("UserService.php"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();

    // Class
    assert!(
        names.contains(&"UserService"),
        "Missing UserService class, got: {:?}",
        names
    );
    let user_svc = analysis
        .symbols
        .iter()
        .find(|s| s.name == "UserService" && s.kind == SymbolKind::Class)
        .unwrap();
    assert_eq!(user_svc.visibility, Visibility::Public);

    // Interface
    assert!(
        names.contains(&"AuthProvider"),
        "Missing AuthProvider interface, got: {:?}",
        names
    );
    let auth_provider = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AuthProvider")
        .unwrap();
    assert_eq!(auth_provider.kind, SymbolKind::Interface);

    // Enum
    assert!(
        names.contains(&"Status"),
        "Missing Status enum, got: {:?}",
        names
    );
    let status = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Status")
        .unwrap();
    assert_eq!(status.kind, SymbolKind::Enum);

    // Methods
    assert!(
        names.contains(&"processRequest"),
        "Missing processRequest method, got: {:?}",
        names
    );
    let process_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "processRequest")
        .unwrap();
    assert_eq!(process_fn.kind, SymbolKind::Function);
    assert_eq!(process_fn.visibility, Visibility::Public);

    // Standalone function
    assert!(
        names.contains(&"helperFunction"),
        "Missing helperFunction, got: {:?}",
        names
    );
    let helper_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "helperFunction")
        .unwrap();
    assert_eq!(helper_fn.kind, SymbolKind::Function);

    // Namespace
    assert!(
        names.iter().any(|n| n.contains("App")),
        "Missing namespace, got: {:?}",
        names
    );

    // Doc comment
    assert!(
        user_svc.doc_comment.is_some(),
        "UserService should have a doc comment"
    );
    assert!(
        user_svc
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("handles user operations"),
        "Doc comment should contain 'handles user operations', got: {:?}",
        user_svc.doc_comment
    );
}

#[test]
fn test_extract_php_visibility() {
    let registry = ParserRegistry::new();
    let source = br#"<?php
class Config {
    public function publicMethod() {}
    protected function protectedMethod() {}
    private function privateMethod() {}
    function defaultMethod() {}
}
"#;
    let analysis = registry
        .parse_file(Path::new("Config.php"), source)
        .unwrap();

    let public_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "publicMethod")
        .unwrap();
    assert_eq!(public_fn.visibility, Visibility::Public);

    let protected_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "protectedMethod")
        .unwrap();
    assert_eq!(
        protected_fn.visibility,
        Visibility::Public,
        "protected should map to Public"
    );

    let private_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "privateMethod")
        .unwrap();
    assert_eq!(private_fn.visibility, Visibility::Private);

    let default_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "defaultMethod")
        .unwrap();
    assert_eq!(
        default_fn.visibility,
        Visibility::Public,
        "no modifier should map to Public (PHP convention)"
    );
}

#[test]
fn test_extract_php_calls() {
    let registry = ParserRegistry::new();
    let source = br#"<?php
function main() {
    $service = new UserService("admin");
    $service->processRequest($req);
    validate($req);
}
"#;
    let analysis = registry
        .parse_file(Path::new("main.php"), source)
        .unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    // Constructor: new UserService(...)
    assert!(
        call_names.contains(&"UserService"),
        "Expected UserService constructor call in {:?}",
        call_names
    );

    // Direct call: validate()
    assert!(
        call_names.contains(&"validate"),
        "Expected validate in {:?}",
        call_names
    );

    // Member call: $service->processRequest()
    assert!(
        call_names.contains(&"processRequest"),
        "Expected processRequest in {:?}",
        call_names
    );

    // Check call kinds
    let constructor_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "UserService")
        .unwrap();
    assert_eq!(constructor_call.kind, CallKind::DirectCall);

    let method_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "processRequest")
        .unwrap();
    assert_eq!(method_call.kind, CallKind::MethodCall);
}

#[test]
fn test_extract_php_imports() {
    let registry = ParserRegistry::new();
    let source = br#"<?php
namespace App\Controllers;

use Illuminate\Database\Eloquent\Model;
use App\Services\UserService;

class HomeController {
}
"#;
    let analysis = registry
        .parse_file(Path::new("HomeController.php"), source)
        .unwrap();

    assert!(
        analysis.imports.len() >= 2,
        "Expected at least 2 imports, got: {} => {:?}",
        analysis.imports.len(),
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:{}", i.module_path, i.imported_name))
            .collect::<Vec<_>>()
    );

    // use Illuminate\Database\Eloquent\Model
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("Model")),
        "Should have import containing 'Model'"
    );

    // use App\Services\UserService
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("UserService")),
        "Should have import containing 'UserService'"
    );
}

#[test]
fn test_registry_supports_php() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("index.php")));
    assert!(registry.supports_file(Path::new("UserService.php")));
}

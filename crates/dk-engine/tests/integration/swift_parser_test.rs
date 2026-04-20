use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_swift_classes_and_protocols() {
    let registry = ParserRegistry::new();
    let source = br#"
import Foundation

// UserService handles user operations.
public class UserService {
    public func processRequest(req: String) { }
    private func validate(req: String) { }
}

public protocol AuthProvider {
    func authenticate(token: String) -> Bool
}

public enum Status {
    case active
    case inactive
}

public struct Point {
    var x: Int
    var y: Int
}

internal func helperFunction() { }
"#;
    let analysis = registry
        .parse_file(Path::new("UserService.swift"), source)
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

    // Protocol (mapped to Interface)
    assert!(
        names.contains(&"AuthProvider"),
        "Missing AuthProvider protocol, got: {:?}",
        names
    );
    let auth_provider = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AuthProvider")
        .unwrap();
    assert_eq!(auth_provider.kind, SymbolKind::Interface);
    assert_eq!(auth_provider.visibility, Visibility::Public);

    // Enum (class_declaration with enum_class_body)
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
    assert_eq!(
        status.kind,
        SymbolKind::Enum,
        "Status should be Enum, got: {:?}",
        status.kind
    );
    assert_eq!(status.visibility, Visibility::Public);

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

    // Doc comment on class
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
fn test_extract_swift_visibility() {
    let registry = ParserRegistry::new();
    let source = br#"
public class Config {
    public func publicMethod() {}
    private func privateMethod() {}
    internal func internalMethod() {}
    func defaultMethod() {}
}
"#;
    let analysis = registry
        .parse_file(Path::new("Config.swift"), source)
        .unwrap();

    let public_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "publicMethod")
        .unwrap();
    assert_eq!(public_fn.visibility, Visibility::Public);

    let private_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "privateMethod")
        .unwrap();
    assert_eq!(private_fn.visibility, Visibility::Private);

    let internal_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "internalMethod")
        .unwrap();
    assert_eq!(
        internal_fn.visibility,
        Visibility::Private,
        "internal should map to Private"
    );

    let default_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "defaultMethod")
        .unwrap();
    assert_eq!(
        default_fn.visibility,
        Visibility::Private,
        "no modifier (= internal) should map to Private"
    );
}

#[test]
fn test_extract_swift_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
func main() {
    let result = process(data)
    service.handleRequest(req)
    print("hello")
}
"#;
    let analysis = registry
        .parse_file(Path::new("main.swift"), source)
        .unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    // Direct calls
    assert!(
        call_names.contains(&"process"),
        "Expected process in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"print"),
        "Expected print in {:?}",
        call_names
    );

    // Navigation call
    assert!(
        call_names.contains(&"handleRequest"),
        "Expected handleRequest in {:?}",
        call_names
    );

    // Check call kinds
    let process_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "process")
        .unwrap();
    assert_eq!(process_call.kind, CallKind::DirectCall);

    let handle_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "handleRequest")
        .unwrap();
    assert_eq!(handle_call.kind, CallKind::MethodCall);
}

#[test]
fn test_extract_swift_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
import Foundation
import UIKit

class App { }
"#;
    let analysis = registry.parse_file(Path::new("App.swift"), source).unwrap();

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

    // import Foundation
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "Foundation"),
        "Should have import 'Foundation'"
    );

    // import UIKit
    assert!(
        analysis.imports.iter().any(|i| i.module_path == "UIKit"),
        "Should have import 'UIKit'"
    );
}

#[test]
fn test_registry_supports_swift() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("App.swift")));
    assert!(registry.supports_file(Path::new("UserService.swift")));
}

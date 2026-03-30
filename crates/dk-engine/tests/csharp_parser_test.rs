use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_csharp_classes_and_interfaces() {
    let registry = ParserRegistry::new();
    let source = br#"
using System;

namespace MyApp.Models
{
    // UserService handles user operations.
    public class UserService
    {
        public void ProcessRequest(string req) { }
        private void Validate(string req) { }
    }

    internal interface IAuthProvider
    {
        bool Authenticate(string token);
    }

    public enum Status { Active, Inactive }

    public struct Point
    {
        public int X;
        public int Y;
    }
}
"#;
    let analysis = registry
        .parse_file(Path::new("UserService.cs"), source)
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
        names.contains(&"IAuthProvider"),
        "Missing IAuthProvider interface, got: {:?}",
        names
    );
    let auth_provider = analysis
        .symbols
        .iter()
        .find(|s| s.name == "IAuthProvider")
        .unwrap();
    assert_eq!(auth_provider.kind, SymbolKind::Interface);
    assert_eq!(auth_provider.visibility, Visibility::Public);

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
    assert_eq!(status.visibility, Visibility::Public);

    // Struct
    assert!(
        names.contains(&"Point"),
        "Missing Point struct, got: {:?}",
        names
    );
    let point = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Point")
        .unwrap();
    assert_eq!(point.kind, SymbolKind::Struct);
    assert_eq!(point.visibility, Visibility::Public);

    // Methods
    assert!(
        names.contains(&"ProcessRequest"),
        "Missing ProcessRequest method, got: {:?}",
        names
    );
    let process_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "ProcessRequest")
        .unwrap();
    assert_eq!(process_fn.kind, SymbolKind::Function);
    assert_eq!(process_fn.visibility, Visibility::Public);

    // Namespace
    assert!(
        names.iter().any(|n| n.contains("MyApp")),
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
fn test_extract_csharp_visibility() {
    let registry = ParserRegistry::new();
    let source = br#"
public class Config
{
    public void PublicMethod() {}
    protected void ProtectedMethod() {}
    private void PrivateMethod() {}
    internal void InternalMethod() {}
    void DefaultMethod() {}
}
"#;
    let analysis = registry
        .parse_file(Path::new("Config.cs"), source)
        .unwrap();

    let public_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "PublicMethod")
        .unwrap();
    assert_eq!(public_fn.visibility, Visibility::Public);

    let protected_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "ProtectedMethod")
        .unwrap();
    assert_eq!(
        protected_fn.visibility,
        Visibility::Public,
        "protected should map to Public"
    );

    let private_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "PrivateMethod")
        .unwrap();
    assert_eq!(private_fn.visibility, Visibility::Private);

    let internal_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "InternalMethod")
        .unwrap();
    assert_eq!(
        internal_fn.visibility,
        Visibility::Public,
        "internal should map to Public"
    );

    let default_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "DefaultMethod")
        .unwrap();
    assert_eq!(
        default_fn.visibility,
        Visibility::Private,
        "no modifier should map to Private"
    );
}

#[test]
fn test_extract_csharp_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
class Main
{
    void Run()
    {
        var service = new UserService("admin");
        service.ProcessRequest(req);
        Validate(req);
        Console.WriteLine("done");
    }
}
"#;
    let analysis = registry
        .parse_file(Path::new("Main.cs"), source)
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

    // Direct call: Validate()
    assert!(
        call_names.contains(&"Validate"),
        "Expected Validate in {:?}",
        call_names
    );

    // Member access: service.ProcessRequest()
    assert!(
        call_names.contains(&"ProcessRequest"),
        "Expected ProcessRequest in {:?}",
        call_names
    );

    // Member access: Console.WriteLine()
    assert!(
        call_names.contains(&"WriteLine"),
        "Expected WriteLine in {:?}",
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
        .find(|c| c.callee_name == "ProcessRequest")
        .unwrap();
    assert_eq!(method_call.kind, CallKind::MethodCall);
}

#[test]
fn test_extract_csharp_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
using System;
using System.Collections.Generic;
using MyApp.Models;

namespace MyApp
{
    class App { }
}
"#;
    let analysis = registry
        .parse_file(Path::new("App.cs"), source)
        .unwrap();

    assert!(
        analysis.imports.len() >= 3,
        "Expected at least 3 imports, got: {} => {:?}",
        analysis.imports.len(),
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:{}", i.module_path, i.imported_name))
            .collect::<Vec<_>>()
    );

    // using System
    assert!(
        analysis.imports.iter().any(|i| i.module_path == "System"),
        "Should have import 'System'"
    );

    // using System.Collections.Generic
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("System.Collections.Generic")),
        "Should have import 'System.Collections.Generic'"
    );
}

#[test]
fn test_registry_supports_csharp() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("UserService.cs")));
    assert!(registry.supports_file(Path::new("Program.cs")));
}

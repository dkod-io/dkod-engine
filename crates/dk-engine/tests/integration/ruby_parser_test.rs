use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_ruby_classes_and_modules() {
    let registry = ParserRegistry::new();
    let source = br#"
require 'json'

# UserService handles user operations.
module MyApp
  class UserService
    def process_request(req)
      validate(req)
    end

    def self.create(params)
      new(params)
    end

    private

    def validate(req)
      # internal validation
    end
  end
end
"#;
    let analysis = registry
        .parse_file(Path::new("user_service.rb"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();

    // Module
    assert!(
        names.contains(&"MyApp"),
        "Missing MyApp module, got: {:?}",
        names
    );
    let my_app = analysis.symbols.iter().find(|s| s.name == "MyApp").unwrap();
    assert_eq!(my_app.kind, SymbolKind::Module);
    assert_eq!(my_app.visibility, Visibility::Public);

    // Class
    assert!(
        names.contains(&"UserService"),
        "Missing UserService class, got: {:?}",
        names
    );
    let user_svc = analysis
        .symbols
        .iter()
        .find(|s| s.name == "UserService")
        .unwrap();
    assert_eq!(user_svc.kind, SymbolKind::Class);
    assert_eq!(user_svc.visibility, Visibility::Public);

    // Instance method
    assert!(
        names.contains(&"process_request"),
        "Missing process_request method, got: {:?}",
        names
    );
    let process_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "process_request")
        .unwrap();
    assert_eq!(process_fn.kind, SymbolKind::Function);

    // Singleton method
    assert!(
        names.contains(&"create"),
        "Missing create singleton method, got: {:?}",
        names
    );
    let create_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "create")
        .unwrap();
    assert_eq!(create_fn.kind, SymbolKind::Function);

    // Doc comment on module
    assert!(
        my_app.doc_comment.is_some(),
        "MyApp module should have a doc comment"
    );
    assert!(
        my_app
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("handles user operations"),
        "Doc comment should contain 'handles user operations', got: {:?}",
        my_app.doc_comment
    );
}

#[test]
fn test_extract_ruby_visibility() {
    let registry = ParserRegistry::new();
    let source = br#"
class Config
  def public_method
  end

  def another_method
  end
end
"#;
    let analysis = registry.parse_file(Path::new("config.rb"), source).unwrap();

    // Ruby: all methods are public by default (AST has no modifier nodes)
    let public_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "public_method")
        .unwrap();
    assert_eq!(
        public_fn.visibility,
        Visibility::Public,
        "Ruby methods should default to Public"
    );

    let another_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "another_method")
        .unwrap();
    assert_eq!(
        another_fn.visibility,
        Visibility::Public,
        "Ruby methods should default to Public"
    );
}

#[test]
fn test_extract_ruby_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
def main
  result = process(data)
  service.handle_request(req)
  puts "hello"
end
"#;
    let analysis = registry.parse_file(Path::new("main.rb"), source).unwrap();

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
        call_names.contains(&"puts"),
        "Expected puts in {:?}",
        call_names
    );

    // Receiver call
    assert!(
        call_names.contains(&"handle_request"),
        "Expected handle_request in {:?}",
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
        .find(|c| c.callee_name == "handle_request")
        .unwrap();
    assert_eq!(handle_call.kind, CallKind::MethodCall);
}

#[test]
fn test_extract_ruby_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
require 'json'
require 'net/http'
require_relative 'helper'
require_relative './utils/parser'

class App
end
"#;
    let analysis = registry.parse_file(Path::new("app.rb"), source).unwrap();

    assert!(
        analysis.imports.len() >= 4,
        "Expected at least 4 imports, got: {} => {:?}",
        analysis.imports.len(),
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:{}", i.module_path, i.imported_name))
            .collect::<Vec<_>>()
    );

    // require 'json'
    assert!(
        analysis.imports.iter().any(|i| i.module_path == "json"),
        "Should have import 'json'"
    );

    // require 'net/http'
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "net/http" && i.imported_name == "http"),
        "Should have import 'net/http' with name 'http'"
    );

    // require_relative 'helper' — internal (no ./ prefix)
    let helper = analysis.imports.iter().find(|i| i.module_path == "helper");
    assert!(helper.is_some(), "Should have import 'helper'");
    assert!(
        !helper.unwrap().is_external,
        "require_relative 'helper' should be internal even without './' prefix"
    );

    // require_relative './utils/parser' — internal (starts with '.')
    let parser_import = analysis
        .imports
        .iter()
        .find(|i| i.module_path == "./utils/parser");
    assert!(
        parser_import.is_some(),
        "Should have import './utils/parser'"
    );
    assert!(
        !parser_import.unwrap().is_external,
        "require_relative paths starting with '.' should be internal"
    );
}

#[test]
fn test_registry_supports_ruby() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("app.rb")));
    assert!(registry.supports_file(Path::new("user_service.rb")));
}

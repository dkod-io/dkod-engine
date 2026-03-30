use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_cpp_functions() {
    let registry = ParserRegistry::new();
    let source = br#"
// Process the incoming request.
void handleRequest(Request& req) {
    validate(req);
}

int computeHash(const std::string& input) {
    return 0;
}
"#;
    let analysis = registry.parse_file(Path::new("handler.cpp"), source).unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();

    assert!(
        names.contains(&"handleRequest"),
        "Missing handleRequest, got: {:?}",
        names
    );
    assert!(
        names.contains(&"computeHash"),
        "Missing computeHash, got: {:?}",
        names
    );

    let handle_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "handleRequest")
        .unwrap();
    assert_eq!(handle_fn.kind, SymbolKind::Function);
    assert_eq!(handle_fn.visibility, Visibility::Public);

    // Doc comment
    assert!(
        handle_fn.doc_comment.is_some(),
        "handleRequest should have a doc comment"
    );
    assert!(
        handle_fn
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("Process the incoming"),
        "Doc comment should contain 'Process the incoming', got: {:?}",
        handle_fn.doc_comment
    );
}

#[test]
fn test_extract_cpp_classes_and_structs() {
    let registry = ParserRegistry::new();
    let source = br#"
class UserService {
public:
    void process();
private:
    int count;
};

struct Config {
    int port;
    std::string host;
};

enum Color {
    RED,
    GREEN,
    BLUE
};

namespace myapp {
    void initialize();
}
"#;
    let analysis = registry.parse_file(Path::new("types.h"), source).unwrap();

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
        .find(|s| s.name == "UserService")
        .unwrap();
    assert_eq!(user_svc.kind, SymbolKind::Class);

    // Struct
    assert!(
        names.contains(&"Config"),
        "Missing Config struct, got: {:?}",
        names
    );
    let config = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Config")
        .unwrap();
    assert_eq!(config.kind, SymbolKind::Struct);

    // Enum
    assert!(
        names.contains(&"Color"),
        "Missing Color enum, got: {:?}",
        names
    );
    let color = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Color")
        .unwrap();
    assert_eq!(color.kind, SymbolKind::Enum);

    // Namespace
    assert!(
        names.contains(&"myapp"),
        "Missing myapp namespace, got: {:?}",
        names
    );
    let ns = analysis
        .symbols
        .iter()
        .find(|s| s.name == "myapp")
        .unwrap();
    assert_eq!(ns.kind, SymbolKind::Module);
}

#[test]
fn test_extract_cpp_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
void main() {
    process(data);
    std::sort(vec.begin(), vec.end());
    obj.save();
}
"#;
    let analysis = registry.parse_file(Path::new("main.cpp"), source).unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    // Direct call
    assert!(
        call_names.contains(&"process"),
        "Expected process in {:?}",
        call_names
    );

    // Member call: obj.save()
    assert!(
        call_names.contains(&"save"),
        "Expected save in {:?}",
        call_names
    );

    // Check call kinds
    let process_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "process")
        .unwrap();
    assert_eq!(process_call.kind, CallKind::DirectCall);

    let save_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "save")
        .unwrap();
    assert_eq!(save_call.kind, CallKind::MethodCall);
}

#[test]
fn test_extract_cpp_includes() {
    let registry = ParserRegistry::new();
    let source = br#"
#include <iostream>
#include <vector>
#include "myheader.h"
#include "utils/helpers.h"

int main() { return 0; }
"#;
    let analysis = registry.parse_file(Path::new("main.cpp"), source).unwrap();

    assert!(
        analysis.imports.len() >= 4,
        "Expected at least 4 includes, got: {} => {:?}",
        analysis.imports.len(),
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:{}", i.module_path, i.imported_name))
            .collect::<Vec<_>>()
    );

    // System includes are external
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("iostream") && i.is_external),
        "Should have external include <iostream>"
    );

    // Local includes are internal
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("myheader.h") && !i.is_external),
        "Should have internal include \"myheader.h\", got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:ext={}", i.module_path, i.is_external))
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_registry_supports_cpp_extensions() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("main.cpp")));
    assert!(registry.supports_file(Path::new("main.cc")));
    assert!(registry.supports_file(Path::new("main.cxx")));
    assert!(registry.supports_file(Path::new("main.c")));
    assert!(registry.supports_file(Path::new("header.h")));
    assert!(registry.supports_file(Path::new("header.hpp")));
    assert!(registry.supports_file(Path::new("header.hxx")));
}

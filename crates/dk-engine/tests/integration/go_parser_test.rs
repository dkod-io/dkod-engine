use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_go_functions_and_methods() {
    let registry = ParserRegistry::new();
    let source = br#"
package main

// HandleRequest processes an incoming HTTP request.
func HandleRequest(w http.ResponseWriter, r *http.Request) {
    process(r)
}

func helperFunc() {
    // unexported helper
}

type Server struct {
    Port int
}

// Start starts the server on the configured port.
func (s *Server) Start() error {
    return nil
}

func (s *Server) shutdown() {
    // unexported method
}
"#;
    let analysis = registry.parse_file(Path::new("main.go"), source).unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"HandleRequest"),
        "Missing HandleRequest, got: {:?}",
        names
    );
    assert!(
        names.contains(&"helperFunc"),
        "Missing helperFunc, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Server"),
        "Missing Server struct, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Start"),
        "Missing Start method, got: {:?}",
        names
    );
    assert!(
        names.contains(&"shutdown"),
        "Missing shutdown method, got: {:?}",
        names
    );

    // Visibility: uppercase = Public, lowercase = Private
    let handle_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "HandleRequest")
        .unwrap();
    assert_eq!(handle_fn.kind, SymbolKind::Function);
    assert_eq!(handle_fn.visibility, Visibility::Public);

    let helper_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "helperFunc")
        .unwrap();
    assert_eq!(helper_fn.kind, SymbolKind::Function);
    assert_eq!(helper_fn.visibility, Visibility::Private);

    let start_method = analysis.symbols.iter().find(|s| s.name == "Start").unwrap();
    assert_eq!(start_method.kind, SymbolKind::Function);
    assert_eq!(start_method.visibility, Visibility::Public);

    let shutdown_method = analysis
        .symbols
        .iter()
        .find(|s| s.name == "shutdown")
        .unwrap();
    assert_eq!(shutdown_method.kind, SymbolKind::Function);
    assert_eq!(shutdown_method.visibility, Visibility::Private);

    // Doc comments
    assert!(
        handle_fn.doc_comment.is_some(),
        "HandleRequest should have a doc comment"
    );
    assert!(
        handle_fn
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("processes an incoming"),
        "Doc comment should contain 'processes an incoming', got: {:?}",
        handle_fn.doc_comment
    );
}

#[test]
fn test_extract_go_types() {
    let registry = ParserRegistry::new();
    let source = br#"
package main

type UserService struct {
    db *sql.DB
}

type Logger interface {
    Log(msg string)
}

type ID int64

const MaxRetries = 3
const defaultTimeout = 30

var GlobalConfig Config
var internalState = "ready"
"#;
    let analysis = registry.parse_file(Path::new("types.go"), source).unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();

    // Struct
    assert!(
        names.contains(&"UserService"),
        "Missing UserService struct, got: {:?}",
        names
    );
    let user_svc = analysis
        .symbols
        .iter()
        .find(|s| s.name == "UserService")
        .unwrap();
    assert_eq!(user_svc.kind, SymbolKind::Struct);
    assert_eq!(user_svc.visibility, Visibility::Public);

    // Interface
    assert!(
        names.contains(&"Logger"),
        "Missing Logger interface, got: {:?}",
        names
    );
    let logger = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Logger")
        .unwrap();
    assert_eq!(logger.kind, SymbolKind::Interface);
    assert_eq!(logger.visibility, Visibility::Public);

    // Constants
    assert!(
        names.contains(&"MaxRetries"),
        "Missing MaxRetries const, got: {:?}",
        names
    );
    let max_retries = analysis
        .symbols
        .iter()
        .find(|s| s.name == "MaxRetries")
        .unwrap();
    assert_eq!(max_retries.kind, SymbolKind::Const);
    assert_eq!(max_retries.visibility, Visibility::Public);

    let default_timeout = analysis
        .symbols
        .iter()
        .find(|s| s.name == "defaultTimeout")
        .unwrap();
    assert_eq!(default_timeout.kind, SymbolKind::Const);
    assert_eq!(default_timeout.visibility, Visibility::Private);

    // Variables
    assert!(
        names.contains(&"GlobalConfig"),
        "Missing GlobalConfig var, got: {:?}",
        names
    );
    let global_cfg = analysis
        .symbols
        .iter()
        .find(|s| s.name == "GlobalConfig")
        .unwrap();
    assert_eq!(global_cfg.kind, SymbolKind::Variable);
    assert_eq!(global_cfg.visibility, Visibility::Public);
}

#[test]
fn test_extract_go_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
package main

func main() {
    fmt.Println("hello")
    result := process(data)
    result.Save()
}
"#;
    let analysis = registry.parse_file(Path::new("main.go"), source).unwrap();

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

    // Selector calls (method-like): fmt.Println, result.Save
    assert!(
        call_names.contains(&"Println"),
        "Expected Println in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"Save"),
        "Expected Save in {:?}",
        call_names
    );

    // Check call kinds
    let process_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "process")
        .unwrap();
    assert_eq!(process_call.kind, CallKind::DirectCall);

    let println_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "Println")
        .unwrap();
    assert_eq!(println_call.kind, CallKind::MethodCall);
}

#[test]
fn test_extract_go_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
package main

import "fmt"

import (
    "net/http"
    "os"
    log "github.com/sirupsen/logrus"
)
"#;
    let analysis = registry.parse_file(Path::new("main.go"), source).unwrap();

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

    // import "fmt"
    assert!(
        analysis.imports.iter().any(|i| i.module_path == "fmt"),
        "Should have import 'fmt'"
    );

    // import "net/http"
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "net/http" && i.imported_name == "http"),
        "Should have import 'net/http' with name 'http', got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:{}", i.module_path, i.imported_name))
            .collect::<Vec<_>>()
    );

    // aliased import: log "github.com/sirupsen/logrus"
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "github.com/sirupsen/logrus"
                && i.alias.as_deref() == Some("log")),
        "Should have aliased import 'logrus' as 'log'"
    );
}

#[test]
fn test_registry_supports_go() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("main.go")));
    assert!(registry.supports_file(Path::new("server.go")));
}

use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_bash_functions() {
    let registry = ParserRegistry::new();
    let source = br#"#!/bin/bash

# Deploy the application to production.
deploy() {
    echo "Deploying..."
    build_artifacts
    push_to_server
}

function cleanup() {
    rm -rf /tmp/build
    echo "Cleaned up"
}

function setup_env() {
    export PATH="/usr/local/bin:$PATH"
}
"#;
    let analysis = registry
        .parse_file(Path::new("deploy.sh"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"deploy"),
        "Missing deploy function, got: {:?}",
        names
    );
    assert!(
        names.contains(&"cleanup"),
        "Missing cleanup function, got: {:?}",
        names
    );
    assert!(
        names.contains(&"setup_env"),
        "Missing setup_env function, got: {:?}",
        names
    );

    let deploy = analysis
        .symbols
        .iter()
        .find(|s| s.name == "deploy")
        .unwrap();
    assert_eq!(deploy.kind, SymbolKind::Function);
    assert_eq!(deploy.visibility, Visibility::Public);

    // Doc comment
    assert!(
        deploy.doc_comment.is_some(),
        "deploy should have a doc comment"
    );
    assert!(
        deploy
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("Deploy"),
        "Doc comment should contain 'Deploy'"
    );
}

#[test]
fn test_extract_bash_calls() {
    let registry = ParserRegistry::new();
    let source = br#"#!/bin/bash

main() {
    echo "starting"
    git status
    docker build -t myapp .
    deploy
}
"#;
    let analysis = registry
        .parse_file(Path::new("run.sh"), source)
        .unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    assert!(
        call_names.contains(&"echo"),
        "Expected echo in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"git"),
        "Expected git in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"docker"),
        "Expected docker in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"deploy"),
        "Expected deploy in {:?}",
        call_names
    );

    let echo_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "echo")
        .unwrap();
    assert_eq!(echo_call.kind, CallKind::DirectCall);
}

#[test]
fn test_bash_function_with_keyword() {
    let registry = ParserRegistry::new();
    let source = br#"
function with_keyword() {
    echo "uses function keyword"
}

without_keyword() {
    echo "no function keyword"
}
"#;
    let analysis = registry
        .parse_file(Path::new("funcs.sh"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"with_keyword"),
        "Missing with_keyword, got: {:?}",
        names
    );
    assert!(
        names.contains(&"without_keyword"),
        "Missing without_keyword, got: {:?}",
        names
    );
}

#[test]
fn test_bash_no_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
source /etc/profile
. ~/.bashrc

main() {
    echo "hello"
}
"#;
    let analysis = registry
        .parse_file(Path::new("init.sh"), source)
        .unwrap();

    // We don't extract source/. imports since they're regular commands
    assert!(
        analysis.imports.is_empty(),
        "Bash imports should be empty, got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| &i.module_path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_registry_supports_bash() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("deploy.sh")));
    assert!(registry.supports_file(Path::new("init.bash")));
    assert!(!registry.supports_file(Path::new("config.zsh")));
}

use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_kotlin_classes_and_interfaces() {
    let registry = ParserRegistry::new();
    let source = br#"
package com.example

// UserService manages user operations.
class UserService(private val db: Database) {
    fun findUser(id: Int): User? {
        return db.query(id)
    }
}

interface Serializable {
    fun serialize(): String
}

object AppConfig {
    val defaultTimeout = 30
}
"#;
    let analysis = registry
        .parse_file(Path::new("UserService.kt"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"UserService"),
        "Missing UserService class, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Serializable"),
        "Missing Serializable interface, got: {:?}",
        names
    );
    assert!(
        names.contains(&"AppConfig"),
        "Missing AppConfig object, got: {:?}",
        names
    );
    assert!(
        names.contains(&"findUser"),
        "Missing findUser function, got: {:?}",
        names
    );

    let user_svc = analysis
        .symbols
        .iter()
        .find(|s| s.name == "UserService")
        .unwrap();
    assert_eq!(user_svc.kind, SymbolKind::Class);

    let serializable = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Serializable")
        .unwrap();
    assert_eq!(serializable.kind, SymbolKind::Interface);

    let app_config = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AppConfig")
        .unwrap();
    assert_eq!(app_config.kind, SymbolKind::Module);
}

#[test]
fn test_extract_kotlin_visibility() {
    let registry = ParserRegistry::new();
    let source = br#"
class PublicClass {
    fun publicMethod(): Unit {}
    private fun privateMethod(): Unit {}
    internal fun internalMethod(): Unit {}
}

private class InternalClass
"#;
    let analysis = registry
        .parse_file(Path::new("Visibility.kt"), source)
        .unwrap();

    let public_class = analysis
        .symbols
        .iter()
        .find(|s| s.name == "PublicClass")
        .unwrap();
    assert_eq!(public_class.visibility, Visibility::Public);

    let public_method = analysis
        .symbols
        .iter()
        .find(|s| s.name == "publicMethod")
        .unwrap();
    assert_eq!(public_method.visibility, Visibility::Public);

    let private_method = analysis
        .symbols
        .iter()
        .find(|s| s.name == "privateMethod")
        .unwrap();
    assert_eq!(private_method.visibility, Visibility::Private);

    let internal_method = analysis
        .symbols
        .iter()
        .find(|s| s.name == "internalMethod")
        .unwrap();
    assert_eq!(internal_method.visibility, Visibility::Private);

    let internal_class = analysis
        .symbols
        .iter()
        .find(|s| s.name == "InternalClass")
        .unwrap();
    assert_eq!(internal_class.visibility, Visibility::Private);
}

#[test]
fn test_extract_kotlin_properties() {
    let registry = ParserRegistry::new();
    let source = br#"
val maxRetries = 3
var currentState = "idle"

class Config {
    val timeout: Int = 30
}
"#;
    let analysis = registry
        .parse_file(Path::new("Config.kt"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"maxRetries"),
        "Missing maxRetries, got: {:?}",
        names
    );
    assert!(
        names.contains(&"currentState"),
        "Missing currentState, got: {:?}",
        names
    );

    let max_retries = analysis
        .symbols
        .iter()
        .find(|s| s.name == "maxRetries")
        .unwrap();
    assert_eq!(max_retries.kind, SymbolKind::Variable);
}

#[test]
fn test_extract_kotlin_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
fun main() {
    println("hello")
    val result = process(data)
    result.save()
}
"#;
    let analysis = registry
        .parse_file(Path::new("Main.kt"), source)
        .unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    assert!(
        call_names.contains(&"println"),
        "Expected println in {:?}",
        call_names
    );
    assert!(
        call_names.contains(&"process"),
        "Expected process in {:?}",
        call_names
    );

    let println_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "println")
        .unwrap();
    assert_eq!(println_call.kind, CallKind::DirectCall);
}

#[test]
fn test_registry_supports_kotlin() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("Main.kt")));
    assert!(registry.supports_file(Path::new("build.gradle.kts")));
}

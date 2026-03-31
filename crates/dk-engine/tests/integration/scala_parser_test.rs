use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_scala_classes_and_traits() {
    let registry = ParserRegistry::new();
    let source = br#"
// UserService handles user operations.
class UserService(db: Database) {
  def findUser(id: Int): Option[User] = {
    db.query(id)
  }
}

trait Serializable {
  def serialize(): String
}

object AppConfig {
  val defaultTimeout = 30
}
"#;
    let analysis = registry
        .parse_file(Path::new("UserService.scala"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"UserService"),
        "Missing UserService class, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Serializable"),
        "Missing Serializable trait, got: {:?}",
        names
    );
    assert!(
        names.contains(&"AppConfig"),
        "Missing AppConfig object, got: {:?}",
        names
    );
    assert!(
        names.contains(&"findUser"),
        "Missing findUser method, got: {:?}",
        names
    );

    let user_svc = analysis
        .symbols
        .iter()
        .find(|s| s.name == "UserService")
        .unwrap();
    assert_eq!(user_svc.kind, SymbolKind::Class);
    assert_eq!(user_svc.visibility, Visibility::Public);

    let serializable = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Serializable")
        .unwrap();
    assert_eq!(serializable.kind, SymbolKind::Trait);

    let app_config = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AppConfig")
        .unwrap();
    assert_eq!(app_config.kind, SymbolKind::Module);

    // Doc comment on UserService
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
        "Doc comment should contain 'handles user operations'"
    );
}

#[test]
fn test_extract_scala_vals_and_vars() {
    let registry = ParserRegistry::new();
    let source = br#"
object Config {
  val maxRetries = 3
  var currentState = "idle"
  private val secret = "hidden"
}
"#;
    let analysis = registry
        .parse_file(Path::new("Config.scala"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"maxRetries"),
        "Missing maxRetries val, got: {:?}",
        names
    );
    assert!(
        names.contains(&"currentState"),
        "Missing currentState var, got: {:?}",
        names
    );

    let max_retries = analysis
        .symbols
        .iter()
        .find(|s| s.name == "maxRetries")
        .unwrap();
    assert_eq!(max_retries.kind, SymbolKind::Const);
    assert_eq!(max_retries.visibility, Visibility::Public);

    let current_state = analysis
        .symbols
        .iter()
        .find(|s| s.name == "currentState")
        .unwrap();
    assert_eq!(current_state.kind, SymbolKind::Variable);
}

#[test]
fn test_extract_scala_visibility() {
    let registry = ParserRegistry::new();
    let source = br#"
class PublicClass {
  def publicMethod(): Unit = {}
  private def privateMethod(): Unit = {}
  protected def protectedMethod(): Unit = {}
}

private class InternalClass {
  def helper(): Unit = {}
}
"#;
    let analysis = registry
        .parse_file(Path::new("Visibility.scala"), source)
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

    let internal_class = analysis
        .symbols
        .iter()
        .find(|s| s.name == "InternalClass")
        .unwrap();
    assert_eq!(internal_class.visibility, Visibility::Private);
}

#[test]
fn test_extract_scala_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
object Main {
  def main(args: Array[String]): Unit = {
    println("hello")
    val result = process(data)
    result.save()
  }
}
"#;
    let analysis = registry
        .parse_file(Path::new("Main.scala"), source)
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
fn test_registry_supports_scala() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("Main.scala")));
    assert!(registry.supports_file(Path::new("script.sc")));
}

#[test]
fn test_extract_scala_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
import scala.collection.mutable.Map
import java.util.ArrayList
import io.netty.channel.ChannelHandler
"#;
    let analysis = registry
        .parse_file(Path::new("Imports.scala"), source)
        .unwrap();

    // tree-sitter-scala exposes each path segment as a separate `path:
    // (identifier)` field, so `import scala.collection.mutable.Map`
    // produces 4 captures: "scala", "collection", "mutable", "Map".
    // With 3 multi-segment imports, expect at least 3 import entries
    // (one per segment).
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

    // Verify that at least one segment from each import statement is captured.
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "scala" || i.module_path == "Map"),
        "Should have a segment from 'import scala.collection.mutable.Map', got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| &i.module_path)
            .collect::<Vec<_>>()
    );

    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "java" || i.module_path == "ArrayList"),
        "Should have a segment from 'import java.util.ArrayList', got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| &i.module_path)
            .collect::<Vec<_>>()
    );

    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path == "io" || i.module_path == "ChannelHandler"),
        "Should have a segment from 'import io.netty.channel.ChannelHandler', got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| &i.module_path)
            .collect::<Vec<_>>()
    );
}

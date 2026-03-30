use dk_core::{SymbolKind, Visibility};
use dk_engine::parser::engine::QueryDrivenParser;
use dk_engine::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_engine::parser::LanguageParser;
use std::path::Path;
use tree_sitter::Language;

/// Minimal test config using Rust grammar to verify engine logic.
struct TestRustConfig;

impl LanguageConfig for TestRustConfig {
    fn language(&self) -> Language {
        tree_sitter_rust::LANGUAGE.into()
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }
    fn symbols_query(&self) -> &'static str {
        r#"(function_item name: (identifier) @name) @definition.function
(struct_item name: (type_identifier) @name) @definition.struct"#
    }
    fn calls_query(&self) -> &'static str {
        r#"(call_expression function: (identifier) @callee) @call"#
    }
    fn imports_query(&self) -> &'static str {
        ""
    }
    fn comment_style(&self) -> CommentStyle {
        CommentStyle::TripleSlash
    }
    fn resolve_visibility(&self, _modifiers: Option<&str>, _name: &str) -> Visibility {
        Visibility::Public
    }
}

#[test]
fn test_engine_extracts_symbols() {
    let parser = QueryDrivenParser::new(Box::new(TestRustConfig)).unwrap();
    let source = b"pub fn hello() {}\nstruct World {}";
    let analysis = parser.parse_file(source, Path::new("test.rs")).unwrap();
    assert_eq!(analysis.symbols.len(), 2);
    let hello = analysis.symbols.iter().find(|s| s.name == "hello").unwrap();
    assert_eq!(hello.kind, SymbolKind::Function);
    let world = analysis.symbols.iter().find(|s| s.name == "World").unwrap();
    assert_eq!(world.kind, SymbolKind::Struct);
}

#[test]
fn test_engine_extracts_calls() {
    let parser = QueryDrivenParser::new(Box::new(TestRustConfig)).unwrap();
    let source = b"fn main() { hello(); }";
    let analysis = parser.parse_file(source, Path::new("test.rs")).unwrap();
    assert_eq!(analysis.calls.len(), 1);
    assert_eq!(analysis.calls[0].callee_name, "hello");
}

#[test]
fn test_engine_empty_source() {
    let parser = QueryDrivenParser::new(Box::new(TestRustConfig)).unwrap();
    let analysis = parser.parse_file(b"", Path::new("test.rs")).unwrap();
    assert!(analysis.symbols.is_empty());
    assert!(analysis.calls.is_empty());
    assert!(analysis.imports.is_empty());
}

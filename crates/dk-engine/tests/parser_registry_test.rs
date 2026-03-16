use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_registry_detects_language() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("main.rs")));
    assert!(registry.supports_file(Path::new("app.ts")));
    assert!(registry.supports_file(Path::new("app.tsx")));
    assert!(registry.supports_file(Path::new("index.js")));
    assert!(registry.supports_file(Path::new("component.jsx")));
    assert!(registry.supports_file(Path::new("script.py")));
    assert!(!registry.supports_file(Path::new("image.png")));
    assert!(!registry.supports_file(Path::new("data.csv")));
    assert!(!registry.supports_file(Path::new("noext")));
}

#[test]
fn test_parse_empty_file() {
    let registry = ParserRegistry::new();
    let analysis = registry.parse_file(Path::new("empty.rs"), b"").unwrap();
    assert!(analysis.symbols.is_empty());
    assert!(analysis.calls.is_empty());
}

#[test]
fn test_unsupported_extension() {
    let registry = ParserRegistry::new();
    let result = registry.parse_file(Path::new("data.csv"), b"a,b,c");
    assert!(result.is_err());
}

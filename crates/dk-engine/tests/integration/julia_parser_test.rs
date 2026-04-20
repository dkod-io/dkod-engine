use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_julia_functions() {
    let registry = ParserRegistry::new();
    let source = br#"
# Calculate the factorial of n.
function factorial(n)
    if n <= 1
        return 1
    end
    return n * factorial(n - 1)
end

# Short form function definition
double(x) = 2 * x

function _private_helper(data)
    process(data)
end
"#;
    let analysis = registry.parse_file(Path::new("math.jl"), source).unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"factorial"),
        "Missing factorial, got: {:?}",
        names
    );
    assert!(
        names.contains(&"double"),
        "Missing double (short form), got: {:?}",
        names
    );
    assert!(
        names.contains(&"_private_helper"),
        "Missing _private_helper, got: {:?}",
        names
    );

    let factorial = analysis
        .symbols
        .iter()
        .find(|s| s.name == "factorial")
        .unwrap();
    assert_eq!(factorial.kind, SymbolKind::Function);
    assert_eq!(factorial.visibility, Visibility::Public);

    let private_helper = analysis
        .symbols
        .iter()
        .find(|s| s.name == "_private_helper")
        .unwrap();
    assert_eq!(private_helper.visibility, Visibility::Private);

    // Doc comment
    assert!(
        factorial.doc_comment.is_some(),
        "factorial should have a doc comment"
    );
    assert!(
        factorial
            .doc_comment
            .as_ref()
            .unwrap()
            .contains("factorial"),
        "Doc comment should mention factorial"
    );
}

#[test]
fn test_extract_julia_structs() {
    let registry = ParserRegistry::new();
    let source = br#"
struct Point
    x::Float64
    y::Float64
end

mutable struct Config
    timeout::Int
    retries::Int
end

abstract type Shape end
"#;
    let analysis = registry.parse_file(Path::new("types.jl"), source).unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Point"),
        "Missing Point struct, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Config"),
        "Missing Config struct, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Shape"),
        "Missing Shape abstract type, got: {:?}",
        names
    );

    let point = analysis.symbols.iter().find(|s| s.name == "Point").unwrap();
    assert_eq!(point.kind, SymbolKind::Struct);

    let shape = analysis.symbols.iter().find(|s| s.name == "Shape").unwrap();
    assert_eq!(shape.kind, SymbolKind::TypeAlias);
}

#[test]
fn test_extract_julia_modules() {
    let registry = ParserRegistry::new();
    let source = br#"
module MyModule

function greet(name)
    println("Hello, $name!")
end

end
"#;
    let analysis = registry
        .parse_file(Path::new("mymodule.jl"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"MyModule"),
        "Missing MyModule, got: {:?}",
        names
    );
    assert!(
        names.contains(&"greet"),
        "Missing greet function, got: {:?}",
        names
    );

    let my_module = analysis
        .symbols
        .iter()
        .find(|s| s.name == "MyModule")
        .unwrap();
    assert_eq!(my_module.kind, SymbolKind::Module);
}

#[test]
fn test_extract_julia_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
function main()
    println("hello")
    result = process(data)
    Base.sort!(items)
end
"#;
    let analysis = registry.parse_file(Path::new("main.jl"), source).unwrap();

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
fn test_extract_julia_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
import Foo
using Bar
import LinearAlgebra
"#;
    let analysis = registry
        .parse_file(Path::new("imports.jl"), source)
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

    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("Foo")),
        "Should have import containing 'Foo', got: {:?}",
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
            .any(|i| i.module_path.contains("Bar")),
        "Should have using containing 'Bar', got: {:?}",
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
            .any(|i| i.module_path.contains("LinearAlgebra")),
        "Should have import containing 'LinearAlgebra', got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| &i.module_path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_registry_supports_julia() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("script.jl")));
    assert!(!registry.supports_file(Path::new("script.jul")));
}

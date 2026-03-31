use dk_core::{SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_haskell_functions() {
    let registry = ParserRegistry::new();
    let source = br#"
module Main where

-- | Calculate the factorial of a number.
factorial :: Int -> Int
factorial 0 = 1
factorial n = n * factorial (n - 1)

-- | Helper function for processing lists.
processItems :: [a] -> [b]
processItems xs = map transform xs

helper x = x + 1
"#;
    let analysis = registry
        .parse_file(Path::new("Main.hs"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"factorial"),
        "Missing factorial, got: {:?}",
        names
    );
    assert!(
        names.contains(&"processItems"),
        "Missing processItems, got: {:?}",
        names
    );
    assert!(
        names.contains(&"helper"),
        "Missing helper, got: {:?}",
        names
    );

    let factorial = analysis
        .symbols
        .iter()
        .find(|s| s.name == "factorial")
        .unwrap();
    assert_eq!(factorial.kind, SymbolKind::Function);
    assert_eq!(factorial.visibility, Visibility::Public);
}

#[test]
fn test_extract_haskell_data_types() {
    let registry = ParserRegistry::new();
    let source = br#"
module Types where

data Color = Red | Green | Blue

data Tree a = Leaf a | Branch (Tree a) (Tree a)

newtype Wrapper a = Wrap a
"#;
    let analysis = registry
        .parse_file(Path::new("Types.hs"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Color"),
        "Missing Color data type, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Tree"),
        "Missing Tree data type, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Wrapper"),
        "Missing Wrapper newtype, got: {:?}",
        names
    );

    let color = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Color")
        .unwrap();
    assert_eq!(color.kind, SymbolKind::Struct);

    let wrapper = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Wrapper")
        .unwrap();
    assert_eq!(wrapper.kind, SymbolKind::Struct);
}

#[test]
fn test_extract_haskell_classes_and_type_synonyms() {
    let registry = ParserRegistry::new();
    let source = br#"
module Abstractions where

class Printable a where
  display :: a -> String

type Name = String

type Mapping k v = [(k, v)]
"#;
    let analysis = registry
        .parse_file(Path::new("Abstractions.hs"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Printable"),
        "Missing Printable class, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Name"),
        "Missing Name type synonym, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Mapping"),
        "Missing Mapping type synonym, got: {:?}",
        names
    );

    let printable = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Printable")
        .unwrap();
    assert_eq!(printable.kind, SymbolKind::Trait);

    let name_type = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Name")
        .unwrap();
    assert_eq!(name_type.kind, SymbolKind::TypeAlias);
}

#[test]
fn test_extract_haskell_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
module Main where

import Data.List
import qualified Data.Map as Map
import Control.Monad
"#;
    let analysis = registry
        .parse_file(Path::new("Main.hs"), source)
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
            .any(|i| i.module_path.contains("Data.List")),
        "Should have import 'Data.List', got: {:?}",
        analysis
            .imports
            .iter()
            .map(|i| &i.module_path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_registry_supports_haskell() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("Main.hs")));
    assert!(!registry.supports_file(Path::new("Main.lhs")));
}

#[test]
fn test_haskell_comment_style_is_dashdash() {
    // Verify the Haskell config returns CommentStyle::DashDash (not
    // SlashSlash), so that the doc-comment collector uses `--` as the
    // prefix. tree-sitter-haskell wraps definitions in a `declaration`
    // super-type, making prev_sibling() doc-comment collection a
    // known limitation — but the prefix itself must be correct.
    use dk_engine::parser::lang_config::{CommentStyle, LanguageConfig};
    use dk_engine::parser::langs::haskell::HaskellConfig;

    let config = HaskellConfig;
    assert_eq!(
        config.comment_style(),
        CommentStyle::DashDash,
        "Haskell should use DashDash comment style, not SlashSlash"
    );
}

//! Swift language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::{Symbol, SymbolKind, Visibility};
use tree_sitter::Language;

/// Swift language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct SwiftConfig;

impl LanguageConfig for SwiftConfig {
    fn language(&self) -> Language {
        tree_sitter_swift::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["swift"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/swift_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/swift_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/swift_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, modifiers: Option<&str>, _name: &str) -> Visibility {
        match modifiers {
            Some(m) if m.contains("public") => Visibility::Public,
            Some(m) if m.contains("open") => Visibility::Public,
            Some(m) if m.contains("private") => Visibility::Private,
            Some(m) if m.contains("fileprivate") => Visibility::Private,
            // internal (explicit or implicit) → Private
            _ => Visibility::Private,
        }
    }

    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, _source: &[u8]) {
        // tree-sitter-swift 0.6 represents enums, structs, and classes all
        // as `class_declaration`. Distinguish by the body child type:
        //   - `enum_class_body` → Enum
        //   - `class_body` with struct keyword in source → Struct
        //   - `class_body` otherwise → Class (default)
        if node.kind() == "class_declaration" && sym.kind == SymbolKind::Class {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "enum_class_body" {
                    sym.kind = SymbolKind::Enum;
                    break;
                }
            }

            // Check if the declaration starts with "struct" keyword
            if sym.kind == SymbolKind::Class {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "struct" {
                        sym.kind = SymbolKind::Struct;
                        break;
                    }
                }
            }
        }
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // Swift imports are module-based. Without SPM/Xcode context
        // we can't distinguish internal vs external, so treat all as external.
        true
    }
}

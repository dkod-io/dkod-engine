//! Kotlin language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::{Symbol, SymbolKind, Visibility};
use tree_sitter::Language;

/// Kotlin language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct KotlinConfig;

impl LanguageConfig for KotlinConfig {
    fn language(&self) -> Language {
        tree_sitter_kotlin_codanna::language()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["kt", "kts"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/kotlin_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/kotlin_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/kotlin_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, _name: &str) -> Visibility {
        // Kotlin defaults to public. Actual visibility modifiers are
        // handled in `adjust_symbol` by walking the AST.
        Visibility::Public
    }

    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, source: &[u8]) {
        // Walk children to find `modifiers` and process ALL modifier types
        // (visibility, class_modifier, inheritance_modifier) before returning.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for modifier in child.children(&mut mod_cursor) {
                    if modifier.kind() == "visibility_modifier" {
                        let text = &source[modifier.start_byte()..modifier.end_byte()];
                        let text_str = std::str::from_utf8(text).unwrap_or("");
                        match text_str.trim() {
                            "private" | "internal" => {
                                sym.visibility = Visibility::Private;
                            }
                            "protected" => {
                                sym.visibility = Visibility::Private;
                            }
                            "public" => {
                                sym.visibility = Visibility::Public;
                            }
                            _ => {}
                        }
                        // Do NOT return here — fall through so class_modifier
                        // (enum) and the interface-keyword check still run.
                    }
                    if modifier.kind() == "class_modifier" {
                        let text = &source[modifier.start_byte()..modifier.end_byte()];
                        let text_str = std::str::from_utf8(text).unwrap_or("");
                        if text_str.trim() == "enum" {
                            sym.kind = SymbolKind::Enum;
                        }
                    }
                    if modifier.kind() == "inheritance_modifier" {
                        let text = &source[modifier.start_byte()..modifier.end_byte()];
                        let text_str = std::str::from_utf8(text).unwrap_or("");
                        if text_str.trim() == "abstract" {
                            // Keep as class
                        }
                    }
                }
            }
        }

        // Check if this is an interface (the "interface" keyword appears
        // as a direct child of class_declaration, not inside modifiers).
        if node.kind() == "class_declaration" && sym.kind == SymbolKind::Class {
            let mut cursor2 = node.walk();
            for child in node.children(&mut cursor2) {
                let text = &source[child.start_byte()..child.end_byte()];
                let text_str = std::str::from_utf8(text).unwrap_or("");
                if text_str == "interface" {
                    sym.kind = SymbolKind::Interface;
                    break;
                }
            }
        }
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // Without Gradle/Maven context we can't distinguish internal
        // vs external packages. Treat all as external.
        true
    }
}

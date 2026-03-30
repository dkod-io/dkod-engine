//! Scala language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::{Symbol, Visibility};
use tree_sitter::Language;

/// Scala language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct ScalaConfig;

impl LanguageConfig for ScalaConfig {
    fn language(&self) -> Language {
        tree_sitter_scala::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["scala", "sc"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/scala_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/scala_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/scala_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, _name: &str) -> Visibility {
        // Scala defaults to public. Visibility modifiers (private/protected)
        // appear as `access_modifier` child nodes, but we handle those in
        // `adjust_symbol` by walking the AST.
        Visibility::Public
    }

    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, source: &[u8]) {
        // Check for access_modifier children (private/protected).
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "access_modifier" || child.kind() == "modifiers" {
                let text = &source[child.start_byte()..child.end_byte()];
                let text_str = std::str::from_utf8(text).unwrap_or("");
                if text_str.contains("private") {
                    sym.visibility = Visibility::Private;
                    return;
                }
                // Scala's protected is roughly equivalent to crate-level visibility.
                if text_str.contains("protected") {
                    sym.visibility = Visibility::Private;
                    return;
                }
            }
        }
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // Scala imports are package-based. Without build tool context
        // we can't distinguish internal vs external, so treat all as external.
        true
    }
}

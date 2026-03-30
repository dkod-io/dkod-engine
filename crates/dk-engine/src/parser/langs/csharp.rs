//! C# language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::{Symbol, Visibility};
use tree_sitter::Language;

/// C# language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct CSharpConfig;

impl LanguageConfig for CSharpConfig {
    fn language(&self) -> Language {
        tree_sitter_c_sharp::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["cs"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/csharp_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/csharp_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/csharp_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, modifiers: Option<&str>, _name: &str) -> Visibility {
        match modifiers {
            Some(m) if m.contains("public") => Visibility::Public,
            Some(m) if m.contains("protected") => Visibility::Public,
            Some(m) if m.contains("internal") => Visibility::Public,
            // private or no modifier → Private
            _ => Visibility::Private,
        }
    }

    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, source: &[u8]) {
        // C# uses `repeat($.modifier)` — each keyword is a separate
        // `modifier` node. Walk the declaration's children to collect all
        // modifier texts and resolve visibility from the combined set.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifier" {
                let text = &source[child.start_byte()..child.end_byte()];
                let modifier = std::str::from_utf8(text).unwrap_or("");
                match modifier {
                    "public" | "protected" | "internal" => {
                        sym.visibility = Visibility::Public;
                        return;
                    }
                    "private" => {
                        sym.visibility = Visibility::Private;
                        return;
                    }
                    _ => {} // static, abstract, sealed, etc. — skip
                }
            }
        }
        // No visibility modifier found → default is private in C#
        sym.visibility = Visibility::Private;
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // C# imports are namespace-based. Without project context we can't
        // distinguish internal vs external, so treat all as external.
        true
    }
}

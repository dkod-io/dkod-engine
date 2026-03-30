//! C# language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
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

    fn is_external_import(&self, _module_path: &str) -> bool {
        // C# imports are namespace-based. Without project context we can't
        // distinguish internal vs external, so treat all as external.
        true
    }
}

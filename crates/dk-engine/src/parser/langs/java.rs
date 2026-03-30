//! Java language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// Java language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct JavaConfig;

impl LanguageConfig for JavaConfig {
    fn language(&self) -> Language {
        tree_sitter_java::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["java"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/java_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/java_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/java_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, modifiers: Option<&str>, _name: &str) -> Visibility {
        match modifiers {
            Some(m) if m.contains("public") => Visibility::Public,
            Some(m) if m.contains("protected") => Visibility::Public,
            // private or package-private (no modifier) → Private
            _ => Visibility::Private,
        }
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // Java imports are always fully qualified. Without project context
        // we can't distinguish internal vs external, so treat all as external.
        true
    }
}

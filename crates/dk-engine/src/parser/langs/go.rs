//! Go language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// Go language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct GoConfig;

impl LanguageConfig for GoConfig {
    fn language(&self) -> Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["go"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/go_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/go_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/go_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, name: &str) -> Visibility {
        // Go visibility: uppercase first letter = exported (Public),
        // lowercase = unexported (Private).
        match name.chars().next() {
            Some(c) if c.is_uppercase() => Visibility::Public,
            _ => Visibility::Private,
        }
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // Go standard library packages have no dots in the path.
        // Third-party packages use domain-based paths (e.g. "github.com/...").
        // We treat both as external since Go doesn't have relative imports.
        // The only truly "internal" packages would be in the same module,
        // but we can't determine that without go.mod context.
        true
    }
}

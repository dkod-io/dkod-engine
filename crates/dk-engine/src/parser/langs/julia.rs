//! Julia language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// Julia language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct JuliaConfig;

impl LanguageConfig for JuliaConfig {
    fn language(&self) -> Language {
        tree_sitter_julia::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["jl"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/julia_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/julia_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/julia_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::Hash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, name: &str) -> Visibility {
        // Julia convention: names starting with `_` are private.
        if name.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // Julia imports are module-based. Without Project.toml context
        // we can't distinguish internal vs external. Treat all as external.
        true
    }
}

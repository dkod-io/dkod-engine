//! Ruby language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// Ruby language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct RubyConfig;

impl LanguageConfig for RubyConfig {
    fn language(&self) -> Language {
        tree_sitter_ruby::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rb"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/ruby_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/ruby_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/ruby_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::Hash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, _name: &str) -> Visibility {
        // Ruby methods are public by default. The `private`/`protected`
        // keywords are method calls that change visibility for subsequent
        // definitions, but they don't appear as AST modifiers on the method
        // node itself. We default everything to Public.
        Visibility::Public
    }

    fn is_external_import(&self, module_path: &str) -> bool {
        // require_relative paths start with '.' — they're internal.
        // Everything else (gems, stdlib) is external.
        !module_path.starts_with('.')
    }
}

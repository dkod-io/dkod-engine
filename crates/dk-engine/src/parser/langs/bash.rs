//! Bash language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// Bash language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct BashConfig;

impl LanguageConfig for BashConfig {
    fn language(&self) -> Language {
        tree_sitter_bash::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["sh", "bash"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/bash_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/bash_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        // Bash uses `source` / `.` for includes, which are regular commands.
        // We leave imports empty since they can't be reliably distinguished
        // from other commands via tree-sitter queries.
        include_str!("../queries/bash_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::Hash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, _name: &str) -> Visibility {
        // Bash has no visibility concept — all functions are global.
        Visibility::Public
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        true
    }
}

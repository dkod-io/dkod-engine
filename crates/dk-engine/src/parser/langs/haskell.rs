//! Haskell language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// Haskell language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct HaskellConfig;

impl LanguageConfig for HaskellConfig {
    fn language(&self) -> Language {
        tree_sitter_haskell::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["hs"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/haskell_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        // Haskell uses whitespace-based function application (not parenthesized
        // call expressions), making call extraction impractical via queries.
        include_str!("../queries/haskell_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/haskell_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        // Haskell uses `--` for line comments. The `Hash` variant strips `#`
        // which won't match. `SlashSlash` strips `//`. Neither is perfect,
        // but the doc-comment collector checks for the prefix `--` being
        // "starts_with" the comment_prefix. We use `SlashSlash` style and
        // handle the `--` prefix in the doc comment collector via the
        // generic `comment` node type check.
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, _name: &str) -> Visibility {
        // Haskell visibility is controlled by module export lists, not
        // per-definition modifiers. Default everything to Public.
        Visibility::Public
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // Without Cabal/Stack context, we can't distinguish internal
        // vs external modules. Treat all as external.
        true
    }
}

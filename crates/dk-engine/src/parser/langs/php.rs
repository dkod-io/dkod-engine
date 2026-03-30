//! PHP language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// PHP language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
///
/// Uses `LANGUAGE_PHP` (includes `<?php` tag handling). PHP files must
/// start with `<?php` for parsing to succeed.
pub struct PhpConfig;

impl LanguageConfig for PhpConfig {
    fn language(&self) -> Language {
        tree_sitter_php::LANGUAGE_PHP.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["php"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/php_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/php_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/php_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, modifiers: Option<&str>, _name: &str) -> Visibility {
        match modifiers {
            Some(m) if m.contains("private") => Visibility::Private,
            Some(m) if m.contains("protected") => Visibility::Public,
            // public or no modifier → Public (PHP convention)
            _ => Visibility::Public,
        }
    }

    fn is_external_import(&self, _module_path: &str) -> bool {
        // PHP `use` statements are always fully qualified namespace paths.
        // Without Composer/autoloader context we can't distinguish internal
        // vs external, so treat all as external.
        true
    }
}

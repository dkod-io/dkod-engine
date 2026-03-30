//! C++ language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::Visibility;
use tree_sitter::Language;

/// C++ language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct CppConfig;

impl LanguageConfig for CppConfig {
    fn language(&self) -> Language {
        tree_sitter_cpp::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        // .c and .h are parsed with the C++ grammar (a superset of C).
        // K&R-style definitions and some C99/C11 constructs may not be
        // captured perfectly, but coverage for modern C is acceptable.
        &["cpp", "cc", "cxx", "c", "h", "hpp", "hxx"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/cpp_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/cpp_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/cpp_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, _modifiers: Option<&str>, _name: &str) -> Visibility {
        // Top-level C++ symbols are effectively public (visible to other
        // translation units unless explicitly static/anonymous-namespace).
        // We default everything to Public for simplicity.
        Visibility::Public
    }

    fn is_external_import(&self, module_path: &str) -> bool {
        // System includes (<iostream>) keep angle brackets after the engine's
        // quote-stripping pass. Local includes ("myheader.h") have quotes
        // stripped, leaving a bare path. Angle brackets → external.
        module_path.starts_with('<')
    }
}

//! Rust language configuration for the query-driven parser.

use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use dk_core::{Symbol, Visibility};
use tree_sitter::Language;

/// Rust language configuration for [`QueryDrivenParser`](crate::parser::engine::QueryDrivenParser).
pub struct RustConfig;

impl LanguageConfig for RustConfig {
    fn language(&self) -> Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/rust_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/rust_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/rust_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::TripleSlash
    }

    fn resolve_visibility(&self, modifiers: Option<&str>, _name: &str) -> Visibility {
        match modifiers {
            Some(m) if m.contains("crate") => Visibility::Crate,
            Some(m) if m.contains("super") => Visibility::Super,
            Some(_) => Visibility::Public,
            None => Visibility::Private,
        }
    }

    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, source: &[u8]) {
        // For impl_item nodes, construct the full name from trait + type fields.
        if node.kind() == "impl_item" {
            let type_name = node.child_by_field_name("type").and_then(|ty| {
                let text = &source[ty.start_byte()..ty.end_byte()];
                std::str::from_utf8(text).ok().map(|s| s.to_string())
            });

            let trait_name = node.child_by_field_name("trait").and_then(|tr| {
                let text = &source[tr.start_byte()..tr.end_byte()];
                std::str::from_utf8(text).ok().map(|s| s.to_string())
            });

            let name = match (trait_name, type_name) {
                (Some(tr), Some(ty)) => format!("impl {tr} for {ty}"),
                (None, Some(ty)) => format!("impl {ty}"),
                _ => "impl".to_string(),
            };

            sym.name = name.clone();
            sym.qualified_name = name;
        }
    }

    fn is_external_import(&self, module_path: &str) -> bool {
        !module_path.starts_with("crate")
            && !module_path.starts_with("super")
            && !module_path.starts_with("self")
    }
}

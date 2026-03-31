//! Per-language configuration for the query-driven parser engine.
//!
//! Each supported language implements [`LanguageConfig`] to provide its
//! tree-sitter grammar, S-expression queries, comment style, and any
//! language-specific fixups.

use dk_core::{Symbol, SymbolKind, Visibility};

// ── Comment style ──

/// How doc-comments are written in a given language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentStyle {
    /// Rust-style `///` doc comments.
    TripleSlash,
    /// Python / Ruby / shell `#` comments.
    Hash,
    /// Go / Java / TypeScript / C `//` comments.
    SlashSlash,
    /// Haskell `--` comments.
    DashDash,
}

// ── Language configuration trait ──

/// Configuration a language must supply so the generic
/// [`QueryDrivenParser`](super) can extract symbols, calls, and imports.
pub trait LanguageConfig: Send + Sync {
    /// The tree-sitter [`Language`] grammar.
    fn language(&self) -> tree_sitter::Language;

    /// File extensions this language handles (without leading dot).
    fn extensions(&self) -> &'static [&'static str];

    /// Tree-sitter S-expression query that captures symbols.
    ///
    /// Capture names must end with a kind suffix that
    /// [`default_kind_mapping`] (or an override of
    /// [`map_capture_to_kind`](Self::map_capture_to_kind)) understands,
    /// e.g. `@definition.function`, `@definition.class`.
    fn symbols_query(&self) -> &'static str;

    /// Tree-sitter S-expression query that captures call-sites.
    fn calls_query(&self) -> &'static str;

    /// Tree-sitter S-expression query that captures import statements.
    fn imports_query(&self) -> &'static str;

    /// The comment style used for doc-comments.
    fn comment_style(&self) -> CommentStyle;

    /// Resolve the visibility of a symbol from its modifier keywords and name.
    fn resolve_visibility(&self, modifiers: Option<&str>, name: &str) -> Visibility;

    // ── Default implementations ──

    /// Map a tree-sitter capture suffix (e.g. `"function"`) to a
    /// [`SymbolKind`].  The default delegates to [`default_kind_mapping`].
    fn map_capture_to_kind(&self, capture_suffix: &str) -> Option<SymbolKind> {
        default_kind_mapping(capture_suffix)
    }

    /// Post-process hook that can mutate a [`Symbol`] after extraction.
    ///
    /// Override this when the generic engine cannot capture all details
    /// through queries alone (e.g. Rust `pub(crate)` modifiers).
    #[allow(unused_variables)]
    fn adjust_symbol(&self, sym: &mut Symbol, node: &tree_sitter::Node, source: &[u8]) {
        // no-op by default
    }

    /// Return `true` if `module_path` refers to an external dependency.
    ///
    /// Paths starting with `.`, `crate`, `self`, or `super` are considered
    /// internal by default.
    fn is_external_import(&self, module_path: &str) -> bool {
        !module_path.starts_with('.')
            && !module_path.starts_with("crate")
            && !module_path.starts_with("self")
            && !module_path.starts_with("super")
    }
}

// ── Shared helpers ──

/// Map a capture-name suffix to a [`SymbolKind`].
///
/// This is the default implementation used by
/// [`LanguageConfig::map_capture_to_kind`].  Language configs can call it
/// directly or override the method entirely.
pub fn default_kind_mapping(capture_suffix: &str) -> Option<SymbolKind> {
    match capture_suffix {
        "function" | "method" => Some(SymbolKind::Function),
        "class" => Some(SymbolKind::Class),
        "struct" => Some(SymbolKind::Struct),
        "enum" => Some(SymbolKind::Enum),
        "trait" => Some(SymbolKind::Trait),
        "impl" => Some(SymbolKind::Impl),
        "interface" => Some(SymbolKind::Interface),
        "type_alias" | "type" => Some(SymbolKind::TypeAlias),
        "const" | "expression" => Some(SymbolKind::Const),
        "static" => Some(SymbolKind::Static),
        "module" => Some(SymbolKind::Module),
        "variable" => Some(SymbolKind::Variable),
        _ => None,
    }
}

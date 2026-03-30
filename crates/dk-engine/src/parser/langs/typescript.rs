//! TypeScript/JavaScript language configuration for the query-driven parser.

use crate::parser::engine::QueryDrivenParser;
use crate::parser::lang_config::{CommentStyle, LanguageConfig};
use crate::parser::LanguageParser;
use dk_core::{Import, RawCallEdge, Result, Symbol, TypeInfo, Visibility};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Language;

/// TypeScript language configuration for [`QueryDrivenParser`].
///
/// Uses the TSX grammar (a superset of TypeScript) so `.ts`, `.tsx`, `.js`,
/// and `.jsx` files are all handled correctly.
pub struct TypeScriptConfig;

impl LanguageConfig for TypeScriptConfig {
    fn language(&self) -> Language {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx"]
    }

    fn symbols_query(&self) -> &'static str {
        include_str!("../queries/typescript_symbols.scm")
    }

    fn calls_query(&self) -> &'static str {
        include_str!("../queries/typescript_calls.scm")
    }

    fn imports_query(&self) -> &'static str {
        include_str!("../queries/typescript_imports.scm")
    }

    fn comment_style(&self) -> CommentStyle {
        CommentStyle::SlashSlash
    }

    fn resolve_visibility(&self, modifiers: Option<&str>, _name: &str) -> Visibility {
        // If @modifiers captured text (meaning the declaration was inside an
        // export_statement), the symbol is Public. Otherwise Private.
        match modifiers {
            Some(_) => Visibility::Public,
            None => Visibility::Private,
        }
    }

    fn is_external_import(&self, module_path: &str) -> bool {
        !module_path.starts_with('.') && !module_path.starts_with('/')
    }
}

/// TypeScript parser wrapper that adds qualified-name deduplication.
///
/// Multiple top-level expressions can produce the same `qualified_name`
/// (e.g. several `app.use(...)` calls). This wrapper calls the generic
/// [`QueryDrivenParser`] and then appends `#N` suffixes to duplicates so
/// every symbol has a unique key for the AST merge BTreeMap.
pub struct TypeScriptParser {
    inner: QueryDrivenParser,
}

impl TypeScriptParser {
    /// Create a new TypeScript query-driven parser.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: QueryDrivenParser::new(Box::new(TypeScriptConfig))?,
        })
    }
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new().expect("TypeScript parser initialization should not fail")
    }
}

impl LanguageParser for TypeScriptParser {
    fn extensions(&self) -> &[&str] {
        self.inner.extensions()
    }

    fn extract_symbols(&self, source: &[u8], file_path: &Path) -> Result<Vec<Symbol>> {
        let mut symbols = self.inner.extract_symbols(source, file_path)?;

        // Deduplicate qualified_names: append #N for duplicates.
        let mut seen: HashMap<String, usize> = HashMap::new();
        for sym in &mut symbols {
            let count = seen.entry(sym.qualified_name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                sym.qualified_name = format!("{}#{}", sym.qualified_name, count);
                sym.name = sym.qualified_name.clone();
            }
        }

        Ok(symbols)
    }

    fn extract_calls(&self, source: &[u8], file_path: &Path) -> Result<Vec<RawCallEdge>> {
        self.inner.extract_calls(source, file_path)
    }

    fn extract_types(&self, source: &[u8], file_path: &Path) -> Result<Vec<TypeInfo>> {
        self.inner.extract_types(source, file_path)
    }

    fn extract_imports(&self, source: &[u8], file_path: &Path) -> Result<Vec<Import>> {
        self.inner.extract_imports(source, file_path)
    }
}

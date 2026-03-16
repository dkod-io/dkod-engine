use super::LanguageParser;
use dk_core::{FileAnalysis, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Central registry that maps file extensions to their language parsers.
///
/// Each parser is wrapped in an `Arc` so multiple extensions (e.g. "ts" and
/// "tsx") can share the same parser instance without cloning.
pub struct ParserRegistry {
    parsers: HashMap<String, Arc<dyn LanguageParser>>,
}

impl ParserRegistry {
    /// Create a new registry with all built-in language parsers registered.
    pub fn new() -> Self {
        let mut parsers: HashMap<String, Arc<dyn LanguageParser>> = HashMap::new();

        // Rust
        let rust = Arc::new(super::rust_parser::RustParser::new()) as Arc<dyn LanguageParser>;
        for ext in rust.extensions() {
            parsers.insert(ext.to_string(), Arc::clone(&rust));
        }

        // TypeScript / JavaScript
        let ts = Arc::new(super::typescript_parser::TypeScriptParser::new())
            as Arc<dyn LanguageParser>;
        for ext in ts.extensions() {
            parsers.insert(ext.to_string(), Arc::clone(&ts));
        }

        // Python
        let py =
            Arc::new(super::python_parser::PythonParser::new()) as Arc<dyn LanguageParser>;
        for ext in py.extensions() {
            parsers.insert(ext.to_string(), Arc::clone(&py));
        }

        Self { parsers }
    }

    /// Return `true` if the file extension is handled by a registered parser.
    pub fn supports_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| self.parsers.contains_key(ext))
            .unwrap_or(false)
    }

    /// Parse a source file, selecting the parser by file extension.
    ///
    /// Returns `Error::UnsupportedLanguage` when no parser is registered for
    /// the extension (or the path has no extension).
    pub fn parse_file(&self, path: &Path, source: &[u8]) -> Result<FileAnalysis> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| dk_core::Error::UnsupportedLanguage("no extension".into()))?;

        let parser = self
            .parsers
            .get(ext)
            .ok_or_else(|| dk_core::Error::UnsupportedLanguage(ext.into()))?;

        parser.parse_file(source, path)
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

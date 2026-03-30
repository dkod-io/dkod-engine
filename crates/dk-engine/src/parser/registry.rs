use super::engine::QueryDrivenParser;
use super::lang_config::LanguageConfig;
use super::langs;
use super::LanguageParser;
use dk_core::{FileAnalysis, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Central registry that maps file extensions to their language parsers.
pub struct ParserRegistry {
    parsers: HashMap<String, Arc<dyn LanguageParser>>,
}

impl ParserRegistry {
    /// Create a new registry with all built-in language parsers registered.
    pub fn new() -> Self {
        let mut parsers: HashMap<String, Arc<dyn LanguageParser>> = HashMap::new();

        let mut register = |config: Box<dyn LanguageConfig>| {
            let exts: Vec<String> = config.extensions().iter().map(|s| s.to_string()).collect();
            match QueryDrivenParser::new(config) {
                Ok(parser) => {
                    let arc: Arc<dyn LanguageParser> = Arc::new(parser);
                    for ext in exts {
                        parsers.insert(ext, Arc::clone(&arc));
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to initialize parser: {e}");
                }
            }
        };

        register(Box::new(langs::rust::RustConfig));
        register(Box::new(langs::python::PythonConfig));

        // TypeScript uses a wrapper (TypeScriptParser) for dedup logic
        let ts_parser = langs::typescript::TypeScriptParser::new();
        match ts_parser {
            Ok(parser) => {
                let arc: Arc<dyn LanguageParser> = Arc::new(parser);
                for ext in ["ts", "tsx", "js", "jsx"] {
                    parsers.insert(ext.to_string(), Arc::clone(&arc));
                }
            }
            Err(e) => {
                tracing::error!("Failed to initialize TypeScript parser: {e}");
            }
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

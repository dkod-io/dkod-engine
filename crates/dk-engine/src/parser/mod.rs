pub mod engine;
pub mod lang_config;
pub mod langs;
pub mod registry;

pub use registry::ParserRegistry;

use dk_core::{FileAnalysis, Import, RawCallEdge, Result, Symbol, TypeInfo};
use std::path::Path;

/// Trait implemented by each language-specific parser.
pub trait LanguageParser: Send + Sync {
    fn extensions(&self) -> &[&str];
    fn extract_symbols(&self, source: &[u8], file_path: &Path) -> Result<Vec<Symbol>>;
    fn extract_calls(&self, source: &[u8], file_path: &Path) -> Result<Vec<RawCallEdge>>;
    fn extract_types(&self, source: &[u8], file_path: &Path) -> Result<Vec<TypeInfo>>;
    fn extract_imports(&self, source: &[u8], file_path: &Path) -> Result<Vec<Import>>;
    fn parse_file(&self, source: &[u8], file_path: &Path) -> Result<FileAnalysis> {
        Ok(FileAnalysis {
            symbols: self.extract_symbols(source, file_path)?,
            calls: self.extract_calls(source, file_path)?,
            types: self.extract_types(source, file_path)?,
            imports: self.extract_imports(source, file_path)?,
        })
    }
}

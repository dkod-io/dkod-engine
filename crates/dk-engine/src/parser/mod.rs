pub mod python_parser;
pub mod registry;
pub mod rust_parser;
pub mod typescript_parser;

pub use registry::ParserRegistry;

use dk_core::{FileAnalysis, Import, RawCallEdge, Result, Symbol, TypeInfo};
use std::path::Path;

/// Trait implemented by each language-specific parser.
///
/// Stub parsers return empty results; real tree-sitter implementations
/// will be added in Tasks 5-7.
pub trait LanguageParser: Send + Sync {
    /// File extensions this parser handles (without leading dot).
    fn extensions(&self) -> &[&str];

    /// Extract all symbols from source code.
    fn extract_symbols(&self, source: &[u8], file_path: &Path) -> Result<Vec<Symbol>>;

    /// Extract raw call edges (unresolved names).
    fn extract_calls(&self, source: &[u8], file_path: &Path) -> Result<Vec<RawCallEdge>>;

    /// Extract type information for symbols.
    fn extract_types(&self, source: &[u8], file_path: &Path) -> Result<Vec<TypeInfo>>;

    /// Extract import statements.
    fn extract_imports(&self, source: &[u8], file_path: &Path) -> Result<Vec<Import>>;

    /// Parse a file and return all extracted data.
    ///
    /// Default implementation delegates to the four individual extract methods.
    fn parse_file(&self, source: &[u8], file_path: &Path) -> Result<FileAnalysis> {
        Ok(FileAnalysis {
            symbols: self.extract_symbols(source, file_path)?,
            calls: self.extract_calls(source, file_path)?,
            types: self.extract_types(source, file_path)?,
            imports: self.extract_imports(source, file_path)?,
        })
    }
}

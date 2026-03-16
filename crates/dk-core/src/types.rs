use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

// ── ID types ──
pub type SymbolId = Uuid;
pub type RepoId = Uuid;
pub type SessionId = Uuid;
pub type AgentId = String;

// ── Span ──
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_byte: u32,
    pub end_byte: u32,
}

// ── Symbol ──
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Interface,
    TypeAlias,
    Const,
    Static,
    Module,
    Variable,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Function => "function",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::TypeAlias => "type",
            Self::Const => "const",
            Self::Static => "static",
            Self::Module => "module",
            Self::Variable => "variable",
        };
        write!(f, "{s}")
    }
}

impl FromStr for SymbolKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "function" => Ok(Self::Function),
            "struct" => Ok(Self::Struct),
            "enum" => Ok(Self::Enum),
            "trait" => Ok(Self::Trait),
            "impl" => Ok(Self::Impl),
            "class" => Ok(Self::Class),
            "interface" => Ok(Self::Interface),
            "type" => Ok(Self::TypeAlias),
            "const" => Ok(Self::Const),
            "static" => Ok(Self::Static),
            "module" => Ok(Self::Module),
            "variable" => Ok(Self::Variable),
            other => Err(format!("unknown SymbolKind: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Crate,
    Super,
}

impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Public => "Public",
            Self::Private => "Private",
            Self::Crate => "Crate",
            Self::Super => "Super",
        };
        write!(f, "{s}")
    }
}

impl FromStr for Visibility {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Public" => Ok(Self::Public),
            "Private" => Ok(Self::Private),
            "Crate" => Ok(Self::Crate),
            "Super" => Ok(Self::Super),
            other => Err(format!("unknown Visibility: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub file_path: PathBuf,
    pub span: Span,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub parent: Option<SymbolId>,
    pub last_modified_by: Option<AgentId>,
    pub last_modified_intent: Option<String>,
}

// ── Call Graph ──
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallKind {
    DirectCall,
    MethodCall,
    Import,
    Implements,
    Inherits,
    MacroInvocation,
}

impl std::fmt::Display for CallKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::DirectCall => "direct_call",
            Self::MethodCall => "method_call",
            Self::Import => "import",
            Self::Implements => "implements",
            Self::Inherits => "inherits",
            Self::MacroInvocation => "macro_invocation",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub id: Uuid,
    pub repo_id: RepoId,
    pub caller: SymbolId,
    pub callee: SymbolId,
    pub kind: CallKind,
}

/// Raw call edge with string names, before symbol resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCallEdge {
    pub caller_name: String,
    pub callee_name: String,
    pub call_site: Span,
    pub kind: CallKind,
}

// ── Dependency Graph ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub id: Uuid,
    pub repo_id: RepoId,
    pub package: String,
    pub version_req: String,
}

// ── Type Information ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeInfo {
    pub symbol_id: SymbolId,
    pub params: Vec<(String, String)>,
    pub return_type: Option<String>,
    pub fields: Vec<(String, String)>,
    pub implements: Vec<String>,
}

// ── Import ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub module_path: String,
    pub imported_name: String,
    pub alias: Option<String>,
    pub is_external: bool,
}

// ── File Analysis (parser output) ──
#[derive(Debug, Clone, Default)]
pub struct FileAnalysis {
    pub symbols: Vec<Symbol>,
    pub calls: Vec<RawCallEdge>,
    pub types: Vec<TypeInfo>,
    pub imports: Vec<Import>,
}

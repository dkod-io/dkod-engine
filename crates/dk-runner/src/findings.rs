use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Error => "[ERR]",
            Self::Warning => "[WARN]",
            Self::Info => "[INFO]",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub check_name: String,
    pub message: String,
    pub file_path: Option<String>,
    pub line: Option<u32>,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub finding_index: usize,
    pub description: String,
    pub file_path: String,
    pub replacement: Option<String>,
}

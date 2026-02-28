use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Repository not found: {0}")]
    RepoNotFound(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session expired: {0}")]
    SessionExpired(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Invalid auth token")]
    InvalidAuth,

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Git error: {0}")]
    Git(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

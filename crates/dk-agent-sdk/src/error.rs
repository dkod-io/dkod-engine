#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("authentication failed")]
    Auth,

    #[error("session expired")]
    SessionExpired,

    #[error("changeset not found: {0}")]
    ChangesetNotFound(String),

    #[error("verification failed: {0}")]
    VerificationFailed(String),

    #[error("merge conflict: {0}")]
    Conflict(String),

    #[error("server error: {0}")]
    Server(#[from] tonic::Status),

    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),
}

pub type Result<T> = std::result::Result<T, SdkError>;

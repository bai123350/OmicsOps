use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid state transition from {from} to {to}")]
    InvalidStateTransition { from: String, to: String },
    #[error("invalid remote path: {0}")]
    InvalidRemotePath(String),
    #[error("policy rejected action: {0}")]
    PolicyViolation(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type CoreResult<T> = Result<T, CoreError>;

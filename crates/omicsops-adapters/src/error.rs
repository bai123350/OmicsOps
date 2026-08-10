use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("credential store failed: {0}")]
    Credential(String),
    #[error("unsupported plan document: {0}")]
    UnsupportedDocument(String),
    #[error("document extraction failed: {0}")]
    Document(String),
    #[error("model endpoint failed: {0}")]
    Llm(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("SSH failed: {0}")]
    Ssh(String),
    #[error("authentication failed")]
    Authentication,
    #[error("host key requires confirmation: {0}")]
    HostKeyConfirmation(String),
    #[error("host key changed; expected {expected}, received {received}")]
    HostKeyChanged { expected: String, received: String },
    #[error("remote command failed with status {status}: {stderr}")]
    RemoteCommand { status: u32, stderr: String },
    #[error("artifact checksum mismatch; expected {expected}, received {received}")]
    Integrity { expected: String, received: String },
}

pub type AdapterResult<T> = Result<T, AdapterError>;

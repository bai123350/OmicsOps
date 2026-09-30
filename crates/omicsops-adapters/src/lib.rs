pub mod claude_code;
pub mod codex_auth;
pub mod credentials;
pub mod document;
pub mod kernel;
pub mod llm;
pub mod research;
pub mod responses;
pub mod skills;
pub mod ssh;

mod error;

pub use error::{AdapterError, AdapterResult};

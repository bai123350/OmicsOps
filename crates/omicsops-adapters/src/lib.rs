pub mod credentials;
pub mod document;
pub mod llm;
pub mod persistence;
pub mod ssh;

mod error;

pub use error::{AdapterError, AdapterResult};

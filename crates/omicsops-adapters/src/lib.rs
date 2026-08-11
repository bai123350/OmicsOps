pub mod credentials;
pub mod document;
pub mod llm;
pub mod kernel;
pub mod persistence;
pub mod research;
pub mod skills;
pub mod ssh;

mod error;

pub use error::{AdapterError, AdapterResult};

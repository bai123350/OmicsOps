//! Tauri's data-only boundary re-exports.
//!
//! Keeping these names in a tiny module makes the native command surface
//! explicit while the actual serde types remain usable by the wasm frontend.

pub use omicsops_dto::*;

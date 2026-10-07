//! Data-only contracts for bounded project CSV/TSV previews.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTablePreviewRequest {
    pub project_id: Uuid,
    pub backend_id: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTablePreview {
    pub project_id: Uuid,
    pub backend_id: String,
    pub relative_path: String,
    pub size_bytes: Option<u64>,
    pub read_bytes: u64,
    pub parsed_bytes: u64,
    /// Hash of the raw source bytes read, never of displayed cells.
    pub sha256: String,
    pub hash_scope: TablePreviewHashScope,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub displayed_rows: u32,
    pub truncation_reasons: Vec<TablePreviewTruncation>,
    pub source_stability: TablePreviewStability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TablePreviewHashScope {
    Full,
    Prefix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TablePreviewStability {
    Checked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TablePreviewTruncation {
    ByteLimit,
    RowLimit,
    CellLimit,
    IncompleteRecord,
    IncompleteUtf8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TablePreviewErrorCode {
    InvalidPath,
    UnsupportedFormat,
    InvalidUtf8,
    MalformedTable,
    ColumnLimit,
    SourceChanged,
    ReadFailed,
    Timeout,
    UnavailableBackend,
}

//! Shared, platform-free selection and hash contracts for workspace files.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFileSelectionV1 {
    pub backend_id: String,
    pub relative_path: String,
    pub expected_full_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceContentModeV1 {
    TextSnapshot,
    ReferenceOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileReferenceHashScopeV1 {
    FullFile,
    RecordedFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReferenceHashV1 {
    pub sha256: String,
    pub scope: FileReferenceHashScopeV1,
}

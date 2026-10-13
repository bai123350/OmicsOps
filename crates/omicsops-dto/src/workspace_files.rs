//! Shared, platform-free selection and hash contracts for workspace files.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFileBaselineRequestV1 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub branch_conversation_id: Option<Uuid>,
    pub title: String,
    pub files: Vec<ProjectFileSelectionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListFileBaselinesRequestV1 {
    pub project_id: Uuid,
    pub branch_conversation_id: Option<Uuid>,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffFileBaselineRequestV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub expected_manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadBaselineTextRequestV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub expected_manifest_sha256: String,
    pub entry_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineTextSnapshotV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub entry_index: u32,
    pub relative_path: String,
    pub full_sha256: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselineEntryV1 {
    pub backend_id: String,
    pub relative_path: Option<String>,
    pub content_mode: WorkspaceContentModeV1,
    pub size_bytes: Option<u64>,
    pub hash: Option<FileReferenceHashV1>,
    pub omission: Option<BaselineOmissionCodeV1>,
    pub captured_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselineV1 {
    pub schema_version: u8,
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch_conversation_id: Option<Uuid>,
    pub title: String,
    pub entries: Vec<FileBaselineEntryV1>,
    pub manifest_sha256: String,
    pub captured_text_bytes: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselinePageV1 {
    pub items: Vec<FileBaselineV1>,
    pub next_offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineTextHunkV1 {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<BaselineDiffLineV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineDiffLineV1 {
    pub kind: BaselineDiffLineKindV1,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselineDiffEntryV1 {
    pub baseline: FileBaselineEntryV1,
    pub state: BaselineDiffStateV1,
    pub current_size_bytes: Option<u64>,
    pub current_hash: Option<FileReferenceHashV1>,
    pub line_endings_changed: Option<bool>,
    pub final_newline_changed: Option<bool>,
    pub hunks: Vec<BaselineTextHunkV1>,
    pub hunks_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselineDiffV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub manifest_sha256: String,
    pub observed_at: String,
    pub entries: Vec<FileBaselineDiffEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselineErrorV1 {
    pub code: FileBaselineErrorCodeV1,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineOmissionCodeV1 {
    RemoteReference,
    TooLarge,
    UnsupportedContent,
    SensitiveContent,
    Missing,
    Unreadable,
    UnsafeFile,
    AggregateBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineDiffStateV1 {
    Unchanged,
    Changed,
    Missing,
    Unreadable,
    UnsafeFile,
    ReferenceOnly,
    BindingChanged,
    CurrentTooLarge,
    SensitiveCurrentContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineDiffLineKindV1 {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileBaselineErrorCodeV1 {
    InvalidSelection,
    NotFound,
    OwnershipMismatch,
    RequestConflict,
    SourceChanged,
    ManifestChanged,
    LimitExceeded,
    StorageFailed,
}

//! Independent, data-only views of the bundled synthetic example.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateOfflineExampleWorkspaceRequest {
    pub request_id: Uuid,
    pub example_id: String,
    pub parent_directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineExampleText {
    pub zh: String,
    pub en: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineExampleEntryKind {
    Question,
    Narration,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineExampleSourceKind {
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineExampleAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineExampleFileAvailability {
    Available,
    Changed,
    Missing,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineExampleUnavailableReason {
    ReceiptMissing,
    ReceiptInvalid,
    RootUnavailable,
    IdentityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineExampleEntry {
    pub id: String,
    pub kind: OfflineExampleEntryKind,
    pub text: OfflineExampleText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineExampleFileView {
    pub relative_path: String,
    pub media_type: String,
    pub expected_sha256: String,
    pub current_sha256: Option<String>,
    pub availability: OfflineExampleFileAvailability,
    pub preview_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineExampleWorkspaceView {
    pub schema_version: u8,
    pub project_id: Uuid,
    pub example_id: String,
    pub source_kind: OfflineExampleSourceKind,
    pub availability: OfflineExampleAvailability,
    pub unavailable_reason: Option<OfflineExampleUnavailableReason>,
    pub title: OfflineExampleText,
    pub disclaimer: OfflineExampleText,
    pub source_description: OfflineExampleText,
    pub license_spdx: String,
    pub fixture_sha256: String,
    pub entries: Vec<OfflineExampleEntry>,
    pub files: Vec<OfflineExampleFileView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOfflineExampleWorkspaceResult {
    pub project_id: Uuid,
    pub workspace: OfflineExampleWorkspaceView,
}

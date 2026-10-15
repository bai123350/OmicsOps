//! In-memory, read-only migration preview contracts. The digest is not approval.
use crate::{FileReferenceHashV1, ProjectFileSelectionV1};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationMetadataCategoryV1 {
    Conversations,
    Publications,
    ScientificMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectMigrationPreviewRequestV1 {
    pub project_id: Uuid,
    pub categories: Vec<MigrationMetadataCategoryV1>,
    pub files: Vec<ProjectFileSelectionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationProjectMetadataV1 {
    pub source_project_id: Uuid,
    pub name: String,
    pub description: String,
    pub template: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationCategorySummaryV1 {
    pub category: MigrationMetadataCategoryV1,
    pub selected: bool,
    pub record_count: u64,
    pub kind_counts: Vec<MigrationKindCountV1>,
    pub examples: Vec<MigrationRecordSummaryV1>,
    pub examples_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationKindCountV1 {
    pub kind: String,
    pub record_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRecordSummaryV1 {
    pub source_id: String,
    pub kind: String,
    pub label: String,
    pub status: String,
    pub revision: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationFileStateV1 {
    SmallTextCandidate,
    ReferenceOnly,
    Missing,
    UnsafeFile,
    SensitiveContent,
    UnsupportedContent,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationFilePreviewV1 {
    pub backend_id: String,
    pub relative_path: Option<String>,
    pub size_bytes: Option<u64>,
    pub hash: Option<FileReferenceHashV1>,
    pub state: MigrationFileStateV1,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRebindKindV1 {
    ProjectPaths,
    SshConnection,
    ModelProfile,
    KernelEnvironment,
    McpServer,
    SkillGrant,
    RuntimeJobs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRebindRequirementV1 {
    pub kind: MigrationRebindKindV1,
    pub count: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectMigrationPreviewV1 {
    pub schema_version: u8,
    pub project: MigrationProjectMetadataV1,
    pub observed_at: String,
    pub scientific_revision: Option<u64>,
    pub categories: Vec<MigrationCategorySummaryV1>,
    pub files: Vec<MigrationFilePreviewV1>,
    pub selected_known_file_bytes: u64,
    pub unknown_size_file_count: u32,
    pub rebind: Vec<MigrationRebindRequirementV1>,
    pub exclusions: Vec<String>,
    pub preview_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPreviewErrorCodeV1 {
    InvalidSelection,
    NotFound,
    OwnershipMismatch,
    SourceChanged,
    LimitExceeded,
    StorageReadFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPreviewErrorV1 {
    pub code: MigrationPreviewErrorCodeV1,
    pub message: String,
}

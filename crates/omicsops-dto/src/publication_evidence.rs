//! Data-only contracts for saved-publication evidence inspection and export.
use crate::{SourceAvailability, WorkspaceContentModeV1, WorkspaceSourceRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationSelectionV1 {
    pub project_id: Uuid,
    pub publication_id: Uuid,
    pub revision: u64,
    pub expected_revision_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationRevisionHashScopeV1 {
    StoredRevisionContent,
    PublicLegacyMarkdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessFindingSeverityV1 {
    Attention,
    Unverified,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessFindingCodeV1 {
    SourceChanged,
    SourceMissing,
    SourceUnavailable,
    InvalidEvidence,
    InvalidArtifact,
    StaleAnalysis,
    IncompleteProvenance,
    MissingRandomSeed,
    MissingSoftwareVersion,
    MissingDatabaseVersion,
    MissingSample,
    InvalidInput,
    ProvenanceNotRecorded,
    LineageNotRecorded,
    OriginalFileNotChecked,
    HistoricalLocationUnverified,
    LegacyRevision,
    InspectionLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessFindingV1 {
    pub code: ReadinessFindingCodeV1,
    pub severity: ReadinessFindingSeverityV1,
    pub source: Option<WorkspaceSourceRef>,
    pub related_record_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationReadinessReportV1 {
    pub schema_version: u8,
    pub selection: PublicationSelectionV1,
    pub revision_hash_scope: PublicationRevisionHashScopeV1,
    pub observed_at: String,
    pub scientific_revision: Option<u64>,
    pub scientific_state_sha256: Option<String>,
    pub references_checked: u32,
    pub related_records_checked: u32,
    pub attention_count: u32,
    pub unverified_count: u32,
    pub findings: Vec<ReadinessFindingV1>,
    pub truncated: bool,
    pub reproduction_status: ReproductionStatusV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReproductionStatusV1 {
    NotExecuted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationPackageExportResultV1 {
    pub path: String,
    pub package_sha256: String,
    pub size_bytes: u64,
    pub manifest_sha256: String,
    pub selection: PublicationSelectionV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationEvidenceErrorCodeV1 {
    InvalidSelection,
    NotFound,
    RevisionChanged,
    LimitExceeded,
    UnsafePublicContent,
    StorageReadFailed,
    ExportWriteFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationEvidenceErrorV1 {
    pub code: PublicationEvidenceErrorCodeV1,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageLocationRootV1 {
    LocalProjectRoot,
    HistoricalSshRoot,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginalFileStatusV1 {
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationPackageReferenceV1 {
    pub source: WorkspaceSourceRef,
    pub title: String,
    pub text: String,
    pub saved_snapshot_sha256: String,
    pub exported_text_sha256: String,
    pub current_availability: SourceAvailability,
    pub recorded_file_sha256: Option<String>,
    pub relative_path: Option<String>,
    pub location_root: Option<PackageLocationRootV1>,
    pub original_file_status: OriginalFileStatusV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationPackageOmissionV1 {
    pub source: Option<WorkspaceSourceRef>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationReferencePackageManifestV1 {
    pub schema_version: u8,
    pub mode: WorkspaceContentModeV1,
    pub selection: PublicationSelectionV1,
    pub revision_hash_scope: PublicationRevisionHashScopeV1,
    pub exported_at: String,
    pub reproduction_status: ReproductionStatusV1,
    pub references: Vec<PublicationPackageReferenceV1>,
    pub omissions: Vec<PublicationPackageOmissionV1>,
}

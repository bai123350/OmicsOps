use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageUsageScopeV4 {
    Managed,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageUsageCategoryV4 {
    Database,
    Skills,
    Browser,
    ProjectMetadata,
    ProjectRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageUsageStatusV4 {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageScanIssueV4 {
    NotCreated,
    Missing,
    Unreadable,
    EntryLimit,
    TimeLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageScanLimitsV4 {
    pub max_entries: u64,
    pub max_duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageUsageEntryV4 {
    pub category: StorageUsageCategoryV4,
    pub project_id: Option<Uuid>,
    pub path: String,
    /// A complete value or the known subtotal before a partial scan stopped.
    /// None means no trustworthy byte subtotal was available.
    pub known_logical_bytes: Option<u64>,
    pub status: StorageUsageStatusV4,
    /// Target roots and directory entries inspected for this category.
    pub scanned_entries: u64,
    /// Symlinks and Windows reparse points observed but deliberately not followed.
    pub skipped_links: u64,
    pub issue: Option<StorageScanIssueV4>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageUsageSnapshotV4 {
    pub scope: StorageUsageScopeV4,
    pub project_id: Option<Uuid>,
    pub entries: Vec<StorageUsageEntryV4>,
    /// Sum of known, de-duplicated logical file bytes. Partial status means
    /// unknown content exists beyond this subtotal.
    pub known_logical_bytes: u64,
    pub status: StorageUsageStatusV4,
    /// Target roots and directory entries inspected across the whole request.
    pub scanned_entries: u64,
    pub skipped_links: u64,
    pub limits: StorageScanLimitsV4,
}

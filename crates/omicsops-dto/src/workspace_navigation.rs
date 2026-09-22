//! Pure data contracts for durable workspace organization and source snapshots.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Message,
    Tool,
    Dataset,
    Analysis,
    Artifact,
    Evidence,
    Provenance,
    LegacyArtifact,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSourceRef {
    pub project_id: Uuid,
    pub kind: SourceKind,
    pub id: String,
    pub conversation_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub sequence: Option<u64>,
    pub event_hash: Option<String>,
    pub content_sha256: Option<String>,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAvailability {
    Available,
    Changed,
    Missing,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSourceSnapshot {
    pub source: WorkspaceSourceRef,
    pub title: String,
    pub text: String,
    pub sha256: String,
    pub status: String,
    pub metadata: BTreeMap<String, String>,
    pub availability: SourceAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationGroup {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationGroupMembership {
    pub conversation_id: Uuid,
    pub group_id: Uuid,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationGroupState {
    pub groups: Vec<ConversationGroup>,
    pub memberships: Vec<ConversationGroupMembership>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyEntry {
    pub source: WorkspaceSourceRef,
    pub title: String,
    pub status: String,
    pub occurred_at: String,
    pub summary: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyPage {
    pub entries: Vec<JourneyEntry>,
    pub next_offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationSummary {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub revision: u64,
    pub updated_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationRevision {
    pub id: Uuid,
    pub publication_id: Uuid,
    pub revision: u64,
    pub title: String,
    pub markdown: String,
    pub references: Vec<WorkspaceSourceSnapshot>,
    pub sha256: String,
    pub created_at: String,
    pub legacy: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationDetail {
    pub publication: PublicationSummary,
    pub revisions: Vec<PublicationRevision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Code,
    Excerpt,
    Artifact,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub id: Uuid,
    pub kind: LibraryKind,
    pub title: String,
    pub source_project_id: Uuid,
    pub source_project_name: String,
    pub source_conversation_id: Option<Uuid>,
    pub source_conversation_title: Option<String>,
    pub text_preview: String,
    pub created_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryDetail {
    pub item: LibrarySummary,
    pub snapshot: WorkspaceSourceSnapshot,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySourceProject {
    pub id: Uuid,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryPage {
    pub items: Vec<LibrarySummary>,
    pub next_offset: Option<u32>,
    pub source_projects: Vec<LibrarySourceProject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveGroupRequest {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub group_id: Option<Uuid>,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveConversationsRequest {
    pub project_id: Uuid,
    pub group_id: Option<Uuid>,
    pub conversation_ids: Vec<Uuid>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JourneyRequest {
    pub project_id: Uuid,
    pub query: String,
    pub kind: Option<SourceKind>,
    #[serde(default)]
    pub status: Option<String>,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavePublicationRequest {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub publication_id: Option<Uuid>,
    pub expected_revision: u64,
    pub title: String,
    pub markdown: String,
    pub sources: Vec<WorkspaceSourceRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestorePublicationRequest {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub publication_id: Uuid,
    pub expected_revision: u64,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListLibraryRequest {
    pub query: String,
    pub kind: Option<LibraryKind>,
    pub project_id: Option<Uuid>,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveLibraryItemRequest {
    pub request_id: Uuid,
    pub title: String,
    pub kind: LibraryKind,
    pub source: WorkspaceSourceRef,
}

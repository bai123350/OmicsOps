//! Local presentation preferences, independent of conversation and run lifecycle.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationVisibilityV1 {
    Visible,
    Shelved,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationVisibilityFilterV1 {
    Visible,
    Shelved,
    All,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationVisibilityListRequestV1 {
    pub project_id: Uuid,
    pub visibility: ConversationVisibilityFilterV1,
    pub query: String,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationVisibilityRecordV1 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub visibility: ConversationVisibilityV1,
    pub revision: u64,
    pub changed_at: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationVisibilityListItemV1 {
    pub state: ConversationVisibilityRecordV1,
    pub title: String,
    pub conversation_status: String,
    pub conversation_created_at: String,
    pub conversation_updated_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationVisibilityPageV1 {
    pub items: Vec<ConversationVisibilityListItemV1>,
    pub next_offset: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetConversationVisibilityRequestV1 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub expected_revision: u64,
    pub visibility: ConversationVisibilityV1,
}
/// Historical result of this request. Refresh current state after a replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationVisibilityReceiptV1 {
    pub request_id: Uuid,
    pub state: ConversationVisibilityRecordV1,
    pub changed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationVisibilityErrorCodeV1 {
    InvalidInput,
    NotFound,
    OwnershipMismatch,
    RevisionConflict,
    RequestConflict,
    StorageFailed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationVisibilityErrorV1 {
    pub code: ConversationVisibilityErrorCodeV1,
    pub message: String,
}

use chrono::{DateTime, Utc};
use omicsops_protocol::UsageTotalsV4;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Side-chat requests are deliberately independent of the primary Agent
/// request. The host resolves the model profile and all material IDs before
/// accepting the request; credentials, provider URLs, tools, and compute
/// authority never cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatSendRequestV4 {
    pub request_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<Uuid>,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
    pub question_markdown: String,
    #[serde(default)]
    pub references: Vec<crate::ComposerReference>,
    #[serde(default)]
    pub attachments: Vec<Uuid>,
}

/// A source excerpt is a host-created snapshot. Message IDs are navigable;
/// event sources carry run and event identity because event sequences restart
/// for each run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatSourceV4 {
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<Uuid>,
    /// SHA-256 of the complete persisted message markdown. The excerpt is
    /// redacted and bounded, so this separate hash lets the Store reject a
    /// source whose content changed between native snapshot and acceptance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_content_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<String>,
    pub sequence: u64,
    pub role: String,
    pub label: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatEventHeadV4 {
    pub run_id: Uuid,
    pub sequence: u64,
    pub event_hash: String,
}

/// A watermark describes the persisted source population at snapshot time.
/// Event sequences are per-run, so the bounded event heads retain run identity
/// and hashes instead of pretending there is one global event sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatSourceWatermarkV4 {
    pub message_count: u64,
    pub event_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_head_sequence: Option<u64>,
    #[serde(default)]
    pub event_heads: Vec<SideChatEventHeadV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideChatTurnStatusV4 {
    Queued,
    Running,
    Completed,
    NoEvidence,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideChatFailureCodeV4 {
    ConfigurationChanged,
    MaterialChanged,
    SourceChanged,
    ProviderFailed,
    InvalidResponse,
    InvalidCitation,
    LeaseUncertain,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideChatSendErrorKindV4 {
    Rejected,
    Unknown,
}

/// A rejected error guarantees that this request was not newly accepted.
/// Unknown means acceptance could not be confirmed and the same request ID
/// must be used for reconciliation. The message is host-authored and must be
/// safe for display; provider details are intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatSendErrorV4 {
    pub kind: SideChatSendErrorKindV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<SideChatFailureCodeV4>,
    pub message: String,
}

/// Public turn state returned by send/list/get. The request material and
/// source snapshot are retained so closing or remounting the panel cannot
/// silently lose an accepted main-composer draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatTurnV4 {
    pub id: Uuid,
    pub request_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<Uuid>,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
    pub model_label: String,
    pub question_markdown: String,
    #[serde(default)]
    pub references: Vec<crate::ComposerReference>,
    #[serde(default)]
    pub attachments: Vec<Uuid>,
    pub source_snapshot_sha256: String,
    pub source_watermark: SideChatSourceWatermarkV4,
    #[serde(default)]
    pub sources: Vec<SideChatSourceV4>,
    pub status: SideChatTurnStatusV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_markdown: Option<String>,
    #[serde(default)]
    pub cited_source_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<SideChatFailureCodeV4>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageTotalsV4>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Internal result of the Store's atomic acquire. Native returns only the
/// turn; `acquired` controls whether it may dispatch a provider request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatBeginResultV4 {
    pub turn: SideChatTurnV4,
    pub acquired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatListRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_request_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideChatGetRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub request_id: Uuid,
}

pub const SIDE_CHAT_MAX_QUESTION_BYTES: usize = 16 * 1024;
pub const SIDE_CHAT_MAX_ANSWER_BYTES: usize = 64 * 1024;
pub const SIDE_CHAT_MAX_SOURCE_SNAPSHOT_BYTES: usize = 64 * 1024;
pub const SIDE_CHAT_MAX_SOURCE_TEXT_BYTES: usize = 8 * 1024;
pub const SIDE_CHAT_MAX_SOURCES: usize = 128;
pub const SIDE_CHAT_MAX_EVENT_HEADS: usize = 128;
pub const SIDE_CHAT_MAX_CITATIONS: usize = 128;
pub const SIDE_CHAT_MAX_REFERENCES: usize = 12;
pub const SIDE_CHAT_MAX_ATTACHMENTS: usize = 8;
pub const SIDE_CHAT_MAX_PER_PROJECT: usize = 100;
pub const SIDE_CHAT_MAX_MODEL_LABEL_BYTES: usize = 256;
pub const SIDE_CHAT_MAX_SOURCE_ID_BYTES: usize = 256;
pub const SIDE_CHAT_MAX_ROLE_BYTES: usize = 32;
pub const SIDE_CHAT_MAX_LABEL_BYTES: usize = 256;

/// Canonical request identity excludes mutable source/output snapshots. This
/// allows a lost response to reconcile the accepted request after the
/// conversation or model profile has changed.
pub fn side_chat_request_hash(turn: &SideChatTurnV4) -> Result<String, serde_json::Error> {
    let value = serde_json::json!({
        "id": turn.id,
        "request_id": turn.request_id,
        "parent_request_id": turn.parent_request_id,
        "project_id": turn.project_id,
        "conversation_id": turn.conversation_id,
        "model_profile_id": turn.model_profile_id,
        "question_markdown": turn.question_markdown,
        "references": turn.references,
        "attachments": turn.attachments,
    });
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}

/// Canonical source snapshot covered by `source_snapshot_sha256`.
pub fn side_chat_source_snapshot_value(
    watermark: &SideChatSourceWatermarkV4,
    sources: &[SideChatSourceV4],
) -> Value {
    serde_json::json!({
        "watermark": watermark,
        "sources": sources,
    })
}

pub fn side_chat_source_snapshot_hash(
    watermark: &SideChatSourceWatermarkV4,
    sources: &[SideChatSourceV4],
) -> Result<String, serde_json::Error> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(
        &side_chat_source_snapshot_value(watermark, sources),
    )?)))
}

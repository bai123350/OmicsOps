use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::RunServiceTierV4;

/// A reviewer backend is selected by a stable host-side binding.  The
/// webview never supplies provider credentials, URLs, or model identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewerBackendChoiceV4 {
    /// Use the conversation's explicitly selected main model profile, but
    /// dispatch a separate read-only reviewer request.
    FollowSession,
    /// Resolve the explicitly configured global reviewer HTTP profile.
    DefaultHttp,
    /// Use this exact stored HTTP model profile.
    HttpProfile { profile_id: Uuid },
}

impl Default for ReviewerBackendChoiceV4 {
    fn default() -> Self {
        Self::FollowSession
    }
}

/// Global reviewer selection settings.  A missing setting retains the
/// safe, backwards-compatible FollowSession selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewerSettingsV4 {
    #[serde(default)]
    pub backend: ReviewerBackendChoiceV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_http_profile_id: Option<Uuid>,
}

impl Default for ReviewerSettingsV4 {
    fn default() -> Self {
        Self {
            backend: ReviewerBackendChoiceV4::FollowSession,
            default_http_profile_id: None,
        }
    }
}

/// Native request identity and the already-resolved model profile.  The
/// backend choice is resolved before this request crosses into the Store so a
/// persisted review can freeze one exact profile configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionReviewRequestV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionReviewSourceV4 {
    pub message_id: Uuid,
    pub sequence: u64,
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionReviewSeverityV4 {
    Error,
    Warn,
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionReviewFindingV4 {
    pub severity: SessionReviewSeverityV4,
    pub code: String,
    pub message: String,
    pub source_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionReviewReportV4 {
    pub summary: String,
    pub findings: Vec<SessionReviewFindingV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionReviewStatusV4 {
    Running,
    Completed,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionReviewRecordV4 {
    pub id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub reviewer_profile_id: Uuid,
    pub reviewer_configuration_hash: String,
    pub source_snapshot_sha256: String,
    /// Total eligible non-system, nonblank messages at snapshot time.  The
    /// bounded `sources` vector may contain only the newest portion.
    pub source_message_count: u64,
    pub sources: Vec<SessionReviewSourceV4>,
    pub status: SessionReviewStatusV4,
    pub report: Option<SessionReviewReportV4>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<RunServiceTierV4>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Result of the transactional review acquire operation.  `acquired` is
/// false when the request UUID already has a durable record and that record
/// is returned without dispatching another network call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionReviewBeginResultV4 {
    pub record: SessionReviewRecordV4,
    pub acquired: bool,
}

pub const SESSION_REVIEW_MAX_SOURCES: usize = 128;
pub const SESSION_REVIEW_MAX_SOURCE_TEXT_BYTES: usize = 8 * 1024;
pub const SESSION_REVIEW_MAX_SOURCE_SNAPSHOT_BYTES: usize = 64 * 1024;
pub const SESSION_REVIEW_MAX_REPORT_SUMMARY_BYTES: usize = 8 * 1024;
pub const SESSION_REVIEW_MAX_FINDINGS: usize = 8;
pub const SESSION_REVIEW_MAX_FINDING_CODE_BYTES: usize = 128;
pub const SESSION_REVIEW_MAX_FINDING_MESSAGE_BYTES: usize = 4 * 1024;
pub const SESSION_REVIEW_MAX_FINDING_SOURCES: usize = 12;
pub const SESSION_REVIEW_MAX_ERROR_BYTES: usize = 1024;
pub const SESSION_REVIEW_MAX_PER_PROJECT: usize = 100;

/// Canonical JSON payload covered by `source_snapshot_sha256`.  Keeping this
/// helper in the protocol crate lets native snapshot creation and Store
/// validation use exactly the same bytes.
pub fn session_review_source_snapshot_value(
    source_message_count: u64,
    sources: &[SessionReviewSourceV4],
) -> Value {
    serde_json::json!({
        "source_message_count": source_message_count,
        "sources": sources,
    })
}

pub fn session_review_source_snapshot_hash(
    source_message_count: u64,
    sources: &[SessionReviewSourceV4],
) -> Result<String, serde_json::Error> {
    let value = session_review_source_snapshot_value(source_message_count, sources);
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_and_settings_defaults_are_follow_session() {
        assert_eq!(
            ReviewerBackendChoiceV4::default(),
            ReviewerBackendChoiceV4::FollowSession
        );
        assert_eq!(
            ReviewerSettingsV4::default().backend,
            ReviewerBackendChoiceV4::FollowSession
        );
        let decoded: ReviewerSettingsV4 = serde_json::from_str("{}").expect("default settings");
        assert_eq!(decoded, ReviewerSettingsV4::default());
    }

    #[test]
    fn backend_serializes_as_stable_tagged_kind() {
        let encoded = serde_json::to_value(ReviewerBackendChoiceV4::HttpProfile {
            profile_id: Uuid::nil(),
        })
        .expect("serialize backend");
        assert_eq!(
            encoded,
            serde_json::json!({"kind":"http_profile","profile_id":Uuid::nil()})
        );
        let follow = serde_json::to_value(ReviewerBackendChoiceV4::FollowSession)
            .expect("serialize follow session");
        assert_eq!(follow, serde_json::json!({"kind":"follow_session"}));
    }

    #[test]
    fn request_rejects_unknown_fields() {
        let result = serde_json::from_value::<SessionReviewRequestV4>(serde_json::json!({
            "request_id": Uuid::nil(),
            "project_id": Uuid::nil(),
            "conversation_id": Uuid::nil(),
            "model_profile_id": Uuid::nil(),
            "unexpected": true,
        }));
        assert!(result.is_err());
    }

    #[test]
    fn source_snapshot_hash_is_stable_and_includes_count() {
        let source = SessionReviewSourceV4 {
            message_id: Uuid::nil(),
            sequence: 7,
            role: "assistant".into(),
            text: "bounded".into(),
        };
        let first =
            session_review_source_snapshot_hash(1, std::slice::from_ref(&source)).expect("hash");
        let second =
            session_review_source_snapshot_hash(1, std::slice::from_ref(&source)).expect("hash");
        let changed_count =
            session_review_source_snapshot_hash(2, std::slice::from_ref(&source)).expect("hash");
        assert_eq!(first, second);
        assert_ne!(first, changed_count);
        assert_eq!(first.len(), 64);
    }
}

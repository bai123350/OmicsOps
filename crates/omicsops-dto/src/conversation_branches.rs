use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The stable user-turn anchor used when creating a conversation branch.
///
/// `before_user` retains the transcript before the user anchor. `after_response`
/// retains the anchor's turn through the response immediately before the next
/// eligible user turn. `after_response` may use either the user message or its
/// non-empty assistant response as the stable anchor; `before_user` requires a
/// user anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationBranchCheckpointKindV4 {
    BeforeUser,
    AfterResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationBranchStateV4 {
    Active,
    Merged,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchCreateFailureKindV4 {
    Rejected,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchCreateFailureV4 {
    pub kind: BranchCreateFailureKindV4,
    pub message: String,
}

/// A read-only snapshot of the source boundary used to protect branch
/// creation from a stale paginated transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationBranchCheckpointV4 {
    pub source_message_id: Uuid,
    pub source_sequence: u64,
    pub source_head_sequence: u64,
    pub checkpoint_kind: ConversationBranchCheckpointKindV4,
    pub boundary_hash: String,
}

/// The user supplied branch creation request. The native Store generates the
/// branch conversation UUID; all expected boundary fields must match the
/// source snapshot in the same write transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateConversationBranchRequestV4 {
    /// Stable client operation identity. Retrying the same request after a
    /// lost response returns the original branch instead of creating another.
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub source_conversation_id: Uuid,
    pub source_message_id: Uuid,
    pub checkpoint_kind: ConversationBranchCheckpointKindV4,
    pub expected_source_sequence: u64,
    pub expected_head_sequence: u64,
    pub expected_boundary_hash: String,
    /// Composer draft text used as the new conversation title. Creating a
    /// branch never submits this text as a new user message.
    pub title: String,
}

/// The durable branch-and-send operation. The nested branch request freezes
/// the source transcript boundary; the remaining fields describe the complete
/// composer turn that will be accepted into the new branch's queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateConversationBranchAndSendRequestV4 {
    pub branch: CreateConversationBranchRequestV4,
    pub message_markdown: String,
    pub mode: crate::ComposerQueueModeV4,
    pub model_profile_id: Uuid,
    pub compute_selection: omicsops_protocol::ComputeSelectionV4,
    pub queue_request_id: Uuid,
    pub queue_message_id: Uuid,
    pub queue_run_id: Uuid,
    pub references: Vec<crate::ComposerReference>,
    pub attachments: Vec<Uuid>,
}

/// Result returned after the branch exists and its complete first turn has
/// been durably accepted into the branch-scoped composer queue. Queue driver
/// execution remains a separate host action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationBranchSendReceiptV4 {
    pub branch: ConversationBranchV4,
    pub queue: crate::ComposerQueueItemV4,
}

/// Durable relation between a branch conversation and its source snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationBranchV4 {
    pub request_id: Uuid,
    pub branch_conversation_id: Uuid,
    pub project_id: Uuid,
    pub source_conversation_id: Uuid,
    pub source_message_id: Uuid,
    pub checkpoint_kind: ConversationBranchCheckpointKindV4,
    pub source_sequence: u64,
    pub source_head_sequence: u64,
    pub boundary_hash: String,
    /// Hash of the complete canonical create request, retained so a reused
    /// request ID cannot silently change its source or title.
    pub request_hash: String,
    pub state: ConversationBranchStateV4,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn branch_request_uses_stable_snake_case_wire_shape() {
        let project_id = Uuid::from_u128(1);
        let source_conversation_id = Uuid::from_u128(2);
        let source_message_id = Uuid::from_u128(3);
        let request_id = Uuid::from_u128(4);
        let request = CreateConversationBranchRequestV4 {
            request_id,
            project_id,
            source_conversation_id,
            source_message_id,
            checkpoint_kind: ConversationBranchCheckpointKindV4::AfterResponse,
            expected_source_sequence: 4,
            expected_head_sequence: 7,
            expected_boundary_hash: "a".repeat(64),
            title: "draft title".into(),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "request_id": request_id,
                "project_id": project_id,
                "source_conversation_id": source_conversation_id,
                "source_message_id": source_message_id,
                "checkpoint_kind": "after_response",
                "expected_source_sequence": 4,
                "expected_head_sequence": 7,
                "expected_boundary_hash": "a".repeat(64),
                "title": "draft title"
            })
        );
    }

    #[test]
    fn branch_request_rejects_unknown_fields() {
        let value = json!({
            "request_id": Uuid::nil(),
            "project_id": Uuid::nil(),
            "source_conversation_id": Uuid::nil(),
            "source_message_id": Uuid::nil(),
            "checkpoint_kind": "before_user",
            "expected_source_sequence": 0,
            "expected_head_sequence": 0,
            "expected_boundary_hash": "a".repeat(64),
            "title": "draft",
            "send": true
        });
        assert!(serde_json::from_value::<CreateConversationBranchRequestV4>(value).is_err());
    }

    #[test]
    fn branch_failure_is_typed_and_rejects_unknown_fields() {
        let encoded = serde_json::to_value(BranchCreateFailureV4 {
            kind: BranchCreateFailureKindV4::Uncertain,
            message: "retry with the same request ID".into(),
        })
        .unwrap();
        assert_eq!(encoded["kind"], "uncertain");
        assert!(
            serde_json::from_value::<BranchCreateFailureV4>(json!({
                "kind": "rejected",
                "message": "no",
                "retry": true
            }))
            .is_err()
        );
    }

    #[test]
    fn branch_and_send_request_keeps_complete_queue_payload_and_rejects_unknown_fields() {
        let branch = CreateConversationBranchRequestV4 {
            request_id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            source_conversation_id: Uuid::from_u128(3),
            source_message_id: Uuid::from_u128(4),
            checkpoint_kind: ConversationBranchCheckpointKindV4::AfterResponse,
            expected_source_sequence: 1,
            expected_head_sequence: 2,
            expected_boundary_hash: "a".repeat(64),
            title: "branch".into(),
        };
        let request = CreateConversationBranchAndSendRequestV4 {
            branch,
            message_markdown: "send this".into(),
            mode: crate::ComposerQueueModeV4::Agent,
            model_profile_id: Uuid::from_u128(5),
            compute_selection: omicsops_protocol::ComputeSelectionV4 {
                schema_version: 4,
                backend_id: "local".into(),
                backend_kind: omicsops_protocol::ComputeBackendKindV4::Local,
                autonomy_mode: omicsops_protocol::AutonomyModeV4::Supervised,
                approval_policy: omicsops_protocol::ApprovalPolicyV4::RiskBased,
                environment: "system".into(),
                network_policy: omicsops_protocol::NetworkPolicyV4::HostInherited,
                container_image: None,
            },
            queue_request_id: Uuid::from_u128(6),
            queue_message_id: Uuid::from_u128(7),
            queue_run_id: Uuid::from_u128(8),
            references: vec![],
            attachments: vec![],
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["message_markdown"], "send this");
        assert_eq!(value["queue_request_id"], Uuid::from_u128(6).to_string());
        let mut unknown = value;
        unknown["direct_start"] = serde_json::Value::Bool(true);
        assert!(
            serde_json::from_value::<CreateConversationBranchAndSendRequestV4>(unknown).is_err()
        );
    }
}

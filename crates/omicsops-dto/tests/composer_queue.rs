use omicsops_dto::{
    ComposerQueueActionV4, ComposerQueueFrozenConfigV4, ComposerQueueItemV4, ComposerQueueModeV4,
    ComposerQueueStatusV4, EnqueueComposerTurnRequestV4,
};
use omicsops_protocol::{
    ApprovalPolicyV4, AutonomyModeV4, ComputeBackendKindV4, ComputeSelectionV4, NetworkPolicyV4,
};
use serde_json::json;
use uuid::Uuid;

fn selection() -> ComputeSelectionV4 {
    ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "local".into(),
        backend_kind: ComputeBackendKindV4::Local,
        autonomy_mode: AutonomyModeV4::Supervised,
        approval_policy: ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: NetworkPolicyV4::HostInherited,
        container_image: None,
    }
}

fn request() -> EnqueueComposerTurnRequestV4 {
    EnqueueComposerTurnRequestV4 {
        request_id: Uuid::from_u128(1),
        message_id: Uuid::from_u128(2),
        run_id: Uuid::from_u128(3),
        project_id: Uuid::from_u128(4),
        conversation_id: Uuid::from_u128(5),
        mode: ComposerQueueModeV4::Agent,
        message_markdown: "run QC".into(),
        model_profile_id: Uuid::from_u128(6),
        compute_selection: selection(),
        references: vec![],
        attachments: vec![],
    }
}

fn frozen() -> ComposerQueueFrozenConfigV4 {
    ComposerQueueFrozenConfigV4 {
        model_profile_id: Uuid::from_u128(6),
        model_configuration_hash: "a".repeat(64),
        conversation_preferences: omicsops_dto::ConversationAgentPreferencesV4::default(),
        service_tier: omicsops_protocol::RunServiceTierV4 { fast_mode: None },
        delegated_model: None,
        reviewer_model: None,
        compute_selection: selection(),
    }
}

#[test]
fn queue_request_wire_shape_is_stable() {
    let request = request();
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["mode"], "agent");
    assert_eq!(encoded["message_markdown"], "run QC");
    assert_eq!(encoded.as_object().unwrap().len(), 11);
    assert_eq!(encoded["request_id"], json!(request.request_id));
    assert_eq!(
        serde_json::from_value::<EnqueueComposerTurnRequestV4>(encoded).unwrap(),
        request
    );
}

#[test]
fn queue_item_does_not_expose_dispatch_lease_fields() {
    let value = json!({
        "request_id": Uuid::from_u128(1),
        "message_id": Uuid::from_u128(2),
        "run_id": Uuid::from_u128(3),
        "project_id": Uuid::from_u128(4),
        "conversation_id": Uuid::from_u128(5),
        "position": 1,
        "revision": 1,
        "mode": "agent",
        "message_markdown": "run QC",
        "frozen": frozen(),
        "references": [],
        "attachments": [],
        "attachment_receipts": [],
        "status": "pending",
        "created_at": "2026-09-14T00:00:00Z",
        "updated_at": "2026-09-14T00:00:00Z"
    });
    let mut item: ComposerQueueItemV4 = serde_json::from_value(value).unwrap();
    assert_eq!(item.status, ComposerQueueStatusV4::Pending);
    assert!(item.cut_in_message_id.is_none());
    item.cut_in_message_id = Some(Uuid::from_u128(7));
    let encoded = serde_json::to_value(item).unwrap();
    assert!(encoded.get("lease_owner").is_none());
    assert!(encoded.get("lease_token").is_none());
    assert_eq!(encoded["cut_in_message_id"], json!(Uuid::from_u128(7)));
    assert!(encoded.get("cutin_message_id").is_none());
}

#[test]
fn queue_action_wire_names_are_scoped_and_snake_case() {
    assert_eq!(
        serde_json::to_value(ComposerQueueActionV4::Cancel).unwrap(),
        "cancel"
    );
    assert_eq!(
        serde_json::to_value(ComposerQueueActionV4::MoveUp).unwrap(),
        "move_up"
    );
    assert_eq!(
        serde_json::to_value(ComposerQueueActionV4::MoveDown).unwrap(),
        "move_down"
    );
    assert_eq!(
        serde_json::to_value(ComposerQueueActionV4::CutIn).unwrap(),
        "cut_in"
    );
}

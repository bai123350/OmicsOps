use chrono::{TimeZone, Utc};
use omicsops_dto::{
    ComposerReference, SideChatEventHeadV4, SideChatFailureCodeV4, SideChatSendErrorKindV4,
    SideChatSendErrorV4, SideChatSendRequestV4, SideChatSourceV4, SideChatSourceWatermarkV4,
    SideChatTurnStatusV4, SideChatTurnV4,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn side_chat_wire_shape_is_tagged_bounded_and_round_trips_material() {
    let project_id = Uuid::from_u128(1);
    let conversation_id = Uuid::from_u128(2);
    let request_id = Uuid::from_u128(3);
    let reference = ComposerReference::Artifact {
        project_id,
        id: Uuid::from_u128(4),
    };
    let request = SideChatSendRequestV4 {
        request_id,
        parent_request_id: None,
        project_id,
        conversation_id,
        model_profile_id: Uuid::from_u128(5),
        question_markdown: "Explain this result".into(),
        references: vec![reference.clone()],
        attachments: vec![Uuid::from_u128(6)],
    };
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["request_id"], json!(request_id));
    assert_eq!(encoded["references"][0]["kind"], "artifact");
    assert!(
        serde_json::from_value::<SideChatSendRequestV4>({
            let mut value = encoded.clone();
            value["unexpected"] = json!(true);
            value
        })
        .is_err()
    );
    assert_eq!(
        serde_json::from_value::<SideChatSendRequestV4>(encoded).unwrap(),
        request
    );

    let source = SideChatSourceV4 {
        source_id: format!("message:{request_id}"),
        message_id: Some(request_id),
        message_content_sha256: Some("c".repeat(64)),
        run_id: None,
        event_sequence: None,
        event_hash: None,
        sequence: 7,
        role: "assistant".into(),
        label: "Assistant message".into(),
        excerpt: "bounded evidence".into(),
    };
    let turn = SideChatTurnV4 {
        id: request_id,
        request_id,
        parent_request_id: None,
        project_id,
        conversation_id,
        model_profile_id: Uuid::from_u128(5),
        model_label: "Side model".into(),
        question_markdown: "Explain this result".into(),
        references: vec![reference],
        attachments: vec![Uuid::from_u128(6)],
        source_snapshot_sha256: "a".repeat(64),
        source_watermark: SideChatSourceWatermarkV4 {
            message_count: 1,
            event_count: 0,
            message_head_sequence: Some(7),
            event_heads: vec![SideChatEventHeadV4 {
                run_id: Uuid::from_u128(8),
                sequence: 9,
                event_hash: "b".repeat(64),
            }],
        },
        sources: vec![source],
        status: SideChatTurnStatusV4::Completed,
        answer_markdown: Some("The result is bounded.".into()),
        cited_source_ids: vec![format!("message:{request_id}")],
        failure_code: None,
        usage: None,
        created_at: Utc.timestamp_millis_opt(10).single().unwrap(),
        updated_at: Utc.timestamp_millis_opt(11).single().unwrap(),
    };
    assert_eq!(
        serde_json::from_value::<SideChatTurnV4>(serde_json::to_value(&turn).unwrap()).unwrap(),
        turn
    );
}

#[test]
fn side_chat_status_failure_and_no_authority_fields_are_stable() {
    assert_eq!(
        serde_json::to_value(SideChatTurnStatusV4::NoEvidence).unwrap(),
        json!("no_evidence")
    );
    let error = SideChatSendErrorV4 {
        kind: SideChatSendErrorKindV4::Unknown,
        code: Some(SideChatFailureCodeV4::LeaseUncertain),
        message: "Retry with the same request ID".into(),
    };
    let encoded = serde_json::to_value(&error).unwrap();
    assert_eq!(encoded["kind"], "unknown");
    assert_eq!(encoded["code"], "lease_uncertain");
    let mut unknown = encoded;
    unknown["provider_credential"] = json!("secret");
    assert!(serde_json::from_value::<SideChatSendErrorV4>(unknown).is_err());
}

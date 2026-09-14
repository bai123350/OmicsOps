use chrono::{TimeZone, Utc};
use omicsops_dto::{StopRunReceiptV4, StopRunRequestV4, StopRunStatusV4};
use serde_json::json;
use uuid::Uuid;

#[test]
fn stop_request_and_receipt_use_stable_wire_contract() {
    let request_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let request = StopRunRequestV4 {
        request_id,
        project_id,
        conversation_id,
        run_id,
    };
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["request_id"], json!(request_id));
    assert_eq!(encoded["project_id"], json!(project_id));
    assert_eq!(encoded["conversation_id"], json!(conversation_id));
    assert_eq!(encoded["run_id"], json!(run_id));
    assert_eq!(encoded.as_object().unwrap().len(), 4);

    let receipt = StopRunReceiptV4 {
        request_id,
        project_id,
        conversation_id,
        run_id,
        status: StopRunStatusV4::Observed,
        created_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
        updated_at: Utc.timestamp_opt(1_700_000_042, 0).single().unwrap(),
    };
    let encoded = serde_json::to_value(&receipt).unwrap();
    assert_eq!(encoded["status"], "observed");
    assert_eq!(
        serde_json::from_value::<StopRunReceiptV4>(encoded).unwrap(),
        receipt
    );
}

#[test]
fn stop_dtos_reject_unknown_fields() {
    let id = Uuid::new_v4();
    let request = json!({
        "request_id": id,
        "project_id": Uuid::new_v4(),
        "conversation_id": Uuid::new_v4(),
        "run_id": Uuid::new_v4(),
        "authority": "cancel_remote_job"
    });
    assert!(serde_json::from_value::<StopRunRequestV4>(request).is_err());

    let receipt = json!({
        "request_id": id,
        "project_id": Uuid::new_v4(),
        "conversation_id": Uuid::new_v4(),
        "run_id": Uuid::new_v4(),
        "status": "requested",
        "created_at": "2023-11-14T22:13:20Z",
        "updated_at": "2023-11-14T22:13:20Z",
        "remote_cancelled": true
    });
    assert!(serde_json::from_value::<StopRunReceiptV4>(receipt).is_err());
}

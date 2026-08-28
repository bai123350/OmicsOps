use crate::dto::{
    ConversationAgentStateV4, GetConversationAgentModeResponseV4, RunSummaryV4, SessionAgentModeV4,
    SetConversationAgentModeRequestV4,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn session_mode_serializes_as_lowercase_wire_values() {
    assert_eq!(
        serde_json::to_value(SessionAgentModeV4::Agent).unwrap(),
        json!("agent")
    );
    assert_eq!(
        serde_json::to_value(SessionAgentModeV4::Plan).unwrap(),
        json!("plan")
    );
    assert_eq!(SessionAgentModeV4::default(), SessionAgentModeV4::Agent);
}

#[test]
fn mode_request_and_response_use_snake_case_fields() {
    let project_id = Uuid::nil();
    let conversation_id = Uuid::from_u128(1);
    let request = SetConversationAgentModeRequestV4 {
        project_id,
        conversation_id,
        mode: SessionAgentModeV4::Plan,
    };
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({
            "project_id": project_id,
            "conversation_id": conversation_id,
            "mode": "plan"
        })
    );
    let response = GetConversationAgentModeResponseV4 {
        project_id,
        conversation_id,
        mode: SessionAgentModeV4::Agent,
    };
    assert_eq!(
        serde_json::to_value(response).unwrap(),
        json!({
            "project_id": project_id,
            "conversation_id": conversation_id,
            "mode": "agent"
        })
    );
}

#[test]
fn old_run_summary_json_deserializes_with_new_optional_fields_absent() {
    let run_id = Uuid::from_u128(2);
    let summary: RunSummaryV4 = serde_json::from_value(json!({
        "run_id": run_id,
        "status": "awaiting_approval",
        "plan": null,
        "plan_hash": null,
        "compute_selection": null,
        "approval_hash": null
    }))
    .unwrap();
    assert_eq!(summary.run_id, run_id);
    assert_eq!(summary.plan_revision, None);
    assert_eq!(summary.session_mode, None);
}

#[test]
fn conversation_agent_state_contract_is_snake_case_and_backward_compatible() {
    let project_id = Uuid::from_u128(3);
    let conversation_id = Uuid::from_u128(4);
    let response = ConversationAgentStateV4 {
        project_id,
        conversation_id,
        mode: SessionAgentModeV4::Plan,
        locked: true,
        latest_plan_revision: None,
        latest_run: None,
    };
    assert_eq!(
        serde_json::to_value(response).unwrap(),
        json!({
            "project_id": project_id,
            "conversation_id": conversation_id,
            "mode": "plan",
            "locked": true,
            "latest_plan_revision": null,
            "latest_run": null
        })
    );

    let legacy: ConversationAgentStateV4 = serde_json::from_value(json!({
        "project_id": project_id,
        "conversation_id": conversation_id,
        "mode": "agent",
        "locked": false
    }))
    .unwrap();
    assert_eq!(legacy.latest_plan_revision, None);
    assert_eq!(legacy.latest_run, None);
}

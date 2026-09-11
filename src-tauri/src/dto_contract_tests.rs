use crate::dto::{
    BrowserApprovalBindingV4, BrowserApprovalScopeV4, BrowserAuthorizationV4, BrowserSessionKindV4,
    ConversationAgentStateV4, GetConversationAgentModeResponseV4, RunSummaryV4, SessionAgentModeV4,
    SetConversationAgentModeRequestV4,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn agent_iteration_settings_use_shared_validated_snake_case_contract() {
    use crate::dto::AgentIterationSettingsV4;
    assert_eq!(
        serde_json::to_value(AgentIterationSettingsV4::default()).unwrap(),
        json!({"max_iterations":100,"auto_continue":false,"auto_continue_limit":10,"auto_compact":true,"follow_up_questions":true})
    );
    let unlimited: AgentIterationSettingsV4 =
        serde_json::from_value(json!({"max_iterations":0})).unwrap();
    assert_eq!(unlimited.max_iterations, 0);
    for invalid in [
        json!({}),
        json!({"max_iterations":-1}),
        json!({"max_iterations":1.5}),
        json!({"max_iterations":4294967296_u64}),
        json!({"max_iterations":100,"extra":true}),
    ] {
        assert!(serde_json::from_value::<AgentIterationSettingsV4>(invalid).is_err());
    }
}

#[test]
fn conversation_capabilities_use_shared_snake_case_contract() {
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let value = json!({
        "project_id":project_id,"conversation_id":conversation_id,
        "skills":[{"id":Uuid::new_v4(),"name":"workflow","enabled":true}],
        "mcp_servers":[{"id":Uuid::new_v4(),"name":"science","enabled":false,"tool_count":7}],
        "memory_count":2
    });
    let snapshot: crate::dto::ConversationCapabilitiesV4 =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(snapshot).unwrap(), value);
    let request = json!({"project_id":project_id,"conversation_id":conversation_id});
    let parsed: crate::dto::GetConversationCapabilitiesV4Request =
        serde_json::from_value(request.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), request);
    assert!(
        serde_json::from_value::<crate::dto::GetConversationCapabilitiesV4Request>(
            json!({"project_id":project_id})
        )
        .is_err()
    );
}

#[test]
fn bundled_mcp_catalog_and_request_use_shared_snake_case_contract() {
    let value = json!({"id":"pubmed", "name":"Science · pubmed", "description":"Literature", "description_zh":"文献", "tool_count":7});
    let preset: crate::dto::BundledMcpPreset = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(preset).unwrap(), value);
    let request: crate::dto::AddBundledMcpServerRequest =
        serde_json::from_value(json!({"preset_id":"pubmed"})).unwrap();
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({"preset_id":"pubmed"})
    );
    assert!(
        serde_json::from_value::<crate::dto::AddBundledMcpServerRequest>(
            json!({"preset_id":"pubmed", "command":"arbitrary.exe"})
        )
        .is_err()
    );
}

#[test]
fn catalog_refresh_is_explicit_and_round_trips_through_shared_dto() {
    let mut value = json!({"label":"test","provider":"open_ai_compatible","base_url":"https://api.openai.com/v1","model":"gpt-4o"});
    let legacy: crate::dto::SaveModelProfileRequest =
        serde_json::from_value(value.clone()).unwrap();
    assert!(!legacy.refresh_catalog);
    for refresh in [false, true] {
        value["refresh_catalog"] = json!(refresh);
        let dto: crate::dto::SaveModelProfileRequest =
            serde_json::from_value(value.clone()).unwrap();
        let backend: crate::model_commands::SaveModelProfileRequest =
            serde_json::from_value(serde_json::to_value(dto).unwrap()).unwrap();
        assert_eq!(backend.refresh_catalog, refresh);
    }
    value["refresh_catalog"] = json!("true");
    assert!(serde_json::from_value::<crate::dto::SaveModelProfileRequest>(value).is_err());
}

#[test]
fn model_binding_request_distinguishes_omission_clear_and_selection() {
    let base = json!({"label":"test","provider":"ollama","base_url":"http://127.0.0.1:11434","model":"exact-test"});
    let missing: crate::model_commands::SaveModelProfileRequest =
        serde_json::from_value(base.clone()).unwrap();
    assert_eq!(missing.delegated_model_profile_id, None);
    assert!(
        serde_json::to_value(&missing)
            .unwrap()
            .get("delegated_model_profile_id")
            .is_none()
    );
    for choice in [None, Some(Uuid::new_v4())] {
        let mut value = base.clone();
        value["delegated_model_profile_id"] = json!(choice);
        let dto: crate::dto::SaveModelProfileRequest = serde_json::from_value(value).unwrap();
        assert_eq!(dto.delegated_model_profile_id, Some(choice));
        let backend: crate::model_commands::SaveModelProfileRequest =
            serde_json::from_value(serde_json::to_value(dto).unwrap()).unwrap();
        assert_eq!(backend.delegated_model_profile_id, Some(choice));
    }
}

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

#[test]
fn browser_authorization_contract_binds_scope_host_session_and_protocol() {
    let value = json!({
        "id":"grant",
        "scope":"project",
        "binding":{
            "capability":"web_scan",
            "target_host":"browser-session",
            "session":"workspace",
            "protocol_version":1
        },
        "project_id":Uuid::from_u128(7),
        "created_at_ms":1
    });
    let grant: BrowserAuthorizationV4 = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(grant.scope, BrowserApprovalScopeV4::Project);
    assert_eq!(
        grant.binding,
        BrowserApprovalBindingV4 {
            capability: "web_scan".into(),
            target_host: "browser-session".into(),
            session: BrowserSessionKindV4::Workspace,
            protocol_version: 1,
        }
    );
    assert_eq!(serde_json::to_value(grant).unwrap(), value);
}

#[test]
fn reasoning_effort_request_distinguishes_omission_clear_and_value() {
    let base = json!({"label":"test","provider":"open_ai_compatible","base_url":"https://gateway.example/v1","model":"exact-test"});
    let missing: crate::dto::SaveModelProfileRequest =
        serde_json::from_value(base.clone()).unwrap();
    assert_eq!(missing.reasoning_effort, None);
    assert!(
        serde_json::to_value(missing)
            .unwrap()
            .get("reasoning_effort")
            .is_none()
    );
    for value in [None, Some("max".to_string()), Some("none".to_string())] {
        let mut payload = base.clone();
        payload["reasoning_effort"] = json!(value);
        let dto: crate::dto::SaveModelProfileRequest = serde_json::from_value(payload).unwrap();
        let backend: crate::model_commands::SaveModelProfileRequest =
            serde_json::from_value(serde_json::to_value(dto).unwrap()).unwrap();
        assert_eq!(backend.reasoning_effort, Some(value.clone()));
        assert_eq!(
            crate::model_commands::model_profile_from_request(backend)
                .unwrap()
                .reasoning_effort,
            value
        );
    }
}

#[test]
fn streaming_preview_contract_clears_without_creating_an_audit_event() {
    let run_id = Uuid::new_v4();
    for text in [Some("public delta".to_string()), None] {
        let dto = omicsops_dto::AgentTextPreviewV4 {
            run_id,
            text: text.clone(),
        };
        let value = serde_json::to_value(&dto).unwrap();
        assert_eq!(value, json!({"run_id":run_id,"text":text}));
        assert_eq!(
            serde_json::from_value::<omicsops_dto::AgentTextPreviewV4>(value).unwrap(),
            dto
        );
    }
}

#[test]
fn session_settings_defaults_preserve_legacy_records_and_validate_new_fields() {
    use crate::dto::AgentIterationSettingsV4;
    let legacy: AgentIterationSettingsV4 =
        serde_json::from_value(json!({"max_iterations":25})).unwrap();
    let value = serde_json::to_value(legacy).unwrap();
    assert_eq!(
        value,
        json!({"max_iterations":25,"auto_continue":false,"auto_continue_limit":10,"auto_compact":true,"follow_up_questions":true})
    );
    for invalid in [
        json!({"max_iterations":100,"auto_continue":1}),
        json!({"max_iterations":100,"auto_continue_limit":-1}),
        json!({"max_iterations":100,"auto_compact":"true"}),
        json!({"max_iterations":100,"follow_up_questions":null}),
    ] {
        assert!(serde_json::from_value::<AgentIterationSettingsV4>(invalid).is_err());
    }
}

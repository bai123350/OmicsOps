use crate::dto::{
    BrowserApprovalBindingV4, BrowserApprovalScopeV4, BrowserAuthorizationV4, BrowserSessionKindV4,
    ConversationAgentStateV4, GetConversationAgentModeResponseV4, RunSummaryV4, SessionAgentModeV4,
    SetConversationAgentModeRequestV4,
};
use serde_json::json;

#[test]
fn workspace_sources_preserve_exact_identity_and_explicit_null_selectors() {
    use omicsops_dto::{SourceKind, WorkspaceSourceRef};
    let project = uuid::Uuid::from_u128(9);
    let source = WorkspaceSourceRef {
        project_id: project,
        kind: SourceKind::LegacyArtifact,
        id: "artifact-1".into(),
        conversation_id: None,
        run_id: None,
        sequence: None,
        event_hash: None,
        content_sha256: None,
        start: None,
        end: None,
    };
    assert_eq!(
        serde_json::to_value(source).unwrap(),
        json!({"project_id":project,"kind":"legacy_artifact","id":"artifact-1",
        "conversation_id":null,"run_id":null,"sequence":null,"event_hash":null,"content_sha256":null,"start":null,"end":null})
    );
    assert!(
        serde_json::from_value::<WorkspaceSourceRef>(
            json!({"project_id":project,"kind":"message","id":"x","path":"../../private"})
        )
        .is_err()
    );
}

#[test]
fn workspace_snapshot_and_pages_keep_availability_and_nullable_cursors() {
    use omicsops_dto::*;
    let source = WorkspaceSourceRef {
        project_id: uuid::Uuid::from_u128(1),
        kind: SourceKind::Tool,
        id: "call".into(),
        conversation_id: Some(uuid::Uuid::from_u128(2)),
        run_id: Some(uuid::Uuid::from_u128(3)),
        sequence: Some(7),
        event_hash: Some("a".repeat(64)),
        content_sha256: None,
        start: None,
        end: None,
    };
    let snapshot = WorkspaceSourceSnapshot {
        source,
        title: "Python".into(),
        text: "print(1)".into(),
        sha256: "b".repeat(64),
        status: "requested".into(),
        metadata: std::collections::BTreeMap::new(),
        availability: SourceAvailability::Missing,
    };
    let value = serde_json::to_value(snapshot).unwrap();
    assert_eq!(value["availability"], "missing");
    assert_eq!(value["status"], "requested");
    assert_eq!(value["source"]["sequence"], 7);
    assert_eq!(value["source"]["event_hash"], "a".repeat(64));
    assert_eq!(
        serde_json::to_value(JourneyPage {
            entries: vec![],
            next_offset: None
        })
        .unwrap(),
        json!({"entries":[],"next_offset":null})
    );
    assert_eq!(
        serde_json::to_value(LibraryPage {
            items: vec![],
            next_offset: Some(25),
            source_projects: vec![LibrarySourceProject {
                id: uuid::Uuid::from_u128(1),
                name: "Saved project".into()
            }],
        })
        .unwrap(),
        json!({"items":[],"next_offset":25,"source_projects":[{"id":uuid::Uuid::from_u128(1),"name":"Saved project"}]})
    );
    let request: JourneyRequest = serde_json::from_value(
        json!({"project_id":uuid::Uuid::from_u128(1),"query":"","kind":null,"offset":0,"limit":25}),
    )
    .unwrap();
    assert_eq!(
        request.status, None,
        "older journey callers may omit the status filter"
    );
}

#[test]
fn project_template_contracts_keep_owner_and_visible_content_fields() {
    let project_id = uuid::Uuid::from_u128(9);
    let workflow_id = uuid::Uuid::from_u128(10);
    let action = omicsops_dto::QuickAction {
        id: uuid::Uuid::from_u128(11),
        project_id,
        name: "Review QC".into(),
        description: "Insert the saved workflow.".into(),
        workflow_id,
        enabled: true,
    };
    let specialist = omicsops_dto::SpecialistTemplate {
        id: uuid::Uuid::from_u128(12),
        project_id,
        name: "Methods reviewer".into(),
        description: "Review the user's draft.".into(),
        instructions: "Identify unsupported claims.".into(),
        enabled: false,
    };

    assert_eq!(
        serde_json::to_value(action).unwrap(),
        json!({
            "id": uuid::Uuid::from_u128(11),
            "project_id": project_id,
            "name": "Review QC",
            "description": "Insert the saved workflow.",
            "workflow_id": workflow_id,
            "enabled": true
        })
    );
    assert_eq!(
        serde_json::to_value(specialist).unwrap(),
        json!({
            "id": uuid::Uuid::from_u128(12),
            "project_id": project_id,
            "name": "Methods reviewer",
            "description": "Review the user's draft.",
            "instructions": "Identify unsupported claims.",
            "enabled": false
        })
    );
}

#[test]
fn memory_file_contract_carries_project_owner_and_content_hash() {
    let project_id = uuid::Uuid::from_u128(9);
    let file = omicsops_dto::MemoryFileV4 {
        project_id,
        name: "study.md".into(),
        content: "# Study".into(),
        size_bytes: 7,
        sha256: "a".repeat(64),
    };
    assert_eq!(
        serde_json::to_value(file).unwrap(),
        json!({
            "project_id": project_id,
            "name": "study.md",
            "content": "# Study",
            "size_bytes": 7,
            "sha256": "a".repeat(64)
        })
    );
}

#[test]
fn storage_usage_snapshot_preserves_partial_and_unknown_bytes() {
    let project_id = uuid::Uuid::from_u128(1);
    let snapshot = omicsops_dto::StorageUsageSnapshotV4 {
        scope: omicsops_dto::StorageUsageScopeV4::Project,
        project_id: Some(project_id),
        entries: vec![omicsops_dto::StorageUsageEntryV4 {
            category: omicsops_dto::StorageUsageCategoryV4::ProjectRoot,
            project_id: Some(project_id),
            path: r"C:\data\project".into(),
            known_logical_bytes: None,
            status: omicsops_dto::StorageUsageStatusV4::Partial,
            scanned_entries: 0,
            skipped_links: 0,
            issue: Some(omicsops_dto::StorageScanIssueV4::Unreadable),
        }],
        known_logical_bytes: 0,
        status: omicsops_dto::StorageUsageStatusV4::Partial,
        scanned_entries: 0,
        skipped_links: 0,
        limits: omicsops_dto::StorageScanLimitsV4 {
            max_entries: 100_000,
            max_duration_ms: 2_000,
        },
    };

    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        json!({
            "scope": "project",
            "project_id": project_id,
            "entries": [{
                "category": "project_root",
                "project_id": project_id,
                "path": r"C:\data\project",
                "known_logical_bytes": null,
                "status": "partial",
                "scanned_entries": 0,
                "skipped_links": 0,
                "issue": "unreadable"
            }],
            "known_logical_bytes": 0,
            "status": "partial",
            "scanned_entries": 0,
            "skipped_links": 0,
            "limits": { "max_entries": 100_000, "max_duration_ms": 2_000 }
        })
    );
}

#[test]
fn composer_attachment_boundary_contains_only_receipts_and_explicit_staging_bytes() {
    let id = uuid::Uuid::new_v4();
    let request: omicsops_dto::StageComposerAttachmentRequest = serde_json::from_value(json!({
        "project_id":id, "conversation_id":id, "name":"qc.csv", "content_base64":"YSwx"
    }))
    .unwrap();
    assert_eq!(request.name, "qc.csv");
    assert!(serde_json::from_value::<omicsops_dto::StageComposerAttachmentRequest>(json!({
        "project_id":id, "conversation_id":id, "name":"qc.csv", "content_base64":"YSwx", "path":"C:/unselected.txt"
    })).is_err());
    let receipt = omicsops_dto::ComposerAttachmentReceipt {
        id,
        project_id: id,
        conversation_id: id,
        name: "qc.csv".into(),
        relative_path: format!(".omicsops/attachments/{id}/bytes.csv"),
        size_bytes: 3,
        sha256: "a".repeat(64),
        media_type: "text/csv".into(),
    };
    let encoded = serde_json::to_value(receipt).unwrap();
    assert_eq!(encoded["size_bytes"], 3);
    assert!(encoded.get("content_base64").is_none());
    assert!(encoded.get("absolute_path").is_none());
}
use uuid::Uuid;

#[test]
fn composer_reference_contract_uses_stable_tagged_ids() {
    let id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    for payload in [
        json!({"kind":"artifact","project_id":project_id,"id":id}),
        json!({"kind":"session","project_id":project_id,"id":id}),
        json!({"kind":"skill","id":id}),
    ] {
        let reference: omicsops_dto::ComposerReference =
            serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(reference).unwrap(), payload);
    }
    assert!(
        serde_json::from_value::<omicsops_dto::ComposerReference>(
            json!({"kind":"artifact","id":id})
        )
        .is_err()
    );
}

#[test]
fn start_requests_accept_legacy_payloads_and_preserve_reference_ids() {
    let id = Uuid::new_v4();
    let mut payload = json!({
        "project_id":id,"conversation_id":id,"model_profile_id":id,"objective":"inspect",
        "compute_selection":{"schema_version":4,"backend_id":"local","backend_kind":"local","autonomy_mode":"supervised","environment":"system","network_policy":"host_inherited"}
    });
    assert!(
        serde_json::from_value::<crate::agent_v4::StartPlanningV4Request>(payload.clone())
            .unwrap()
            .references
            .is_empty()
    );
    assert!(
        serde_json::from_value::<crate::agent_v4::StartDirectV4Request>(payload.clone())
            .unwrap()
            .references
            .is_empty()
    );
    payload["references"] = json!([{"kind":"skill","id":id}]);
    let plan: crate::agent_v4::StartPlanningV4Request =
        serde_json::from_value(payload.clone()).unwrap();
    let direct: crate::agent_v4::StartDirectV4Request = serde_json::from_value(payload).unwrap();
    assert_eq!(plan.references, direct.references);
    assert_eq!(
        serde_json::to_value(direct).unwrap()["references"],
        json!([{"kind":"skill","id":id}])
    );
}

#[test]
fn conversation_export_uses_shared_bounded_format_contract() {
    use omicsops_dto::{ConversationExportFormat, ConversationExportRequest};
    let request = ConversationExportRequest {
        format: ConversationExportFormat::Png,
        content_base64: "test".into(),
    };
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({"format":"png","content_base64":"test"})
    );
    assert!(
        serde_json::from_value::<ConversationExportRequest>(
            json!({"format":"exe","content_base64":"test"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ConversationExportRequest>(
            json!({"format":"html","content_base64":"test","path":"bypass"})
        )
        .is_err()
    );
}

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
fn transient_model_activity_contract_contains_only_phase_and_run_identity() {
    let run_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let activity = omicsops_dto::AgentModelActivityV4 {
        run_id,
        attempt_id,
        phase: "reasoning".into(),
    };
    let value = serde_json::to_value(&activity).unwrap();
    assert_eq!(
        value,
        json!({"run_id":run_id,"attempt_id":attempt_id,"phase":"reasoning"})
    );
    assert_eq!(
        serde_json::from_value::<omicsops_dto::AgentModelActivityV4>(value).unwrap(),
        activity
    );
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

#[test]
fn workflow_references_only_transport_project_and_stable_id() {
    let project_id = uuid::Uuid::new_v4();
    let id = uuid::Uuid::new_v4();
    let reference = omicsops_dto::ComposerReference::Workflow { project_id, id };
    assert_eq!(
        serde_json::to_value(reference).unwrap(),
        serde_json::json!({
            "kind": "workflow", "project_id": project_id, "id": id
        })
    );
}

#[test]
fn quote_contract_keeps_source_verification_separate_from_stable_submission_id() {
    let project_id = Uuid::new_v4();
    let id = Uuid::new_v4();
    assert_eq!(
        serde_json::to_value(omicsops_dto::ComposerReference::Quote { project_id, id }).unwrap(),
        json!({"kind":"quote","project_id":project_id,"id":id})
    );
    let request = json!({"project_id":project_id,"conversation_id":Uuid::new_v4(),"backend_id":"local","relative_path":"notes.md","sha256":"a".repeat(64),"text":"selected text"});
    assert!(
        serde_json::from_value::<omicsops_dto::CreateComposerQuoteRequest>(request.clone()).is_ok()
    );
    let mut forged = request;
    forged["absolute_path"] = json!("C:\\outside\\secret.txt");
    assert!(serde_json::from_value::<omicsops_dto::CreateComposerQuoteRequest>(forged).is_err());
}

#[test]
fn workspace_file_contract_preserves_exact_source_without_attachment_bytes() {
    let project_id = Uuid::new_v4();
    for backend_id in ["local".to_owned(), format!("ssh:{}", Uuid::new_v4())] {
        let reference = omicsops_dto::ComposerReference::WorkspaceFile {
            project_id,
            backend_id: backend_id.clone(),
            relative_path: "results/counts.csv".into(),
        };
        let wire = serde_json::to_value(&reference).unwrap();
        assert_eq!(
            wire,
            json!({"kind":"workspace_file","project_id":project_id,"backend_id":backend_id,"relative_path":"results/counts.csv"})
        );
        assert_eq!(
            serde_json::from_value::<omicsops_dto::ComposerReference>(wire).unwrap(),
            reference
        );
    }
}

#[test]
fn conversation_preferences_use_shared_optional_behavior_contract() {
    use omicsops_dto::ConversationAgentPreferencesV4;
    let defaults: ConversationAgentPreferencesV4 = serde_json::from_value(json!({})).unwrap();
    assert_eq!(
        serde_json::to_value(defaults).unwrap(),
        json!({"delegation_enabled":true,"auto_review":true,"memory_enabled":true})
    );
    let disabled: ConversationAgentPreferencesV4 = serde_json::from_value(
        json!({"delegation_enabled":false,"auto_review":false,"memory_enabled":false}),
    )
    .unwrap();
    assert!(!disabled.memory_enabled && !disabled.auto_review && !disabled.delegation_enabled);
    for mode in [serde_json::Value::Null, json!(true), json!(false)] {
        let preferences: ConversationAgentPreferencesV4 =
            serde_json::from_value(json!({"fast_mode": mode})).unwrap();
        assert_eq!(preferences.fast_mode, mode.as_bool());
    }
    assert!(
        serde_json::from_value::<ConversationAgentPreferencesV4>(json!({"fast_mode":"priority"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ConversationAgentPreferencesV4>(json!({"memory_enabled":"false"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ConversationAgentPreferencesV4>(json!({"grant_full_access":true}))
            .is_err()
    );
}

#[test]
fn retrospective_review_contract_is_scoped_and_separate_from_run_verification() {
    use omicsops_dto::{ReviewerSettingsV4, SessionReviewReportV4, SessionReviewRequestV4};
    assert_eq!(
        serde_json::to_value(ReviewerSettingsV4::default()).unwrap(),
        json!({"backend":{"kind":"follow_session"}})
    );
    let request = json!({"request_id":Uuid::new_v4(), "project_id":Uuid::new_v4(), "conversation_id":Uuid::new_v4(), "model_profile_id":Uuid::new_v4()});
    assert_eq!(
        serde_json::to_value(
            serde_json::from_value::<SessionReviewRequestV4>(request.clone()).unwrap()
        )
        .unwrap(),
        request
    );
    let mut forged = request;
    forged["sources"] = json!([{"text":"client supplied evidence"}]);
    assert!(serde_json::from_value::<SessionReviewRequestV4>(forged).is_err());
    let report = json!({"summary":"Read-only report", "findings":[{"severity":"warn","code":"missing_control","message":"Control is not described","source_ids":[Uuid::new_v4()]}]});
    assert_eq!(
        serde_json::to_value(
            serde_json::from_value::<SessionReviewReportV4>(report.clone()).unwrap()
        )
        .unwrap(),
        report
    );
    assert!(
        serde_json::from_value::<SessionReviewReportV4>(
            json!({"summary":"report","findings":[],"verified":true})
        )
        .is_err()
    );
}

#[test]
fn review_start_failure_preserves_dispatch_certainty_across_ui_boundary() {
    use omicsops_dto::{SessionReviewStartErrorV4, SessionReviewStartFailureKindV4};
    let failure = SessionReviewStartErrorV4 {
        kind: SessionReviewStartFailureKindV4::Rejected,
        message: "Review was not started".into(),
    };
    assert_eq!(
        serde_json::to_value(failure).unwrap(),
        json!({"kind":"rejected","message":"Review was not started"})
    );
    assert!(
        serde_json::from_value::<SessionReviewStartErrorV4>(
            json!({"kind":"unknown","message":"failure"})
        )
        .is_err()
    );
}

#[test]
fn stop_request_contract_is_scoped_and_does_not_accept_remote_cancellation_claims() {
    let request = json!({"request_id":Uuid::new_v4(),"project_id":Uuid::new_v4(),"conversation_id":Uuid::new_v4(),"run_id":Uuid::new_v4()});
    let parsed: omicsops_dto::StopRunRequestV4 = serde_json::from_value(request.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), request);
    let mut forged = request;
    forged["cancel_remote_jobs"] = json!(true);
    assert!(serde_json::from_value::<omicsops_dto::StopRunRequestV4>(forged).is_err());
    assert_eq!(
        serde_json::to_value(omicsops_dto::StopRunStatusV4::Observed).unwrap(),
        json!("observed")
    );
}

#[test]
fn branch_request_contract_cannot_copy_execution_authority() {
    let request = json!({"request_id":Uuid::new_v4(),"project_id":Uuid::new_v4(),"source_conversation_id":Uuid::new_v4(),"source_message_id":Uuid::new_v4(),"checkpoint_kind":"after_response","expected_source_sequence":1,"expected_head_sequence":2,"expected_boundary_hash":"a".repeat(64),"title":"Alternative analysis"});
    let parsed: omicsops_dto::CreateConversationBranchRequestV4 =
        serde_json::from_value(request.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), request);
    let mut forged = request;
    forged["copy_approvals"] = json!(true);
    assert!(
        serde_json::from_value::<omicsops_dto::CreateConversationBranchRequestV4>(forged).is_err()
    );
}

#[test]
fn replacement_contract_binds_complete_turn_and_rejects_inherited_authority() {
    use omicsops_dto::{
        ComposerReplacementErrorV4, ComposerReplacementFailureKindV4, ReplaceComposerTurnRequestV4,
    };
    let request = json!({"turn":{"request_id":Uuid::new_v4(),"message_id":Uuid::new_v4(),"run_id":Uuid::new_v4(),"project_id":Uuid::new_v4(),"conversation_id":Uuid::new_v4(),"mode":"agent","message_markdown":"New goal","model_profile_id":Uuid::new_v4(),"compute_selection":{"schema_version":4,"backend_id":"local","backend_kind":"local","autonomy_mode":"supervised","approval_policy":"risk_based","environment":"system","network_policy":"host_inherited","container_image":null},"references":[],"attachments":[Uuid::new_v4()]},"target_run_id":Uuid::new_v4(),"expected_event_sequence":3,"expected_event_hash":"a".repeat(64),"stop_request_id":Uuid::new_v4()});
    let parsed: ReplaceComposerTurnRequestV4 = serde_json::from_value(request.clone()).unwrap();
    // ComputeSelection serializes its default policy and absent container
    // sparsely; assert the normalized wire contract and the decoded meaning.
    assert_eq!(
        parsed.turn.compute_selection.approval_policy,
        omicsops_protocol::ApprovalPolicyV4::RiskBased
    );
    let mut normalized = request.clone();
    normalized["turn"]["compute_selection"]
        .as_object_mut()
        .unwrap()
        .remove("approval_policy");
    normalized["turn"]["compute_selection"]
        .as_object_mut()
        .unwrap()
        .remove("container_image");
    assert_eq!(serde_json::to_value(parsed).unwrap(), normalized);
    let mut forged = request;
    forged["copy_approvals"] = json!(true);
    assert!(serde_json::from_value::<ReplaceComposerTurnRequestV4>(forged).is_err());
    assert_eq!(
        serde_json::to_value(ComposerReplacementErrorV4 {
            kind: ComposerReplacementFailureKindV4::Unknown,
            message: "Reconcile".into()
        })
        .unwrap(),
        json!({"kind":"unknown","message":"Reconcile"})
    );
}

#[test]
fn credential_settings_contract_uses_entity_targets_without_secret_outputs() {
    use omicsops_dto::{
        CredentialConsumer, CredentialEntry, CredentialPresence, CredentialTarget,
        CredentialValueKind, ReplaceCredentialRequest,
    };
    let id = Uuid::from_u128(91);
    let entry = CredentialEntry {
        target: CredentialTarget::Ssh { id },
        reference: format!("ssh/{id}"),
        label: "Cluster".into(),
        presence: CredentialPresence::Present,
        value_kind: CredentialValueKind::SshPrivateKey,
        consumers: vec![CredentialConsumer {
            kind: "ssh".into(),
            id,
            label: "Cluster".into(),
            binding_name: None,
        }],
        can_replace: true,
        can_delete: false,
    };
    assert_eq!(
        serde_json::to_value(entry).unwrap(),
        json!({
            "target":{"kind":"ssh","id":id},
            "reference":format!("ssh/{id}"),
            "label":"Cluster",
            "presence":"present",
            "value_kind":"ssh_private_key",
            "consumers":[{"kind":"ssh","id":id,"label":"Cluster","binding_name":null}],
            "can_replace":true,
            "can_delete":false
        })
    );
    assert!(
        serde_json::from_value::<ReplaceCredentialRequest>(json!({
            "target":{"kind":"managed","id":id},
            "expected_reference":format!("settings/{id}"),
            "secret":"value"
        }))
        .is_err()
    );
}

#[test]
fn usage_settings_contract_keeps_missing_counters_distinct_from_zero_and_excludes_raw_events() {
    use omicsops_dto::{UsageAggregatePage, UsageFilter};
    use omicsops_protocol::UsageTotalsV4;

    let project_id = Uuid::from_u128(77);
    let filter = UsageFilter {
        project_id: Some(project_id),
        from: Some("2026-09-01T00:00:00Z".into()),
        until: None,
    };
    assert_eq!(
        serde_json::to_value(filter).unwrap(),
        json!({"project_id":project_id,"from":"2026-09-01T00:00:00Z"})
    );
    let page = UsageAggregatePage {
        totals: UsageTotalsV4::default(),
        projects: vec![],
        models: vec![],
        days: vec![],
        tools: vec![],
        next_cursor: None,
        scanned_runs: 0,
        omitted_runs: 0,
        unattributed_events: 0,
        snapshot_at: "2026-09-15T00:00:00Z".into(),
        completeness: "complete".into(),
    };
    let value = serde_json::to_value(page).unwrap();
    assert_eq!(
        value["totals"]["input_tokens"],
        json!({"incomplete_attempts":0})
    );
    assert!(value.get("events").is_none());
    assert!(value.get("tool_arguments").is_none());
    assert!(
        serde_json::from_value::<UsageFilter>(json!({"project_id":project_id,"raw_events":true}))
            .is_err()
    );
}

#[test]
fn mcp_environment_save_contract_distinguishes_keep_from_replace() {
    use omicsops_dto::SaveMcpEnvBindingRequest;

    assert_eq!(
        serde_json::to_value(SaveMcpEnvBindingRequest {
            name: "NCBI_EMAIL".into(),
            value: None,
            credential_reference: None,
            keep_existing: true,
        })
        .unwrap(),
        json!({"name":"NCBI_EMAIL","keep_existing":true})
    );
    assert!(
        serde_json::from_value::<SaveMcpEnvBindingRequest>(json!({
            "name":"NCBI_EMAIL",
            "keep_existing":true,
            "secret":"must-not-cross-this-contract"
        }))
        .is_err()
    );
}

#[test]
fn general_settings_contract_keeps_probe_truth_separate_from_update_configuration() {
    use omicsops_dto::{
        GeneralNativePreferences, GeneralSystemStatus, GeneralUpdateStatus,
        SystemInterpreterDiagnostic, SystemInterpreterDiagnostics, SystemInterpreterStatus,
    };

    assert_eq!(
        serde_json::to_value(GeneralNativePreferences {
            project_directory_start: Some(r"C:\Science".into()),
        })
        .unwrap(),
        json!({"project_directory_start":r"C:\Science"})
    );
    assert_eq!(
        serde_json::to_value(GeneralSystemStatus {
            app_version: "1.2.3".into(),
            app_data_directory: r"C:\OmicsOps\Data".into(),
            update_status: GeneralUpdateStatus::Unconfigured,
            update_source_configured: false,
        })
        .unwrap(),
        json!({
            "app_version":"1.2.3",
            "app_data_directory":r"C:\OmicsOps\Data",
            "update_status":"unconfigured",
            "update_source_configured":false
        })
    );
    let diagnostics = SystemInterpreterDiagnostics {
        python: SystemInterpreterDiagnostic {
            program: "python".into(),
            status: SystemInterpreterStatus::Found,
            detail: None,
        },
        r: SystemInterpreterDiagnostic {
            program: "Rscript".into(),
            status: SystemInterpreterStatus::Missing,
            detail: Some("not found on system PATH".into()),
        },
        checked_at: "2026-09-15T00:00:00Z".into(),
    };
    let value = serde_json::to_value(diagnostics).unwrap();
    assert_eq!(value["python"]["status"], "found");
    assert_eq!(value["r"]["status"], "missing");
    assert!(value.get("dependencies_installed").is_none());
}

#[test]
fn skill_installation_receipt_records_origin_without_file_contents() {
    use omicsops_dto::{SkillInstallationReceipt, SkillOrigin};

    let skill_id = uuid::Uuid::from_u128(41);
    let receipt = SkillInstallationReceipt {
        skill_id,
        package_sha256: "a".repeat(64),
        installed_root: r"C:\OmicsOps\skills\sample\sha".into(),
        origin: SkillOrigin::ManagedImport,
        owns_files: true,
        plugin_installation_id: None,
        phase: "active".into(),
    };
    assert_eq!(
        serde_json::to_value(receipt).unwrap(),
        json!({
            "skill_id": skill_id,
            "package_sha256": "a".repeat(64),
            "installed_root": r"C:\OmicsOps\skills\sample\sha",
            "origin": "managed_import",
            "owns_files": true,
            "plugin_installation_id": null,
            "phase": "active"
        })
    );
}

#[test]
fn skill_settings_detail_and_preview_are_bounded_public_contracts() {
    use omicsops_dto::{
        SkillFilePreview, SkillOrigin, SkillSettingsDetail, SkillSettingsFile, SkillSettingsPackage,
    };
    let skill_id = uuid::Uuid::from_u128(42);
    let detail = SkillSettingsDetail {
        skill: SkillSettingsPackage {
            id: skill_id,
            name: "QC".into(),
            version: "1.0.0".into(),
            source_path: r"C:\OmicsOps\skills\qc\sha".into(),
            sha256: "a".repeat(64),
            enabled: true,
            capabilities: vec!["read_project_files".into()],
            category: Some("analysis".into()),
        },
        origin: SkillOrigin::ManagedImport,
        integrity: "verified".into(),
        files: vec![SkillSettingsFile {
            relative_path: "SKILL.md".into(),
            size_bytes: 12,
            previewable: true,
        }],
        inventory_complete: true,
        dependent_skills: vec![],
        can_remove_from_library: true,
        can_delete_files: true,
        blocking_reasons: vec![],
    };
    let detail_value = serde_json::to_value(detail).unwrap();
    assert_eq!(detail_value["origin"], "managed_import");
    assert_eq!(detail_value["files"][0]["relative_path"], "SKILL.md");
    assert!(detail_value.get("content").is_none());

    let preview = SkillFilePreview {
        relative_path: "SKILL.md".into(),
        content: "# QC".into(),
        redacted: false,
        package_sha256: "a".repeat(64),
    };
    assert_eq!(
        serde_json::to_value(preview).unwrap(),
        json!({
            "relative_path":"SKILL.md",
            "content":"# QC",
            "redacted":false,
            "package_sha256":"a".repeat(64)
        })
    );
}

#[test]
fn skill_removal_contracts_expose_status_without_journal_paths() {
    use omicsops_dto::{
        RemoveSkillRequest, SkillRemovalMode, SkillRemovalOperation, SkillRemovalResult,
    };
    let skill_id = uuid::Uuid::from_u128(43);
    let operation_id = uuid::Uuid::from_u128(44);
    assert_eq!(
        serde_json::to_value(RemoveSkillRequest {
            skill_id,
            expected_package_sha256: "b".repeat(64),
            mode: SkillRemovalMode::OwnedFiles,
        })
        .unwrap(),
        json!({
            "skill_id": skill_id,
            "expected_package_sha256": "b".repeat(64),
            "mode": "owned_files"
        })
    );
    assert_eq!(
        serde_json::to_value(SkillRemovalResult {
            removed_from_library: true,
            files_removed: false,
            preserved_files: true,
            status: "needs_attention".into(),
            message: "cleanup can be retried".into(),
        })
        .unwrap(),
        json!({
            "removed_from_library": true,
            "files_removed": false,
            "preserved_files": true,
            "status": "needs_attention",
            "message": "cleanup can be retried"
        })
    );
    let operation = serde_json::to_value(SkillRemovalOperation {
        operation_id,
        skill_id,
        name: "QC".into(),
        package_sha256: "b".repeat(64),
        phase: "needs_attention".into(),
        preserved_files: true,
    })
    .unwrap();
    assert_eq!(operation["operation_id"], operation_id.to_string());
    assert!(operation.get("original_root").is_none());
    assert!(operation.get("quarantine_root").is_none());
}

#[test]
fn integration_package_contract_is_declarative_and_exposes_recovery_state() {
    use omicsops_dto::{InstallPluginRequest, InstalledPlugin, PluginPhase};
    let installation_id = uuid::Uuid::from_u128(45);
    let request = InstallPluginRequest {
        source_path: r"C:\Lab\qc-plugin".into(),
        expected_digest: "c".repeat(64),
        expected_old_digest: Some("b".repeat(64)),
    };
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({
            "source_path": r"C:\Lab\qc-plugin",
            "expected_digest": "c".repeat(64),
            "expected_old_digest": "b".repeat(64)
        })
    );
    assert!(
        serde_json::from_value::<InstallPluginRequest>(json!({
            "source_path": r"C:\Lab\qc-plugin",
            "expected_digest": "c".repeat(64),
            "expected_old_digest": null,
            "script": "install.ps1"
        }))
        .is_err()
    );

    let value = serde_json::to_value(InstalledPlugin {
        installation_id,
        package_id: "lab.qc".into(),
        version: "2.0.0".into(),
        name: "QC helpers".into(),
        digest: "c".repeat(64),
        source_path: r"C:\Lab\qc-plugin".into(),
        trust: "local_unverified".into(),
        enabled: false,
        phase: PluginPhase::NeedsAttention,
        cleanup_pending: true,
        predecessor_installation_id: Some(uuid::Uuid::from_u128(44)),
        files: vec![],
        skills: vec![],
        mcp_bindings: vec![],
        last_error: Some("cleanup can be retried".into()),
        created_at: "2026-09-21T00:00:00Z".into(),
    })
    .unwrap();
    assert_eq!(value["phase"], "needs_attention");
    assert_eq!(value["cleanup_pending"], true);
    assert_eq!(
        value["predecessor_installation_id"],
        uuid::Uuid::from_u128(44).to_string()
    );
    assert!(value.get("cleanup_path").is_none());
}

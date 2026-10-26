//! New-turn isolation after an observed Stop, using a temporary project only.
use super::*;
use omicsops_core::workspace::{Conversation, ProjectTemplate};
use omicsops_dto::{
    ComposerQueueFrozenConfigV4, ComposerQueueMaterialSnapshotV4, ComposerQueueModeV4,
    ComposerQueueStatusV4, EnqueueComposerTurnRequestV4, StopRunRequestV4,
};
use omicsops_protocol::{ApprovalPolicyV4, AutonomyModeV4, NetworkPolicyV4, RunModeV4};
use sha2::{Digest, Sha256};

#[test]
fn stopped_boundary_is_bounded_and_absent_without_stopped_runs() {
    assert_eq!(
        with_stopped_run_boundary("current request".into(), &[]),
        "current request"
    );
    let ids = (1..=24).map(Uuid::from_u128).collect::<Vec<_>>();
    let context = with_stopped_run_boundary("current request".into(), &ids);
    for id in &ids[..8] {
        assert!(context.contains(&id.to_string()));
    }
    for id in &ids[8..] {
        assert!(!context.contains(&id.to_string()));
    }
    assert!(context.contains("not an exhaustive list"));
    assert!(context.ends_with("current request"));
    assert!(context.len() < 2_000);
}

struct TestEvents(Store);

#[async_trait]
impl EventStoreV4 for TestEvents {
    async fn append(&self, event: &AgentEventV4) -> Result<(), String> {
        if let AgentEventKindV4::ToolDispatchStarted { effect, .. } = event.event {
            assert_eq!(
                effect,
                ToolEffectV4::ReadOnly,
                "new turn must not replay runtime code"
            );
        }
        self.0
            .append_agent_event_v4_with_conversation(event)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn load(&self, run_id: Uuid) -> Result<Vec<AgentEventV4>, String> {
        self.0
            .agent_events_v4(run_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn commit_context_compaction(
        &self,
        spec: &RunSpecV4,
        transcript: &str,
        events: &[AgentEventV4; 2],
    ) -> Result<(), ContextCompactionCommitErrorV4> {
        self.0
            .commit_automatic_context_compaction_v4(spec, transcript, events)
            .await
            .map_err(context_compaction_commit_error_v4)
    }
}

struct NewTurnModel {
    repository: Store,
    run_id: Uuid,
    old_run_id: Uuid,
    completion_criteria: Vec<String>,
    desktop: Arc<DesktopModelPortV4>,
    requests: std::sync::Mutex<Vec<ModelRequestV4>>,
}

#[async_trait]
impl ModelPortV4 for NewTurnModel {
    fn prompt_layers(&self) -> PromptLayersV4 {
        self.desktop.prompt_layers()
    }

    fn usage_metadata(&self) -> ModelUsageMetadataV4 {
        self.desktop.usage_metadata()
    }

    async fn stream(
        &self,
        request: ModelRequestV4,
        _: &mut (dyn FnMut(ModelStreamEventV4) + Send),
    ) -> Result<ModelTurnV4, ModelFailureV4> {
        assert!(request.context.contains(&self.old_run_id.to_string()));
        assert!(
            !serde_json::to_string(&request)
                .unwrap()
                .contains("OLD_CODE_MUST_NOT_REPLAY")
        );
        self.requests.lock().unwrap().push(request);
        let recorded = self.repository.agent_events_v4(self.run_id).await.unwrap();
        let observed = recorded.iter().find_map(|event| match &event.event {
            AgentEventKindV4::ToolFinished { outcome }
                if outcome.call_id == "new-list" && outcome.succeeded =>
            {
                Some(event.sequence)
            }
            _ => None,
        });
        let call = match observed {
            None => ToolCallV4 {
                call_id: "new-list".into(),
                tool_id: "project.list".into(),
                arguments: json!({"path":"."}),
            },
            Some(sequence) => ToolCallV4 {
                call_id: "new-complete".into(),
                tool_id: "agent.complete".into(),
                arguments: json!({
                    "schema_version":4,
                    "summary":"The new request was checked independently.",
                    "answer_markdown":"项目根目录已检查。旧操作的结果仍未确定，本次没有重试旧操作。",
                    "criteria": self.completion_criteria.iter().map(|criterion| json!({
                        "criterion": criterion,
                        "evidence": [{"kind":"event", "sequence":sequence}]
                    })).collect::<Vec<_>>()
                }),
            },
        };
        Ok(ModelTurnV4 {
            public_text: String::new(),
            tool_calls: vec![call],
            provider_continuation: None,
        })
    }
}

#[tokio::test]
async fn stopped_uncertain_turn_allows_a_fresh_queued_agent_to_complete_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let repository = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "stopped turn",
        dir.path().to_str().unwrap(),
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "same conversation", Utc::now());
    let profile: ModelProfile = serde_json::from_value(json!({
        "id":Uuid::new_v4(), "label":"offline fixture", "provider":"ollama",
        "base_url":"http://127.0.0.1:1", "model":"offline-fixture",
        "credential_reference":null, "supports_tools":true, "supports_vision":false
    }))
    .unwrap();
    repository.save_project(&project).await.unwrap();
    repository.save_conversation(&conversation).await.unwrap();
    repository.save_model_profile(&profile).await.unwrap();
    let state = AppState {
        repository: repository.clone(),
        credentials: SystemCredentialVault,
        subscription_models: Arc::new(
            crate::subscription_models::SubscriptionLoginManager::system(Default::default())
                .unwrap(),
        ),
        mcp_sessions: McpSessionManager::new(),
        active_runs: Default::default(),
        live_runtime_registry_v4: Default::default(),
        skills_root: dir.path().join("skills"),
        file_baseline_storage: crate::workspace_file_baselines::FileBaselineStorage::new(
            dir.path().join("baseline-app"),
        ),
        skills_gate: Default::default(),
        research_last_request: Default::default(),
        active_kernels: Default::default(),
        project_kernel_queues: Default::default(),
        sync_controls: Default::default(),
        browser: omicsops_browser::BrowserRuntime::new(
            dir.path().join("browser"),
            dir.path().join("extension"),
        ),
    };
    let selection = ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "local".into(),
        backend_kind: ComputeBackendKindV4::Local,
        autonomy_mode: AutonomyModeV4::Supervised,
        approval_policy: ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: NetworkPolicyV4::HostInherited,
        container_image: None,
    };
    let preferences = omicsops_protocol::ConversationAgentPreferencesV4 {
        auto_review: false,
        delegation_enabled: false,
        memory_enabled: false,
        fast_mode: None,
    };
    repository
        .set_conversation_agent_preferences(project.id, conversation.id, preferences)
        .await
        .unwrap();
    let tier = omicsops_protocol::RunServiceTierV4 { fast_mode: None };
    let old_run_id = Uuid::new_v4();
    let old_request = StartDirectV4Request {
        project_id: project.id,
        conversation_id: conversation.id,
        model_profile_id: profile.id,
        objective: "previous computation".into(),
        compute_selection: selection.clone(),
        references: vec![],
        attachments: vec![],
    };
    let (old_record, old_spec) = prepare_direct_run_v4(
        &old_request,
        old_run_id,
        "[]",
        queue_execute_capabilities(&preferences),
        DirectRunSnapshotV4 {
            stopped_run_ids: vec![],
            model_configuration_hash: profile.execution_configuration_hash(),
            conversation_preferences: preferences,
            service_tier: tier,
            reviewer_model: None,
            delegated_model: None,
            reference_context: String::new(),
            input_images: vec![],
        },
        Utc::now(),
    )
    .unwrap();
    save_record(&repository, &old_record).await.unwrap();
    let mut previous = AgentEventV4::first(
        old_run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    repository.append_agent_event_v4(&previous).await.unwrap();
    for kind in [
        AgentEventKindV4::RunSpecFrozen {
            approval_hash: old_spec.approval_hash.clone().unwrap(),
            spec_hash: old_spec.spec_hash.clone().unwrap(),
        },
        AgentEventKindV4::ToolRequested {
            call: ToolCallV4 {
                call_id: "old-runtime-call".into(),
                tool_id: "runtime.execute".into(),
                arguments: json!({"language":"python","environment":"system","code":"OLD_CODE_MUST_NOT_REPLAY"}),
            },
        },
        AgentEventKindV4::ToolDispatchStarted {
            call_id: "old-runtime-call".into(),
            tool_id: "runtime.execute".into(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: "old-runtime-call".into(),
        },
    ] {
        previous = AgentEventV4::next(&previous, Utc::now(), kind);
        repository.append_agent_event_v4(&previous).await.unwrap();
    }
    repository
        .request_run_stop_v4(&StopRunRequestV4 {
            request_id: Uuid::new_v4(),
            project_id: project.id,
            conversation_id: conversation.id,
            run_id: old_run_id,
        })
        .await
        .unwrap();
    repository
        .finalize_inactive_run_stop_v4(project.id, conversation.id, old_run_id)
        .await
        .unwrap();
    let old_events = repository.agent_events_v4(old_run_id).await.unwrap();
    let old_value = repository.agent_run_v4(old_run_id).await.unwrap();
    let old_stop = repository
        .get_run_stop_v4(project.id, conversation.id, old_run_id)
        .await
        .unwrap();
    assert!(omicsops_store::has_unresolved_side_effect_dispatch(
        &old_events
    ));
    assert_eq!(old_value.as_ref().unwrap()["status"], "needs_attention");

    let request = EnqueueComposerTurnRequestV4 {
        request_id: Uuid::new_v4(),
        message_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        project_id: project.id,
        conversation_id: conversation.id,
        mode: ComposerQueueModeV4::Agent,
        message_markdown: "检查当前项目根目录，给出结果。".into(),
        model_profile_id: profile.id,
        compute_selection: selection.clone(),
        references: vec![],
        attachments: vec![],
    };
    let frozen = ComposerQueueFrozenConfigV4 {
        model_profile_id: profile.id,
        model_configuration_hash: profile.execution_configuration_hash(),
        conversation_preferences: preferences,
        service_tier: tier,
        delegated_model: None,
        reviewer_model: None,
        compute_selection: selection.clone(),
    };
    let material = ComposerQueueMaterialSnapshotV4 {
        reference_context: String::new(),
        reference_context_sha256: hex::encode(Sha256::digest(b"")),
        attachment_receipts: vec![],
        attachment_snapshot_sha256: hex::encode(Sha256::digest(b"[]")),
    };
    repository
        .enqueue_composer_turn(&request, &frozen, &material)
        .await
        .unwrap();
    let lease = repository
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .expect("observed Stop permits a new run");
    let prepared = prepare_queued_agent(&state, &lease, material.clone(), vec![])
        .await
        .unwrap();
    assert!(prepared.spec.plan.objective.contains("HOST STOP BOUNDARY"));
    assert!(
        prepared
            .spec
            .plan
            .objective
            .contains(&old_run_id.to_string())
    );
    assert!(
        !prepared
            .spec
            .plan
            .objective
            .contains("OLD_CODE_MUST_NOT_REPLAY")
    );
    assert_eq!(prepared.record.objective, request.message_markdown);
    assert_eq!(
        prepared.record.reference_context,
        material.reference_context
    );
    assert_ne!(prepared.spec.run_id, old_run_id);
    assert_ne!(prepared.spec.approval_hash, old_spec.approval_hash);
    assert_ne!(prepared.spec.spec_hash, old_spec.spec_hash);
    prepared.spec.validate_integrity().unwrap();
    let committed = repository
        .commit_composer_queue_dispatch(
            &lease,
            &prepared.value,
            &[prepared.first, prepared.second],
            prepared.created_at,
        )
        .await
        .unwrap();
    assert_eq!(committed.status, ComposerQueueStatusV4::Running);
    let messages = repository
        .messages_for_conversation(conversation.id)
        .await
        .unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message.id == request.message_id)
    );

    let (old_model, _) = compose(
        &state,
        &project,
        &selection,
        profile.id,
        old_run_id,
        conversation.id,
        Some(&old_spec.plan.requested_capabilities),
        None,
        None,
        old_spec.model_configuration_hash.as_deref(),
        true,
        &[],
        Some(&preferences),
        Some(&tier),
        None,
    )
    .await
    .unwrap();
    let (desktop, tools) = compose(
        &state,
        &project,
        &selection,
        profile.id,
        prepared.spec.run_id,
        conversation.id,
        Some(&prepared.spec.plan.requested_capabilities),
        None,
        None,
        prepared.spec.model_configuration_hash.as_deref(),
        true,
        &[],
        Some(&preferences),
        Some(&tier),
        None,
    )
    .await
    .unwrap();
    assert!(
        desktop
            .prompt_layers()
            .environment
            .contains("HOST STOP BOUNDARY")
    );
    let old_resources = old_model
        .resources
        .as_ref()
        .unwrap()
        .initialize()
        .await
        .unwrap();
    let new_resources = desktop
        .resources
        .as_ref()
        .unwrap()
        .initialize()
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&old_resources.runtime, &new_resources.runtime));
    assert!(
        desktop
            .prompt_layers()
            .environment
            .contains("HOST STOP BOUNDARY")
    );
    assert_eq!(
        new_resources
            .runtime
            .inspect_existing(
                &ExecutionContextKeyV4 {
                    project_id: project.id,
                    run_id: request.run_id,
                    backend_id: "local".into(),
                    language: KernelLanguageV4::Python,
                    environment: "system".into(),
                },
                None
            )
            .await
            .status,
        omicsops_protocol::KernelInspectionStatusV4::NotStarted
    );
    assert_eq!(tools.run_id(), request.run_id);
    let model = NewTurnModel {
        repository: repository.clone(),
        run_id: request.run_id,
        old_run_id,
        completion_criteria: prepared.spec.plan.completion_criteria.clone(),
        desktop,
        requests: Default::default(),
    };
    let events = TestEvents(repository.clone());
    let science = RepositoryScientificStateStoreV4 {
        repository: repository.clone(),
        backend_id: selection.backend_id.clone(),
        mutation_lock: scientific_state_lock_v4(project.id),
    };
    AgentCoreV4 {
        model: &model,
        tools: tools.registry.as_ref(),
        events: &events,
        science: Some(&science),
    }
    .execute(&prepared.spec, 3)
    .await
    .unwrap();
    let recorded = repository.agent_events_v4(request.run_id).await.unwrap();
    assert!(matches!(
        recorded.last().unwrap().event,
        AgentEventKindV4::RunCompleted
    ));
    assert!(recorded.iter().any(|event| matches!(&event.event, AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == "new-list" && outcome.succeeded)));
    assert!(
        !serde_json::to_string(&recorded)
            .unwrap()
            .contains("old-runtime-call")
    );
    assert!(!recorded.iter().any(|event| matches!(
        event.event,
        AgentEventKindV4::ToolDispatchUncertain { .. }
            | AgentEventKindV4::ToolApprovalRequested { .. }
    )));
    assert_eq!(model.requests.lock().unwrap().len(), 2);
    assert_eq!(
        repository.agent_events_v4(old_run_id).await.unwrap(),
        old_events
    );
    assert_eq!(
        repository.agent_run_v4(old_run_id).await.unwrap(),
        old_value
    );
    assert_eq!(
        repository
            .get_run_stop_v4(project.id, conversation.id, old_run_id)
            .await
            .unwrap(),
        old_stop
    );
    assert!(
        reject_cancelled_execution(&repository, old_run_id)
            .await
            .is_err()
    );
}

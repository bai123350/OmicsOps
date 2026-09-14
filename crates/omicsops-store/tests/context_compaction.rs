use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::StopRunRequestV4;
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ApprovalPolicyV4, AutonomyModeV4, CompactContextRequestV4,
    CompletionProposalV4, ComputeBackendKindV4, ComputeSelectionV4, ContextCompactionStatusV4,
    ExecutionPlanV4, NetworkPolicyV4, RunModeV4, RunSpecV4, ToolApprovalRequestV4, ToolCallV4,
    ToolEffectV4,
};
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

struct Fixture {
    store: Store,
    project: Project,
    conversation: Conversation,
    run_id: Uuid,
}

async fn fixture(status: &str, with_large_event: bool, waiting_for_input: bool) -> Fixture {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "compaction fixture",
        "C:\\data\\compaction-fixture",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "compaction conversation",
        Utc::now(),
    );
    store.save_conversation(&conversation).await.unwrap();

    let run_id = Uuid::new_v4();
    let model_profile_id = Uuid::new_v4();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "compact durable context".into(),
        steps: vec!["inspect".into(), "retain evidence".into()],
        completion_criteria: vec!["original evidence remains available".into()],
        requested_capabilities: Default::default(),
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
    let approval_hash = RunSpecV4::approval_hash_for(
        run_id,
        project.id,
        conversation.id,
        model_profile_id,
        &plan,
        &selection,
    )
    .unwrap();
    let spec = RunSpecV4::freeze_with_compute(
        run_id,
        project.id,
        conversation.id,
        model_profile_id,
        plan.clone(),
        selection,
        &approval_hash,
        Utc::now(),
    )
    .unwrap();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            status,
            &json!({"status":status,"spec":spec}),
        )
        .await
        .unwrap();
    let first = AgentEventV4::first(
        run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: omicsops_protocol::RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    if with_large_event {
        let event = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ModelText {
                text: "model evidence ".repeat(2_000),
            },
        );
        store.append_agent_event_v4(&event).await.unwrap();
    }
    if waiting_for_input {
        let previous = store.agent_events_v4(run_id).await.unwrap();
        let event = AgentEventV4::next(
            previous.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::InputRequested {
                question_id: "question-1".into(),
                question: "Which retained evidence should I inspect next?".into(),
                reason: omicsops_protocol::AgentInputReasonV4::Decision,
            },
        );
        store.append_agent_event_v4(&event).await.unwrap();
    }
    if status == "completed" {
        let previous = store.agent_events_v4(run_id).await.unwrap();
        let proposal = AgentEventV4::next(
            previous.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::CompletionProposalSubmitted {
                proposal: CompletionProposalV4 {
                    schema_version: 4,
                    summary: "completed fixture".into(),
                    answer_markdown: "completed fixture".into(),
                    criteria: vec![],
                },
            },
        );
        store.append_agent_event_v4(&proposal).await.unwrap();
        let previous = store.agent_events_v4(run_id).await.unwrap();
        let event = AgentEventV4::next(
            previous.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::RunCompleted,
        );
        store.append_agent_event_v4(&event).await.unwrap();
    }
    Fixture {
        store,
        project,
        conversation,
        run_id,
    }
}

fn request(fixture: &Fixture, request_id: Uuid) -> CompactContextRequestV4 {
    CompactContextRequestV4 {
        request_id,
        project_id: fixture.project.id,
        conversation_id: fixture.conversation.id,
        run_id: fixture.run_id,
    }
}

#[tokio::test]
async fn completed_compaction_is_atomic_idempotent_and_keeps_transcript() {
    let fixture = fixture("waiting_for_input", true, true).await;
    let previous = fixture.store.agent_events_v4(fixture.run_id).await.unwrap();
    let guidance = AgentEventV4::next(
        previous.last().unwrap(),
        Utc::now(),
        AgentEventKindV4::GuidanceConsumed {
            message_id: Uuid::new_v4(),
            markdown: "keep the frozen evidence chain intact".into(),
        },
    );
    fixture
        .store
        .append_agent_event_v4(&guidance)
        .await
        .unwrap();
    let request = request(&fixture, Uuid::new_v4());
    let first = fixture.store.compact_context_v4(&request).await.unwrap();
    assert_eq!(first.status, ContextCompactionStatusV4::Completed);
    assert!(first.archive.is_some());
    assert_eq!(
        first.archive.as_ref().unwrap().size_bytes,
        first.before_bytes
    );
    assert_eq!(
        first.archive.as_ref().unwrap().sha256.len(),
        64,
        "archive hash is a durable content digest"
    );
    assert!(
        first
            .checkpoint_sha256
            .as_ref()
            .is_some_and(|hash| hash.len() == 64)
    );
    assert_eq!(first.frozen_spec_hash.as_deref().map(str::len), Some(64));

    let second = fixture.store.compact_context_v4(&request).await.unwrap();
    assert_eq!(second, first);
    let events = fixture.store.agent_events_v4(fixture.run_id).await.unwrap();
    assert!(events.iter().any(|event| matches!(
        event.event,
        AgentEventKindV4::ContextCompactionCompleted { .. }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, AgentEventKindV4::ContextArchived { .. }))
    );
    let (transcript, checkpoint) = fixture
        .store
        .agent_context_archive_v4(first.archive.unwrap().archive_id)
        .await
        .unwrap()
        .unwrap();
    assert!(transcript.contains("model evidence"));
    assert!(transcript.contains("frozen evidence chain intact"));
    assert_eq!(checkpoint.through_sequence, first.source_through_sequence);
    assert!(
        checkpoint
            .completion_criteria
            .iter()
            .any(|criterion| criterion.contains("original evidence"))
    );
}

#[tokio::test]
async fn paused_projection_can_complete_without_fabricating_idle_state() {
    let fixture = fixture("waiting_for_input", false, true).await;
    let receipt = fixture
        .store
        .compact_context_v4(&request(&fixture, Uuid::new_v4()))
        .await
        .unwrap();
    assert_eq!(receipt.status, ContextCompactionStatusV4::Completed);
    assert!(receipt.archive.is_some());
    let events = fixture.store.agent_events_v4(fixture.run_id).await.unwrap();
    assert!(events.iter().any(|event| matches!(
        event.event,
        AgentEventKindV4::ContextCompactionCompleted { .. }
    )));
}

#[tokio::test]
async fn compaction_rejects_busy_or_cross_scope_requests() {
    let first_fixture = fixture("running", true, false).await;
    let error = first_fixture
        .store
        .compact_context_v4(&request(&first_fixture, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("paused waiting_for_input"));

    let second_fixture = fixture("waiting_for_input", true, true).await;
    let request_id = Uuid::new_v4();
    let completed = second_fixture
        .store
        .compact_context_v4(&request(&second_fixture, request_id))
        .await
        .unwrap();
    let mut tampered = request(&second_fixture, request_id);
    tampered.project_id = Uuid::new_v4();
    let error = second_fixture
        .store
        .compact_context_v4(&tampered)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("another scope"));
    assert_eq!(
        second_fixture
            .store
            .compact_context_v4(&request(&second_fixture, request_id))
            .await
            .unwrap(),
        completed
    );

    let unknown = fixture("idle", true, false).await;
    let error = unknown
        .store
        .compact_context_v4(&request(&unknown, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("paused waiting_for_input"));
}

#[tokio::test]
async fn completed_run_is_an_explicit_noop_and_does_not_mutate_terminal_chain() {
    let fixture = fixture("completed", true, false).await;
    let before = fixture.store.agent_events_v4(fixture.run_id).await.unwrap();
    let receipt = fixture
        .store
        .compact_context_v4(&request(&fixture, Uuid::new_v4()))
        .await
        .unwrap();
    assert_eq!(receipt.status, ContextCompactionStatusV4::NotNeeded);
    assert!(receipt.archive.is_none());
    assert_eq!(receipt.after_bytes, Some(receipt.before_bytes));
    assert!(
        receipt
            .message
            .as_deref()
            .is_some_and(|message| message.contains("completed runs"))
    );
    let after = fixture.store.agent_events_v4(fixture.run_id).await.unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn paused_compaction_keeps_pending_question_in_checkpoint() {
    let fixture = fixture("waiting_for_input", true, true).await;
    let receipt = fixture
        .store
        .compact_context_v4(&request(&fixture, Uuid::new_v4()))
        .await
        .unwrap();
    let (_, checkpoint) = fixture
        .store
        .agent_context_archive_v4(receipt.archive.unwrap().archive_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        checkpoint
            .recent_steps
            .iter()
            .any(|step| step.contains("question-1") && step.contains("evidence"))
    );
}

#[tokio::test]
async fn paused_compaction_rejects_stop_approval_and_unresolved_side_effects() {
    let stopped = fixture("waiting_for_input", true, true).await;
    stopped
        .store
        .request_run_stop_v4(&StopRunRequestV4 {
            request_id: Uuid::new_v4(),
            project_id: stopped.project.id,
            conversation_id: stopped.conversation.id,
            run_id: stopped.run_id,
        })
        .await
        .unwrap();
    let error = stopped
        .store
        .compact_context_v4(&request(&stopped, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("stop request"));

    let approval = fixture("waiting_for_input", true, true).await;
    let previous = approval
        .store
        .agent_events_v4(approval.run_id)
        .await
        .unwrap();
    let approval_event = AgentEventV4::next(
        previous.last().unwrap(),
        Utc::now(),
        AgentEventKindV4::ToolApprovalRequested {
            request: ToolApprovalRequestV4 {
                approval_id: "approval-1".into(),
                call: ToolCallV4 {
                    call_id: "call-1".into(),
                    tool_id: "runtime.execute".into(),
                    arguments: json!({"code":"print(1)"}),
                },
                effect: ToolEffectV4::Runtime,
                reason: "approval required".into(),
                call_hash: "call-hash".into(),
                scope_hash: None,
                mode: RunModeV4::Execute,
            },
        },
    );
    approval
        .store
        .append_agent_event_v4(&approval_event)
        .await
        .unwrap();
    let error = approval
        .store
        .compact_context_v4(&request(&approval, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("approval"));

    let side_effect = fixture("waiting_for_input", true, true).await;
    let previous = side_effect
        .store
        .agent_events_v4(side_effect.run_id)
        .await
        .unwrap();
    let dispatch = AgentEventV4::next(
        previous.last().unwrap(),
        Utc::now(),
        AgentEventKindV4::ToolDispatchStarted {
            call_id: "effect-1".into(),
            tool_id: "runtime.execute".into(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: "effect-1".into(),
        },
    );
    side_effect
        .store
        .append_agent_event_v4(&dispatch)
        .await
        .unwrap();
    let error = side_effect
        .store
        .compact_context_v4(&request(&side_effect, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("side-effect dispatch"));
}

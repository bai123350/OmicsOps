use chrono::Utc;
use omicsops_core::workspace::{Conversation, Message, MessageRole, Project, ProjectTemplate};
use omicsops_dto::{
    AgentV4RequestPlanRevisionRequest, PlanRevisionStatusV4, SessionAgentModeV4,
    SetConversationAgentModeRequestV4,
};
use omicsops_protocol::{
    ApprovalPolicyV4, AutonomyModeV4, ComputeBackendKindV4, ComputeSelectionV4, ExecutionPlanV4,
    NetworkPolicyV4, RunSpecV4,
};
use omicsops_store::Store;
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::AtomicBool},
};
use uuid::Uuid;

use crate::agent_commands::persist_submitted_message;
use crate::agent_v4::{
    ApprovePlanV4Request, PlanApprovalRunContext, approve_plan_revision_for_command,
    begin_v4_plan_resume, cancel_active_run_command_response, cancel_active_run_for_command,
    ensure_v4_resume_allowed, ensure_v4_start_allowed, plan_generation_is_active,
    request_plan_revision_command_response, request_plan_revision_response,
};
use crate::conversation_mode::set_conversation_agent_mode_response;

#[tokio::test]
async fn revision_request_command_emit_failure_does_not_hide_commit_or_block_resume() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "command project",
        r"C:\data\command-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "command conversation",
        Utc::now(),
    );
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &json!({
                "run_id": run_id,
                "project_id": project.id,
                "conversation_id": conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "command plan",
                "status": "awaiting_approval",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
        )
        .await
        .unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "command plan".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    let revision = store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            plan.clone(),
            "# command plan".into(),
            plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let mut emit_attempts = 0;
    let response = request_plan_revision_command_response(
        &store,
        &AgentV4RequestPlanRevisionRequest {
            run_id,
            plan_hash: revision.plan_hash.clone(),
            feedback: "add a control".into(),
        },
        |_event| {
            emit_attempts += 1;
            Err("emit failed".into())
        },
    )
    .await
    .unwrap();
    assert_eq!(emit_attempts, 1);
    assert_eq!(response.revision, 1);
    assert_eq!(response.status, PlanRevisionStatusV4::Revising);
    assert_eq!(
        serde_json::to_value(response).unwrap()["plan_hash"],
        revision.plan_hash
    );
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "planning"
    );
    let latest = store
        .latest_proposed_plan_revision_v4(project.id, conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, PlanRevisionStatusV4::Revising);
    assert_eq!(latest.feedback.as_deref(), Some("add a control"));
    let events = store.agent_events_v4(run_id).await.unwrap();
    assert_eq!(events.len(), 1);
    events[0].verify().unwrap();
    let generating = begin_v4_plan_resume(&store, run_id).await.unwrap();
    assert_eq!(generating.revision, 2);
    assert_eq!(generating.status, PlanRevisionStatusV4::Generating);
}

#[tokio::test]
async fn cancellation_emit_failure_does_not_hide_the_durable_commit() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "cancel emit project",
        r"C:\data\cancel-emit",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "cancel emit conversation",
        Utc::now(),
    );
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .start_plan_run_v4(
            run_id,
            project.id,
            conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": project.id,
                "conversation_id": conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "cancel after commit",
                "status": "planning",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
            "cancel after commit",
            Utc::now(),
        )
        .await
        .unwrap();
    let mut attempts = 0;
    cancel_active_run_command_response(&store, run_id, None, |_event| {
        attempts += 1;
        Err("listener closed".into())
    })
    .await
    .unwrap();

    assert_eq!(attempts, 1);
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    assert_eq!(
        store
            .latest_proposed_plan_revision_v4(project.id, conversation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Cancelled
    );
    assert_eq!(
        store
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Plan
    );
    assert!(
        !store
            .is_conversation_locked_v4(project.id, conversation.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn ordinary_direct_or_planning_start_is_rejected_only_for_the_locked_conversation() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "lock project",
        r"C:\data\lock-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "locked", Utc::now());
    let other = Conversation::new(Uuid::new_v4(), project.id, "free", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    store.save_conversation(&other).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &json!({"status":"awaiting_approval"}),
        )
        .await
        .unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "locked plan".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            plan.clone(),
            "# locked plan".into(),
            plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let error = ensure_v4_start_allowed(&store, project.id, conversation.id)
        .await
        .unwrap_err();
    assert!(error.contains("locked"));
    ensure_v4_start_allowed(&store, project.id, other.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn pending_resume_is_rejected_before_legacy_spec_fallback() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "pending resume project",
        r"C:\data\pending-resume",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "pending", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &json!({
                "run_id": run_id,
                "project_id": project.id,
                "conversation_id": conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "pending resume",
                "status":"awaiting_approval",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": 1,
                "spec":null
            }),
        )
        .await
        .unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "pending resume".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            plan.clone(),
            "# pending resume".into(),
            plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let error = ensure_v4_resume_allowed(&store, run_id).await.unwrap_err();
    assert!(error.contains("pending"));
}

#[tokio::test]
async fn cancelled_resume_is_rejected_and_does_not_revive_the_plan() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "cancel resume project",
        r"C:\data\cancel-resume",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "cancelled", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &json!({
                "run_id": run_id,
                "project_id": project.id,
                "conversation_id": conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "cancel resume",
                "status":"awaiting_approval",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": 1,
                "spec":null
            }),
        )
        .await
        .unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "cancel resume".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            plan.clone(),
            "# cancel resume".into(),
            plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Cancelled,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let error = ensure_v4_resume_allowed(&store, run_id).await.unwrap_err();
    assert!(error.contains("cancelled") || error.contains("terminal"));
}

#[tokio::test]
async fn manual_agent_mode_switch_is_rejected_while_plan_revision_is_locked() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "mode lock project",
        r"C:\data\mode-lock",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "locked", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &json!({"status":"awaiting_approval"}),
        )
        .await
        .unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "mode lock".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            plan.clone(),
            "# mode lock".into(),
            plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let error = set_conversation_agent_mode_response(
        &store,
        SetConversationAgentModeRequestV4 {
            project_id: project.id,
            conversation_id: conversation.id,
            mode: SessionAgentModeV4::Agent,
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("locked"));
}

#[tokio::test]
async fn request_then_resume_service_accepts_planning_status_and_creates_revision_two() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "request resume project",
        r"C:\data\request-resume",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "resume", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    let model_profile_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &json!({
                "run_id": run_id,
                "project_id": project.id,
                "conversation_id": conversation.id,
                "model_profile_id": model_profile_id,
                "objective": "request resume",
                "status": "awaiting_approval",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": 1,
                "spec": null
            }),
        )
        .await
        .unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "request resume".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    let revision = store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            plan.clone(),
            "# request resume".into(),
            plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    request_plan_revision_response(
        &store,
        &AgentV4RequestPlanRevisionRequest {
            run_id,
            plan_hash: revision.plan_hash,
            feedback: "revise it".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "planning"
    );
    let generating = begin_v4_plan_resume(&store, run_id).await.unwrap();
    assert_eq!(generating.revision, 2);
    assert_eq!(generating.status, PlanRevisionStatusV4::Generating);
}

#[tokio::test]
async fn active_plan_cancel_service_persists_before_return_and_sets_token() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "active cancel project",
        r"C:\data\active-cancel",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "active", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": project.id,
                "conversation_id": conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "active cancel",
                "status": "planning",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
        )
        .await
        .unwrap();
    let generation = store
        .acquire_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            "active cancel",
            Utc::now(),
        )
        .await
        .unwrap();
    let token = Arc::new(AtomicBool::new(false));
    let result = cancel_active_run_for_command(&store, run_id, Some(token.clone()))
        .await
        .unwrap()
        .expect("planning cancellation is durable");
    assert!(token.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(result.revision, generation.revision);
    assert_eq!(
        store
            .proposed_plan_revision_v4(generation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Cancelled
    );
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    assert!(matches!(
        store
            .agent_events_v4(run_id)
            .await
            .unwrap()
            .last()
            .map(|event| &event.event),
        Some(omicsops_protocol::AgentEventKindV4::RunCancelled)
    ));
    assert_eq!(
        store
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Plan
    );
    assert!(
        !store
            .is_conversation_locked_v4(project.id, conversation.id)
            .await
            .unwrap()
    );
    let replay = cancel_active_run_for_command(&store, run_id, None)
        .await
        .unwrap()
        .expect("repeated planning cancellation is idempotent");
    assert!(replay.events.is_empty());
}

#[tokio::test]
async fn cancellation_between_acquire_and_registration_prevents_model_entry() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "registration gap project",
        r"C:\data\registration-gap",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "gap", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": project.id,
                "conversation_id": conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "registration gap",
                "status": "planning",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
        )
        .await
        .unwrap();
    let generation = store
        .acquire_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            "registration gap",
            Utc::now(),
        )
        .await
        .unwrap();
    cancel_active_run_for_command(&store, run_id, None)
        .await
        .unwrap()
        .expect("cancel without a registered token still persists");
    assert!(
        !plan_generation_is_active(&store, generation.id)
            .await
            .unwrap()
    );
    assert_eq!(store.agent_events_v4(run_id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn legacy_approval_service_creates_revision_one_then_approves_atomically() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "legacy approval project",
        r"C:\data\legacy-approval",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "legacy", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    let model_profile_id = Uuid::new_v4();
    let legacy_plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "legacy approval".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    let legacy_hash = legacy_plan.canonical_hash().unwrap();
    let run_value = json!({
        "run_id": run_id,
        "project_id": project.id,
        "conversation_id": conversation.id,
        "model_profile_id": model_profile_id,
        "objective": "legacy approval",
        "status": "awaiting_approval",
        "plan": legacy_plan.clone(),
        "plan_hash": legacy_hash,
        "compute_selection": null,
        "approval_hash": null,
        "plan_revision": null,
        "spec": null
    });
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &run_value,
        )
        .await
        .unwrap();
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
        &legacy_plan,
        &selection,
    )
    .unwrap();
    let result = approve_plan_revision_for_command(
        &store,
        PlanApprovalRunContext {
            project_id: project.id,
            conversation_id: conversation.id,
            run_id,
            model_profile_id,
        },
        Some(legacy_hash.as_str()),
        legacy_plan,
        selection,
        &ApprovePlanV4Request {
            run_id,
            approval_hash: Some(approval_hash),
            plan_hash: None,
            revision: None,
        },
        &run_value,
    )
    .await
    .unwrap();
    assert_eq!(result.revision, 1);
    assert_eq!(result.approval.revision, 1);
    assert_eq!(
        store
            .proposed_plan_revisions_v4(project.id, conversation.id)
            .await
            .unwrap()
            .first()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Approved
    );
    assert!(
        !store
            .is_conversation_locked_v4(project.id, conversation.id)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Agent
    );
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "running"
    );
}

#[tokio::test]
async fn legacy_approval_service_rejects_bad_hash_before_materialization() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "legacy bad hash project",
        r"C:\data\legacy-bad-hash",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "legacy", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    let model_profile_id = Uuid::new_v4();
    let legacy_plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "legacy bad hash".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    let legacy_hash = legacy_plan.canonical_hash().unwrap();
    let run_value = json!({
        "run_id": run_id,
        "project_id": project.id,
        "conversation_id": conversation.id,
        "model_profile_id": model_profile_id,
        "objective": "legacy bad hash",
        "status": "awaiting_approval",
        "plan": legacy_plan.clone(),
        "plan_hash": legacy_hash,
        "compute_selection": null,
        "approval_hash": null,
        "plan_revision": null,
        "spec": null
    });
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &run_value,
        )
        .await
        .unwrap();
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
    let error = approve_plan_revision_for_command(
        &store,
        PlanApprovalRunContext {
            project_id: project.id,
            conversation_id: conversation.id,
            run_id,
            model_profile_id,
        },
        Some(legacy_hash.as_str()),
        legacy_plan,
        selection,
        &ApprovePlanV4Request {
            run_id,
            approval_hash: Some("wrong-hash".into()),
            plan_hash: None,
            revision: None,
        },
        &run_value,
    )
    .await
    .unwrap_err();
    assert!(error.contains("approval hash"), "{error}");
    assert!(
        store
            .proposed_plan_revisions_v4(project.id, conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !store
            .is_conversation_locked_v4(project.id, conversation.id)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Agent
    );
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "awaiting_approval"
    );
}

#[tokio::test]
async fn submit_message_service_keeps_title_and_message_atomic_under_plan_lock() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "message service project",
        r"C:\data\message-service",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "original", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "awaiting_approval",
            &json!({"status":"awaiting_approval"}),
        )
        .await
        .unwrap();
    let locked_plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "lock message service".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            locked_plan.clone(),
            "# lock message service".into(),
            locked_plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let message = Message::markdown(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        1,
        MessageRole::User,
        "title must not persist",
        Utc::now(),
    );
    let error = persist_submitted_message(&store, &message)
        .await
        .unwrap_err();
    assert!(error.contains("locked"), "{error}");
    assert!(
        store
            .messages_for_conversation(conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    let stored = store
        .conversations_for_project(project.id)
        .await
        .unwrap()
        .into_iter()
        .find(|value| value.id == conversation.id)
        .unwrap();
    assert_eq!(stored.title, "original");
}

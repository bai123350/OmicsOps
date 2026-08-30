use chrono::Utc;
use omicsops_core::workspace::{Conversation, Message, MessageRole, Project, ProjectTemplate};
use omicsops_dto::{PlanRevisionStatusV4, ProposedPlanRevisionV4, SessionAgentModeV4};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ExecutionPlanV4, PlanApprovalScopeV4, RunModeV4, RunSpecV4,
    ToolApprovalDecisionV4, ToolApprovalRequestV4, ToolCallV4, ToolEffectV4,
};
use omicsops_store::{ApprovalOptionsV4, Store};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::BTreeSet;
use uuid::Uuid;

struct Fixture {
    store: Store,
    project: Project,
    conversation: Conversation,
    other_conversation: Conversation,
}

async fn fixture() -> Fixture {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "plan revisions",
        r"C:\data\plan-revisions",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "planning conversation",
        Utc::now(),
    );
    let other_conversation =
        Conversation::new(Uuid::new_v4(), project.id, "other conversation", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    store.save_conversation(&other_conversation).await.unwrap();
    Fixture {
        store,
        project,
        conversation,
        other_conversation,
    }
}

fn plan(objective: &str) -> ExecutionPlanV4 {
    ExecutionPlanV4 {
        schema_version: 4,
        objective: objective.into(),
        steps: vec!["inspect".into(), "verify".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    }
}

async fn save_run(fixture: &Fixture, run_id: Uuid) {
    fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "awaiting_approval",
            &json!({
                "run_id": run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "plan",
                "status": "awaiting_approval",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "spec": null
            }),
        )
        .await
        .unwrap();
}

async fn save_revision(
    fixture: &Fixture,
    run_id: Uuid,
    revision: u64,
    plan: &ExecutionPlanV4,
    status: PlanRevisionStatusV4,
) -> ProposedPlanRevisionV4 {
    fixture
        .store
        .create_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            revision,
            plan.clone(),
            format!("# {}", plan.objective),
            plan.canonical_hash().unwrap(),
            status,
            None,
            Utc::now(),
        )
        .await
        .unwrap()
}

async fn append_plan_approval_request(
    fixture: &Fixture,
    generation: &ProposedPlanRevisionV4,
) -> (PlanApprovalScopeV4, ToolCallV4, ToolApprovalRequestV4) {
    let run_id = generation.run_id;
    let scope = PlanApprovalScopeV4 {
        project_id: generation.project_id,
        conversation_id: generation.conversation_id,
        run_id,
        revision_id: generation.id,
        revision: generation.revision,
    };
    let call = ToolCallV4 {
        call_id: "plan-mcp-call".into(),
        tool_id: "use_mcp_tool".into(),
        arguments: json!({
            "server_id": Uuid::new_v4(),
            "tool": "search",
            "arguments": {"q": "x"},
            "catalog_sha256": "catalog",
            "schema_sha256": "schema"
        }),
    };
    let request = ToolApprovalRequestV4::new_with_scope(
        run_id,
        &scope.hash(),
        call.clone(),
        ToolEffectV4::ReadOnly,
        "third-party readOnlyHint is an unverified hint trusted by the user, not a Host guarantee",
    )
    .unwrap();
    let seed = AgentEventV4::first(
        run_id,
        generation.project_id,
        generation.conversation_id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    fixture.store.append_agent_event_v4(&seed).await.unwrap();
    let requested = AgentEventV4::next(
        &seed,
        Utc::now(),
        AgentEventKindV4::ToolRequested { call: call.clone() },
    );
    fixture
        .store
        .append_agent_event_v4(&requested)
        .await
        .unwrap();
    let approval = AgentEventV4::next(
        &requested,
        Utc::now(),
        AgentEventKindV4::ToolApprovalRequested {
            request: request.clone(),
        },
    );
    fixture
        .store
        .append_agent_event_v4(&approval)
        .await
        .unwrap();
    (scope, call, request)
}

fn assert_send<T: Send>(_: T) {}

#[tokio::test]
async fn finalize_plan_revision_future_is_send() {
    let fixture = fixture().await;
    let proposal = plan("send regression");
    let future = fixture.store.finalize_plan_revision_v4(
        fixture.project.id,
        fixture.conversation.id,
        Uuid::new_v4(),
        1,
        proposal.clone(),
        "# send regression".into(),
        proposal.canonical_hash().unwrap(),
        Utc::now(),
    );
    assert_send(future);
}

#[tokio::test]
async fn revisions_are_immutable_and_round_trip_all_fields() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let original_plan = plan("original");
    let original = save_revision(
        &fixture,
        run_id,
        1,
        &original_plan,
        PlanRevisionStatusV4::Pending,
    )
    .await;

    let stored = fixture
        .store
        .proposed_plan_revision_v4(original.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored, original);
    assert_eq!(stored.run_id, run_id);
    assert_eq!(stored.project_id, fixture.project.id);
    assert_eq!(stored.conversation_id, fixture.conversation.id);
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.plan_hash, original_plan.canonical_hash().unwrap());
    assert_eq!(stored.markdown, "# original");
    assert_eq!(stored.status, PlanRevisionStatusV4::Pending);

    let error = fixture
        .store
        .update_proposed_plan_revision_v4(
            original.id,
            "# tampered".into(),
            PlanRevisionStatusV4::Approved,
            Some("tamper".into()),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("immutable"));
    let direct_sql_error =
        sqlx::query("UPDATE proposed_plans SET markdown='direct tamper' WHERE id=?1")
            .bind(original.id.to_string())
            .execute(fixture.store.pool())
            .await
            .unwrap_err();
    assert!(direct_sql_error.to_string().contains("immutable"));
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(original.id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
}

#[tokio::test]
async fn lock_is_conversation_scoped_and_allows_plan_actions() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let pending = save_revision(
        &fixture,
        run_id,
        1,
        &plan("locked"),
        PlanRevisionStatusV4::Pending,
    )
    .await;

    assert!(
        fixture
            .store
            .conversation_plan_lock_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );
    assert!(
        !fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.other_conversation.id)
            .await
            .unwrap()
    );
    assert!(
        fixture
            .store
            .ensure_plan_action_allowed_v4(fixture.project.id, fixture.conversation.id)
            .await
            .is_ok()
    );
    assert_eq!(pending.status, PlanRevisionStatusV4::Pending);
    let error = fixture
        .store
        .ensure_conversation_unlocked_v4(fixture.project.id, fixture.conversation.id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("locked"));
}

#[tokio::test]
async fn ordinary_message_write_is_atomically_rejected_only_for_locked_conversation() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let pending = save_revision(
        &fixture,
        run_id,
        1,
        &plan("message lock"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let locked_message = Message::markdown(
        Uuid::new_v4(),
        fixture.project.id,
        fixture.conversation.id,
        1,
        MessageRole::User,
        "blocked",
        Utc::now(),
    );
    let error = fixture
        .store
        .save_message(&locked_message)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("locked"));
    let free_message = Message::markdown(
        Uuid::new_v4(),
        fixture.project.id,
        fixture.other_conversation.id,
        1,
        MessageRole::User,
        "allowed",
        Utc::now(),
    );
    fixture.store.save_message(&free_message).await.unwrap();
    assert_eq!(pending.status, PlanRevisionStatusV4::Pending);
}

#[tokio::test]
async fn request_changes_records_feedback_and_next_revision_preserves_history() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let first_plan = plan("first");
    let first = save_revision(
        &fixture,
        run_id,
        1,
        &first_plan,
        PlanRevisionStatusV4::Pending,
    )
    .await;
    fixture
        .store
        .request_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &first.plan_hash,
            "add a QC step",
        )
        .await
        .unwrap();
    let revised = save_revision(
        &fixture,
        run_id,
        2,
        &plan("revised"),
        PlanRevisionStatusV4::Pending,
    )
    .await;

    let history = fixture
        .store
        .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, first.id);
    assert_eq!(history[0].plan.objective, "first");
    assert_eq!(history[0].feedback.as_deref(), Some("add a QC step"));
    assert_eq!(history[0].status, PlanRevisionStatusV4::Revising);
    assert_eq!(history[1], revised);
    assert_eq!(
        fixture
            .store
            .latest_pending_plan_revision_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .unwrap()
            .revision,
        2
    );
}

#[tokio::test]
async fn stale_revision_or_hash_cannot_be_approved() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let first = save_revision(
        &fixture,
        run_id,
        1,
        &plan("first"),
        PlanRevisionStatusV4::Superseded,
    )
    .await;
    let latest = save_revision(
        &fixture,
        run_id,
        2,
        &plan("latest"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let spec = minimal_spec(&fixture, run_id, &latest.plan);
    let run_value = json!({"status":"awaiting_approval"});
    let stale = fixture
        .store
        .approve_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            first.revision,
            &first.plan_hash,
            &spec,
            &run_value,
        )
        .await
        .unwrap_err();
    assert!(stale.to_string().contains("latest") || stale.to_string().contains("pending"));
    let hash = fixture
        .store
        .approve_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            latest.revision,
            "wrong-hash",
            &spec,
            &run_value,
        )
        .await
        .unwrap_err();
    assert!(hash.to_string().contains("hash"));
}

#[tokio::test]
async fn approval_is_atomic_and_switches_mode_only_after_commit() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let latest = save_revision(
        &fixture,
        run_id,
        1,
        &plan("approve"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let spec = minimal_spec(&fixture, run_id, &latest.plan);
    let run_value = json!({"status":"awaiting_approval"});

    let error = fixture
        .store
        .approve_plan_revision_v4_with_options(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            latest.revision,
            &latest.plan_hash,
            &spec,
            &run_value,
            ApprovalOptionsV4 {
                fail_after_step: Some(2),
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("injected"));
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(latest.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Pending
    );
    assert_eq!(
        fixture
            .store
            .get_conversation_agent_mode(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        omicsops_dto::SessionAgentModeV4::Plan
    );
    assert_eq!(
        fixture.store.agent_events_v4(run_id).await.unwrap().len(),
        0
    );

    let result = fixture
        .store
        .approve_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            latest.revision,
            &latest.plan_hash,
            &spec,
            &run_value,
        )
        .await
        .unwrap();
    assert_eq!(result.revision, 1);
    assert_eq!(
        fixture
            .store
            .get_conversation_agent_mode(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        omicsops_dto::SessionAgentModeV4::Agent
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(latest.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Approved
    );
    assert_eq!(
        fixture
            .store
            .agent_events_v4(run_id)
            .await
            .unwrap()
            .iter()
            .filter(|event| matches!(
                event.event,
                AgentEventKindV4::PlanApproved { .. }
                    | AgentEventKindV4::RunSpecFrozen { .. }
                    | AgentEventKindV4::ModeChanged { .. }
            ))
            .count(),
        3
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "running"
    );
}

#[tokio::test]
async fn cancel_keeps_plan_mode_and_unlocks_conversation() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let latest = save_revision(
        &fixture,
        run_id,
        1,
        &plan("cancel"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let result = fixture
        .store
        .cancel_plan_v4(fixture.project.id, fixture.conversation.id, run_id)
        .await
        .unwrap();
    assert_eq!(result.revision, latest.revision);
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(latest.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Cancelled
    );
    assert!(
        !fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );
    assert_eq!(
        fixture
            .store
            .get_conversation_agent_mode(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        omicsops_dto::SessionAgentModeV4::Plan
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
}

async fn insert_stale_active_revision(
    fixture: &Fixture,
    run_id: Uuid,
    revision: u64,
    status: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let stale_plan = plan("stale active revision");
    sqlx::query(
        "INSERT INTO proposed_plans
         (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,?9,?10,?10)",
    )
    .bind(id.to_string())
    .bind(fixture.project.id.to_string())
    .bind(fixture.conversation.id.to_string())
    .bind(i64::try_from(revision).unwrap())
    .bind(stale_plan.canonical_hash().unwrap())
    .bind(status)
    .bind(serde_json::to_string(&stale_plan).unwrap())
    .bind("# stale active revision")
    .bind(run_id.to_string())
    .bind(Utc::now().timestamp_millis())
    .execute(fixture.store.pool())
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn approving_newest_revision_supersedes_stale_active_rows_and_unlocks() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let latest = save_revision(
        &fixture,
        run_id,
        2,
        &plan("newest approval"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let stale_id = insert_stale_active_revision(&fixture, run_id, 1, "revising").await;
    assert!(
        fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );

    let spec = minimal_spec(&fixture, run_id, &latest.plan);
    fixture
        .store
        .approve_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            latest.revision,
            &latest.plan_hash,
            &spec,
            &json!({"status":"awaiting_approval"}),
        )
        .await
        .unwrap();

    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(stale_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Superseded
    );
    assert!(
        !fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );
    assert_eq!(
        fixture
            .store
            .get_conversation_agent_mode(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Agent
    );
}

#[tokio::test]
async fn cancelling_newest_revision_supersedes_stale_active_rows_and_unlocks() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let stale_run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    save_run(&fixture, stale_run_id).await;
    let latest = save_revision(
        &fixture,
        run_id,
        2,
        &plan("newest cancellation"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let stale_id = insert_stale_active_revision(&fixture, stale_run_id, 1, "generating").await;

    fixture
        .store
        .cancel_plan_v4(fixture.project.id, fixture.conversation.id, run_id)
        .await
        .unwrap();

    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(latest.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Cancelled
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(stale_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Superseded
    );
    assert!(
        !fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );
    assert_eq!(
        fixture
            .store
            .get_conversation_agent_mode(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Plan
    );
}

#[tokio::test]
async fn non_object_run_json_cannot_partially_finalize_or_cancel_a_plan() {
    let fixture = fixture().await;
    let rejected_run = Uuid::new_v4();
    let start_error = fixture
        .store
        .start_plan_run_v4(
            rejected_run,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!("not an object"),
            "reject malformed run JSON",
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(start_error.to_string().contains("object"), "{start_error}");
    assert!(
        fixture
            .store
            .agent_run_v4(rejected_run)
            .await
            .unwrap()
            .is_none()
    );

    let run_id = Uuid::new_v4();
    let generation = fixture
        .store
        .start_plan_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "status": "planning"
            }),
            "corrupt after start",
            Utc::now(),
        )
        .await
        .unwrap();
    sqlx::query("UPDATE agent_runs_v4 SET value_json=?1 WHERE run_id=?2")
        .bind(json!("corrupted run JSON").to_string())
        .bind(run_id.to_string())
        .execute(fixture.store.pool())
        .await
        .unwrap();
    let proposal = plan("must not finalize");
    let finalize_error = fixture
        .store
        .finalize_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generation.revision,
            proposal.clone(),
            "# must not finalize".into(),
            proposal.canonical_hash().unwrap(),
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(
        finalize_error.to_string().contains("object"),
        "{finalize_error}"
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(generation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Generating
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap(),
        json!("corrupted run JSON")
    );

    let cancel_error = fixture
        .store
        .cancel_plan_v4(fixture.project.id, fixture.conversation.id, run_id)
        .await
        .unwrap_err();
    assert!(
        cancel_error.to_string().contains("object"),
        "{cancel_error}"
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(generation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Generating
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap(),
        json!("corrupted run JSON")
    );
}

#[tokio::test]
async fn revisions_survive_store_restart_without_rewriting_the_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plan-revisions.sqlite3");
    let project = Project::new(
        Uuid::new_v4(),
        "restart project",
        r"C:\data\restart-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "restart conversation",
        Utc::now(),
    );
    let run_id = Uuid::new_v4();
    let store = Store::open(&path).await.unwrap();
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
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
    let revision = store
        .create_proposed_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            1,
            plan("restart"),
            "# restart".into(),
            plan("restart").canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            Some("keep provenance".into()),
            Utc::now(),
        )
        .await
        .unwrap();
    drop(store);
    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .proposed_plan_revisions_v4(project.id, conversation.id)
            .await
            .unwrap(),
        vec![revision]
    );
    assert!(
        reopened
            .is_conversation_locked_v4(project.id, conversation.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn acquire_and_finalize_plan_generation_use_one_revision_and_lock_atomically() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;

    let generating = fixture
        .store
        .acquire_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "generate a plan",
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(generating.revision, 1);
    assert_eq!(generating.status, PlanRevisionStatusV4::Generating);
    assert!(
        fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );

    let finalized_plan = plan("final plan");
    let finalized = fixture
        .store
        .finalize_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generating.revision,
            finalized_plan.clone(),
            "# final plan".into(),
            finalized_plan.canonical_hash().unwrap(),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(finalized.id, generating.id);
    assert_eq!(finalized.revision, generating.revision);
    assert_eq!(finalized.status, PlanRevisionStatusV4::Pending);
    assert_eq!(finalized.plan, finalized_plan);
    assert!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"] == "awaiting_approval"
    );
}

#[tokio::test]
async fn concurrent_generation_acquire_allows_only_one_conversation_owner() {
    let fixture = fixture().await;
    let first_run = Uuid::new_v4();
    let second_run = Uuid::new_v4();
    save_run(&fixture, first_run).await;
    save_run(&fixture, second_run).await;

    let first = fixture.store.acquire_plan_revision_v4(
        fixture.project.id,
        fixture.conversation.id,
        first_run,
        "first generation",
        Utc::now(),
    );
    let second = fixture.store.acquire_plan_revision_v4(
        fixture.project.id,
        fixture.conversation.id,
        second_run,
        "second generation",
        Utc::now(),
    );
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.is_ok() as u8 + second.is_ok() as u8, 1);
    let error = first.err().or_else(|| second.err()).unwrap();
    assert!(error.to_string().contains("active plan") || error.to_string().contains("locked"));
}

#[tokio::test]
async fn plan_start_transaction_writes_run_and_generation_lock_together() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let run_value = json!({
        "run_id": run_id,
        "project_id": fixture.project.id,
        "conversation_id": fixture.conversation.id,
        "model_profile_id": Uuid::new_v4(),
        "objective": "atomic start",
        "status": "planning",
        "plan": null,
        "plan_hash": null,
        "compute_selection": null,
        "approval_hash": null,
        "plan_revision": null,
        "spec": null
    });
    let generation = fixture
        .store
        .start_plan_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &run_value,
            "atomic start",
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(generation.status, PlanRevisionStatusV4::Generating);
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "planning"
    );

    let second_run = Uuid::new_v4();
    let second_error = fixture
        .store
        .start_plan_run_v4(
            second_run,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({"status":"planning"}),
            "atomic start",
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(second_error.to_string().contains("active plan"));
    assert_eq!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        fixture
            .store
            .agent_run_v4(second_run)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn plan_tool_approval_pause_and_resume_keep_the_same_revision_scope() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let generation = fixture
        .store
        .start_plan_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "approval pause",
                "status": "planning",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
            "approval pause",
            Utc::now(),
        )
        .await
        .unwrap();

    let (scope, call, approval) = append_plan_approval_request(&fixture, &generation).await;
    let paused = fixture
        .store
        .pause_plan_generation_for_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generation.revision,
            Some("waiting for exact MCP approval"),
        )
        .await
        .unwrap();
    assert_eq!(paused.id, generation.id);
    assert_eq!(paused.revision, generation.revision);
    assert_eq!(paused.status, PlanRevisionStatusV4::Revising);
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "waiting_for_approval"
    );
    fixture
        .store
        .decide_tool_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &approval.approval_id,
            &call.canonical_hash().unwrap(),
            ToolApprovalDecisionV4::Approved,
            &scope.hash(),
            RunModeV4::Plan,
            Some(&scope.hash()),
        )
        .await
        .unwrap();

    let resumed = fixture
        .store
        .resume_plan_generation_after_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            paused.id,
            paused.revision,
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(resumed.id, generation.id);
    assert_eq!(resumed.revision, generation.revision);
    assert_eq!(resumed.status, PlanRevisionStatusV4::Generating);
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "planning"
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn plan_tool_decision_is_single_writer_and_decision_before_pause_can_resume() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let generation = fixture
        .store
        .start_plan_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "decision race",
                "status": "planning",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
            "decision race",
            Utc::now(),
        )
        .await
        .unwrap();
    let (scope, call, approval) = append_plan_approval_request(&fixture, &generation).await;
    let call_hash = call.canonical_hash().unwrap();
    let scope_hash = scope.hash();
    let first = fixture.store.decide_tool_approval_v4(
        fixture.project.id,
        fixture.conversation.id,
        run_id,
        &approval.approval_id,
        &call_hash,
        ToolApprovalDecisionV4::Approved,
        &scope_hash,
        RunModeV4::Plan,
        Some(&scope_hash),
    );
    let second = fixture.store.decide_tool_approval_v4(
        fixture.project.id,
        fixture.conversation.id,
        run_id,
        &approval.approval_id,
        &call_hash,
        ToolApprovalDecisionV4::Approved,
        &scope_hash,
        RunModeV4::Plan,
        Some(&scope_hash),
    );
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.is_ok() as u8 + second.is_ok() as u8, 1);
    let pause = fixture
        .store
        .pause_plan_generation_for_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generation.revision,
            Some("already decided"),
        )
        .await
        .unwrap();
    assert_eq!(pause.id, generation.id);
    assert_eq!(pause.status, PlanRevisionStatusV4::Revising);
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "waiting_for_approval"
    );
    let resumed = fixture
        .store
        .resume_plan_generation_after_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generation.id,
            generation.revision,
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(resumed.id, generation.id);
    assert_eq!(resumed.status, PlanRevisionStatusV4::Generating);
    let events = fixture.store.agent_events_v4(run_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.event, AgentEventKindV4::ToolApprovalDecided { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn plan_approval_resume_rejects_an_undecided_request_without_changing_state() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let generation = fixture
        .store
        .start_plan_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "undecided resume",
                "status": "planning",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
            "undecided resume",
            Utc::now(),
        )
        .await
        .unwrap();
    append_plan_approval_request(&fixture, &generation).await;
    let paused = fixture
        .store
        .pause_plan_generation_for_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generation.revision,
            Some("waiting"),
        )
        .await
        .unwrap();
    let error = fixture
        .store
        .resume_plan_generation_after_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            paused.id,
            paused.revision,
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("decided"));
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(paused.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Revising
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "waiting_for_approval"
    );
}

#[tokio::test]
async fn plan_tool_decision_rejects_an_old_revision_scope() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let first = fixture
        .store
        .start_plan_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({
                "run_id": run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "old scope",
                "status": "planning",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": null
            }),
            "old scope",
            Utc::now(),
        )
        .await
        .unwrap();
    let (old_scope, call, approval) = append_plan_approval_request(&fixture, &first).await;
    fixture
        .store
        .pause_plan_generation_for_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            first.revision,
            Some("request changes"),
        )
        .await
        .unwrap();
    let old_call_hash = call.canonical_hash().unwrap();
    let old_scope_hash = old_scope.hash();
    fixture
        .store
        .decide_tool_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &approval.approval_id,
            &old_call_hash,
            ToolApprovalDecisionV4::Approved,
            &old_scope_hash,
            RunModeV4::Plan,
            Some(&old_scope_hash),
        )
        .await
        .unwrap();
    fixture
        .store
        .resume_plan_generation_after_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            first.id,
            first.revision,
            Utc::now(),
        )
        .await
        .unwrap();
    let second = fixture
        .store
        .create_next_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            plan("new scope"),
            "# new scope".into(),
            plan("new scope").canonical_hash().unwrap(),
            PlanRevisionStatusV4::Generating,
            None,
            Utc::now(),
        )
        .await;
    assert!(second.is_err());
    // The old approval is invalid as soon as its revision is no longer the
    // latest one. Materialize a new revision through the normal user-change
    // transition and then check the low-level decision gate.
    fixture
        .store
        .terminate_plan_generation_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            first.revision,
            PlanRevisionStatusV4::Revising,
            Some("new revision"),
        )
        .await
        .unwrap();
    let next = fixture
        .store
        .acquire_plan_revision_resume_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "new revision",
            Utc::now(),
        )
        .await
        .unwrap();
    let error = fixture
        .store
        .decide_tool_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &approval.approval_id,
            &old_call_hash,
            ToolApprovalDecisionV4::Approved,
            &old_scope_hash,
            RunModeV4::Plan,
            Some(&old_scope_hash),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("scope") || error.to_string().contains("current"));
    assert_eq!(next.revision, 2);
}

#[tokio::test]
async fn execute_approval_store_gate_preserves_legacy_hash_and_rejects_plan_binding() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let spec_hash = "legacy-spec";
    fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "awaiting_approval",
            &json!({
                "run_id": run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "plan",
                "status": "awaiting_approval",
                "plan": null,
                "plan_hash": null,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": null,
                "spec": {"spec_hash": spec_hash}
            }),
        )
        .await
        .unwrap();
    let seed = AgentEventV4::first(
        run_id,
        fixture.project.id,
        fixture.conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    fixture.store.append_agent_event_v4(&seed).await.unwrap();
    let call = ToolCallV4 {
        call_id: "execute-approval".into(),
        tool_id: "runtime.execute".into(),
        arguments: json!({"code":"print(1)"}),
    };
    let approval = ToolApprovalRequestV4::new(
        run_id,
        spec_hash,
        call.clone(),
        ToolEffectV4::Runtime,
        "legacy execute approval",
    )
    .unwrap();
    let requested = AgentEventV4::next(
        &seed,
        Utc::now(),
        AgentEventKindV4::ToolApprovalRequested {
            request: approval.clone(),
        },
    );
    fixture
        .store
        .append_agent_event_v4(&requested)
        .await
        .unwrap();
    let decided = fixture
        .store
        .decide_tool_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &approval.approval_id,
            &approval.call_hash,
            ToolApprovalDecisionV4::Approved,
            spec_hash,
            RunModeV4::Execute,
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        decided.event,
        AgentEventKindV4::ToolApprovalDecided {
            decision: ToolApprovalDecisionV4::Approved,
            ..
        }
    ));

    let plan_call = ToolCallV4 {
        call_id: "plan-approval-in-execute-run".into(),
        tool_id: "use_mcp_tool".into(),
        arguments: json!({
            "server_id": Uuid::new_v4(),
            "tool": "search",
            "catalog_sha256": "catalog",
            "schema_sha256": "schema",
            "arguments": {"q":"x"}
        }),
    };
    let plan_request = ToolApprovalRequestV4::new_with_scope(
        run_id,
        "plan-scope",
        plan_call,
        ToolEffectV4::ReadOnly,
        "plan approval",
    )
    .unwrap();
    let events = fixture.store.agent_events_v4(run_id).await.unwrap();
    let plan_event = AgentEventV4::next(
        events.last().unwrap(),
        Utc::now(),
        AgentEventKindV4::ToolApprovalRequested {
            request: plan_request.clone(),
        },
    );
    fixture
        .store
        .append_agent_event_v4(&plan_event)
        .await
        .unwrap();
    let wrong_mode = fixture
        .store
        .decide_tool_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &approval.approval_id,
            &approval.call_hash,
            ToolApprovalDecisionV4::Approved,
            "scope",
            RunModeV4::Plan,
            Some("scope"),
        )
        .await
        .unwrap_err();
    assert!(wrong_mode.to_string().contains("phase") || wrong_mode.to_string().contains("current"));
    let plan_as_execute = fixture
        .store
        .decide_tool_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &plan_request.approval_id,
            &plan_request.call_hash,
            ToolApprovalDecisionV4::Approved,
            spec_hash,
            RunModeV4::Execute,
            None,
        )
        .await
        .unwrap_err();
    assert!(plan_as_execute.to_string().contains("Plan binding"));
}

#[tokio::test]
async fn execute_approval_requires_the_frozen_spec_hash_in_run_json() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let seed = AgentEventV4::first(
        run_id,
        fixture.project.id,
        fixture.conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    fixture.store.append_agent_event_v4(&seed).await.unwrap();
    let call = ToolCallV4 {
        call_id: "missing-spec-approval".into(),
        tool_id: "runtime.execute".into(),
        arguments: json!({"code":"print(1)"}),
    };
    let approval = ToolApprovalRequestV4::new(
        run_id,
        "legacy-spec",
        call,
        ToolEffectV4::Runtime,
        "legacy execute approval",
    )
    .unwrap();
    fixture
        .store
        .append_agent_event_v4(&AgentEventV4::next(
            &seed,
            Utc::now(),
            AgentEventKindV4::ToolApprovalRequested {
                request: approval.clone(),
            },
        ))
        .await
        .unwrap();
    let error = fixture
        .store
        .decide_tool_approval_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &approval.approval_id,
            &approval.call_hash,
            ToolApprovalDecisionV4::Approved,
            "legacy-spec",
            RunModeV4::Execute,
            None,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("persisted frozen spec_hash"));
    assert_eq!(
        fixture.store.agent_events_v4(run_id).await.unwrap().len(),
        2
    );
}

#[tokio::test]
async fn request_revision_persists_feedback_and_event_as_one_valid_chain() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let proposal = save_revision(
        &fixture,
        run_id,
        1,
        &plan("request event"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let seed = AgentEventV4::first(
        run_id,
        fixture.project.id,
        fixture.conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    fixture.store.append_agent_event_v4(&seed).await.unwrap();

    let result = fixture
        .store
        .request_plan_revision_v4_with_event(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &proposal.plan_hash,
            "include controls",
        )
        .await
        .unwrap();
    assert_eq!(result.revision.status, PlanRevisionStatusV4::Revising);
    assert_eq!(result.event.previous_hash, seed.event_hash);
    result.event.verify().unwrap();
    let events = fixture.store.agent_events_v4(run_id).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1], result.event);
}

#[tokio::test]
async fn request_revision_rolls_back_feedback_when_event_append_fails() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let proposal = save_revision(
        &fixture,
        run_id,
        1,
        &plan("request rollback"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let error = fixture
        .store
        .request_plan_revision_v4_with_options(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &proposal.plan_hash,
            "must fail atomically",
            omicsops_store::PlanRevisionRequestOptionsV4 {
                fail_after_step: Some(1),
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("injected"));
    let unchanged = fixture
        .store
        .proposed_plan_revision_v4(proposal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.status, PlanRevisionStatusV4::Pending);
    assert_eq!(unchanged.feedback, None);
    assert!(
        fixture
            .store
            .agent_events_v4(run_id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn reopening_repairs_a_pre_release_immutable_trigger_and_remains_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old-trigger.sqlite3");
    let project = Project::new(
        Uuid::new_v4(),
        "old trigger project",
        r"C:\data\old-trigger",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "old trigger conversation",
        Utc::now(),
    );
    let run_id = Uuid::new_v4();
    let store = Store::open(&path).await.unwrap();
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    save_run(
        &Fixture {
            store: store.clone(),
            project: project.clone(),
            conversation: conversation.clone(),
            other_conversation: conversation.clone(),
        },
        run_id,
    )
    .await;
    drop(store);

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(false)
        .foreign_keys(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER trg_proposed_plans_immutable_content")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER trg_proposed_plans_immutable_content
         BEFORE UPDATE OF id,project_id,frame_id,revision,plan_hash,plan_json,markdown,run_id,created_at
         ON proposed_plans
         BEGIN SELECT RAISE(ABORT, 'old immutable trigger'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let reopened = Store::open(&path).await.unwrap();
    let trigger_sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type='trigger' AND name='trg_proposed_plans_immutable_content'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert!(trigger_sql.contains("OLD.status = 'generating'"));
    let generation = reopened
        .acquire_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            "repair old trigger",
            Utc::now(),
        )
        .await
        .unwrap();
    let final_plan = plan("repaired");
    let finalized = reopened
        .finalize_plan_revision_v4(
            project.id,
            conversation.id,
            run_id,
            generation.revision,
            final_plan.clone(),
            "# repaired".into(),
            final_plan.canonical_hash().unwrap(),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(finalized.status, PlanRevisionStatusV4::Pending);
    drop(reopened);
    let repeated = Store::open(&path).await.unwrap();
    assert_eq!(repeated.schema_version().await.unwrap(), 4);
    assert_eq!(
        repeated
            .proposed_plan_revision_v4(finalized.id)
            .await
            .unwrap()
            .unwrap(),
        finalized
    );
    assert!(repeated.foreign_keys_enabled().await.unwrap());
    let fk_violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(repeated.pool())
        .await
        .unwrap();
    assert!(fk_violations.is_empty());
}

#[tokio::test]
async fn reopening_a_pre_release_plan_table_without_run_id_fails_contextually() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing-plan-run-id.sqlite3");
    let store = Store::open(&path).await.unwrap();
    sqlx::query("DROP TRIGGER trg_proposed_plans_immutable_content")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DROP TABLE proposed_plans")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE proposed_plans (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            frame_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            plan_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            plan_json TEXT NOT NULL,
            markdown TEXT NOT NULL,
            feedback TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(store.pool())
    .await
    .unwrap();
    drop(store);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("pre-release table without run_id unexpectedly reopened"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(
        message.contains("proposed_plans") && message.contains("run_id"),
        "{message}"
    );
    assert!(!message.contains("no such column"), "{message}");

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(false);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let columns = sqlx::query("PRAGMA table_info(proposed_plans)")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        !columns
            .iter()
            .any(|row| row.get::<String, _>(1) == "run_id")
    );
}

#[tokio::test]
async fn failed_finalize_cancels_generation_and_writes_terminal_event() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let duplicate = plan("duplicate final hash");
    let old = save_revision(
        &fixture,
        run_id,
        1,
        &duplicate,
        PlanRevisionStatusV4::Cancelled,
    )
    .await;
    let generation = fixture
        .store
        .acquire_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "duplicate final hash",
            Utc::now(),
        )
        .await
        .unwrap();
    let error = fixture
        .store
        .finalize_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generation.revision,
            duplicate.clone(),
            "# duplicate".into(),
            old.plan_hash.clone(),
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("UNIQUE") || error.to_string().contains("unique"));
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(generation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Cancelled
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    let events = fixture.store.agent_events_v4(run_id).await.unwrap();
    assert!(matches!(
        events.last().map(|event| &event.event),
        Some(AgentEventKindV4::RunCancelled)
    ));
    assert!(
        !fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn terminal_run_rejects_post_cancel_events_but_accepts_exact_replay() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let generation = fixture
        .store
        .acquire_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "cancel event guard",
            Utc::now(),
        )
        .await
        .unwrap();
    let cancellation = fixture
        .store
        .cancel_plan_v4(fixture.project.id, fixture.conversation.id, run_id)
        .await
        .unwrap();
    let cancelled = cancellation.events[0].clone();
    fixture
        .store
        .append_agent_event_v4(&cancelled)
        .await
        .unwrap();
    let proposal = AgentEventV4::next(
        &cancelled,
        Utc::now(),
        AgentEventKindV4::PlanProposed {
            plan: plan("post cancel"),
            plan_hash: plan("post cancel").canonical_hash().unwrap(),
        },
    );
    let error = fixture
        .store
        .append_agent_event_v4(&proposal)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("terminal") || error.to_string().contains("cancel"));
    let events = fixture.store.agent_events_v4(run_id).await.unwrap();
    assert!(matches!(
        events.last().map(|event| &event.event),
        Some(AgentEventKindV4::RunCancelled)
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, AgentEventKindV4::PlanProposed { .. }))
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(generation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Cancelled
    );
}

#[tokio::test]
async fn low_level_next_revision_cannot_revive_terminal_run_without_latest_revision() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "cancelled",
            &json!({"status":"cancelled"}),
        )
        .await
        .unwrap();

    let proposal = plan("low-level revive");
    let error = fixture
        .store
        .create_next_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            proposal.clone(),
            "# low-level revive".into(),
            proposal.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("terminal") || error.to_string().contains("revising"));
    assert!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
}

#[tokio::test]
async fn concurrent_revision_requests_are_serialized_without_losing_the_chain() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let proposal = save_revision(
        &fixture,
        run_id,
        1,
        &plan("concurrent request"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let first = fixture.store.request_plan_revision_v4_with_event(
        fixture.project.id,
        fixture.conversation.id,
        run_id,
        &proposal.plan_hash,
        "first feedback",
    );
    let second = fixture.store.request_plan_revision_v4_with_event(
        fixture.project.id,
        fixture.conversation.id,
        run_id,
        &proposal.plan_hash,
        "second feedback",
    );
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.is_ok() as u8 + second.is_ok() as u8, 1);
    let events = fixture.store.agent_events_v4(run_id).await.unwrap();
    assert_eq!(events.len(), 1);
    events[0].verify().unwrap();
    let stored = fixture
        .store
        .proposed_plan_revision_v4(proposal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, PlanRevisionStatusV4::Revising);
    assert!(matches!(
        stored.feedback.as_deref(),
        Some("first feedback" | "second feedback")
    ));
}

#[tokio::test]
async fn generic_run_save_cannot_overwrite_an_active_plan_run() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let generation = fixture
        .store
        .acquire_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "generic save guard",
            Utc::now(),
        )
        .await
        .unwrap();
    let error = fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({"status":"planning", "stale":true}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("locked") || error.to_string().contains("active plan"));
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(generation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Generating
    );
}

#[tokio::test]
async fn generic_run_save_cannot_reassign_an_existing_run_owner() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let generating = fixture
        .store
        .acquire_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "owner guard",
            Utc::now(),
        )
        .await
        .unwrap();

    let other_project = Project::new(
        Uuid::new_v4(),
        "other owner project",
        r"C:\data\other-owner",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    fixture.store.save_project(&other_project).await.unwrap();
    let other_conversation = Conversation::new(
        Uuid::new_v4(),
        other_project.id,
        "other owner conversation",
        Utc::now(),
    );
    fixture
        .store
        .save_conversation(&other_conversation)
        .await
        .unwrap();

    let error = fixture
        .store
        .save_agent_run_v4(
            run_id,
            other_project.id,
            other_conversation.id,
            "running",
            &json!({"status":"running"}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("owner") || error.to_string().contains("context"));
    let owner = sqlx::query("SELECT project_id,conversation_id FROM agent_runs_v4 WHERE run_id=?1")
        .bind(run_id.to_string())
        .fetch_one(fixture.store.pool())
        .await
        .unwrap();
    assert_eq!(
        owner.try_get::<String, _>(0).unwrap(),
        fixture.project.id.to_string()
    );
    assert_eq!(
        owner.try_get::<String, _>(1).unwrap(),
        fixture.conversation.id.to_string()
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(generating.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Generating
    );
}

#[tokio::test]
async fn generating_to_pending_direct_sql_cannot_mutate_plan_content() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let generation = fixture
        .store
        .acquire_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "trigger content guard",
            Utc::now(),
        )
        .await
        .unwrap();
    let error =
        sqlx::query("UPDATE proposed_plans SET status='pending', markdown='tampered' WHERE id=?1")
            .bind(generation.id.to_string())
            .execute(fixture.store.pool())
            .await
            .unwrap_err();
    assert!(error.to_string().contains("immutable"));
    let unchanged = fixture
        .store
        .proposed_plan_revision_v4(generation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.status, PlanRevisionStatusV4::Generating);
    assert!(unchanged.markdown.is_empty());
}

#[tokio::test]
async fn start_plan_run_rejects_reuse_of_an_existing_terminal_run_id() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "cancelled",
            &json!({"status":"cancelled"}),
        )
        .await
        .unwrap();
    let error = fixture
        .store
        .start_plan_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "planning",
            &json!({"status":"planning"}),
            "reused run",
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("already exists") || error.to_string().contains("reuse"));
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    assert!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn create_plan_revision_rejects_terminal_runs_and_multiple_active_revisions() {
    let fixture = fixture().await;
    let terminal_run = Uuid::new_v4();
    save_run(&fixture, terminal_run).await;
    fixture
        .store
        .save_agent_run_v4(
            terminal_run,
            fixture.project.id,
            fixture.conversation.id,
            "cancelled",
            &json!({"status":"cancelled"}),
        )
        .await
        .unwrap();
    let terminal_plan = plan("terminal create");
    let terminal_error = fixture
        .store
        .create_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            terminal_run,
            1,
            terminal_plan.clone(),
            "# terminal create".into(),
            terminal_plan.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(terminal_error.to_string().contains("terminal"));

    let active_run = Uuid::new_v4();
    save_run(&fixture, active_run).await;
    let first = plan("first active");
    fixture
        .store
        .create_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            active_run,
            1,
            first.clone(),
            "# first active".into(),
            first.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Generating,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    let second = plan("second active");
    let active_error = fixture
        .store
        .create_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            active_run,
            2,
            second.clone(),
            "# second active".into(),
            second.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(
        active_error.to_string().contains("active") || active_error.to_string().contains("locked")
    );
    assert_eq!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .iter()
            .filter(|revision| revision.run_id == active_run)
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_acquire_rejects_a_terminal_run_even_with_a_revising_latest_revision() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let proposal = save_revision(
        &fixture,
        run_id,
        1,
        &plan("terminal resume"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    fixture
        .store
        .request_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &proposal.plan_hash,
            "terminal before resume",
        )
        .await
        .unwrap();
    sqlx::query("UPDATE agent_runs_v4 SET status='cancelled',value_json=?1 WHERE run_id=?2")
        .bind(json!({"status":"cancelled"}).to_string())
        .bind(run_id.to_string())
        .execute(fixture.store.pool())
        .await
        .unwrap();

    let error = fixture
        .store
        .acquire_plan_revision_resume_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "terminal resume",
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("terminal"));
    assert_eq!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn repeated_plan_cancel_is_idempotent_and_keeps_one_terminal_event() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let generation = fixture
        .store
        .acquire_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "repeat cancel",
            Utc::now(),
        )
        .await
        .unwrap();
    let first = fixture
        .store
        .cancel_plan_v4(fixture.project.id, fixture.conversation.id, run_id)
        .await
        .unwrap();
    let second = fixture
        .store
        .cancel_plan_v4(fixture.project.id, fixture.conversation.id, run_id)
        .await
        .unwrap();
    assert_eq!(first.revision, generation.revision);
    assert_eq!(second.revision, generation.revision);
    assert_eq!(second.events.len(), 0);
    assert_eq!(
        fixture.store.agent_events_v4(run_id).await.unwrap().len(),
        1
    );
    assert!(matches!(
        fixture
            .store
            .agent_events_v4(run_id)
            .await
            .unwrap()
            .last()
            .map(|event| &event.event),
        Some(AgentEventKindV4::RunCancelled)
    ));
    assert!(
        !fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn request_revision_rejects_terminal_run_without_mutating_revision_or_events() {
    for terminal_status in ["completed", "cancelled", "failed", "needs_attention"] {
        let fixture = fixture().await;
        let run_id = Uuid::new_v4();
        save_run(&fixture, run_id).await;
        let proposal = save_revision(
            &fixture,
            run_id,
            1,
            &plan("terminal request guard"),
            PlanRevisionStatusV4::Pending,
        )
        .await;

        sqlx::query("UPDATE agent_runs_v4 SET status=?1,value_json=?2 WHERE run_id=?3")
            .bind(terminal_status)
            .bind(json!({"status": terminal_status}).to_string())
            .bind(run_id.to_string())
            .execute(fixture.store.pool())
            .await
            .unwrap();

        let error = fixture
            .store
            .request_plan_revision_v4_with_options(
                fixture.project.id,
                fixture.conversation.id,
                run_id,
                &proposal.plan_hash,
                "must not revive terminal run",
                omicsops_store::PlanRevisionRequestOptionsV4::default(),
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("terminal"),
            "{terminal_status}: {error}"
        );

        let unchanged = fixture
            .store
            .proposed_plan_revision_v4(proposal.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.status, PlanRevisionStatusV4::Pending);
        assert_eq!(unchanged.feedback, None);
        assert!(
            fixture
                .store
                .agent_events_v4(run_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
            terminal_status
        );
    }
}

#[tokio::test]
async fn approval_requires_awaiting_approval_run_and_rejects_terminal_or_other_statuses() {
    for run_status in [
        "planning",
        "running",
        "completed",
        "cancelled",
        "failed",
        "needs_attention",
    ] {
        let fixture = fixture().await;
        let run_id = Uuid::new_v4();
        save_run(&fixture, run_id).await;
        let proposal = save_revision(
            &fixture,
            run_id,
            1,
            &plan("approval status guard"),
            PlanRevisionStatusV4::Pending,
        )
        .await;
        let spec = minimal_spec(&fixture, run_id, &proposal.plan);
        sqlx::query("UPDATE agent_runs_v4 SET status=?1,value_json=?2 WHERE run_id=?3")
            .bind(run_status)
            .bind(json!({"status": run_status}).to_string())
            .bind(run_id.to_string())
            .execute(fixture.store.pool())
            .await
            .unwrap();

        let error = fixture
            .store
            .approve_plan_revision_v4(
                fixture.project.id,
                fixture.conversation.id,
                run_id,
                proposal.revision,
                &proposal.plan_hash,
                &spec,
                &json!({"status":"awaiting_approval"}),
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("awaiting") || error.to_string().contains("terminal"),
            "{run_status}: {error}"
        );
        assert_eq!(
            fixture
                .store
                .proposed_plan_revision_v4(proposal.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            PlanRevisionStatusV4::Pending
        );
        assert!(
            fixture
                .store
                .agent_events_v4(run_id)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn approval_rejects_non_object_run_value_without_partial_persistence() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let proposal = save_revision(
        &fixture,
        run_id,
        1,
        &plan("object run value guard"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    let spec = minimal_spec(&fixture, run_id, &proposal.plan);

    let error = fixture
        .store
        .approve_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            proposal.revision,
            &proposal.plan_hash,
            &spec,
            &json!("run value must be an object"),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("object"), "{error}");
    assert_eq!(
        fixture
            .store
            .proposed_plan_revision_v4(proposal.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanRevisionStatusV4::Pending
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "awaiting_approval"
    );
    assert!(
        fixture
            .store
            .agent_events_v4(run_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .store
            .get_conversation_agent_mode(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        omicsops_dto::SessionAgentModeV4::Plan
    );
}

#[tokio::test]
async fn first_message_title_and_message_are_unchanged_when_plan_lock_wins() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let proposal = save_revision(
        &fixture,
        run_id,
        1,
        &plan("message lock race"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    assert_eq!(proposal.status, PlanRevisionStatusV4::Pending);

    let message = Message::markdown(
        Uuid::new_v4(),
        fixture.project.id,
        fixture.conversation.id,
        1,
        MessageRole::User,
        "first message must not leak",
        Utc::now(),
    );
    let error = fixture
        .store
        .save_message_with_first_title(&message, "first message must not leak")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("locked"), "{error}");
    assert!(
        fixture
            .store
            .messages_for_conversation(fixture.conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    let conversation = fixture
        .store
        .conversations_for_project(fixture.project.id)
        .await
        .unwrap()
        .into_iter()
        .find(|conversation| conversation.id == fixture.conversation.id)
        .unwrap();
    assert_eq!(conversation.title, fixture.conversation.title);
}

#[tokio::test]
async fn first_message_title_rolls_back_when_message_write_fails() {
    let fixture = fixture().await;
    let message = Message::markdown(
        Uuid::new_v4(),
        fixture.project.id,
        fixture.conversation.id,
        u64::MAX,
        MessageRole::User,
        "invalid sequence must not leak title",
        Utc::now(),
    );
    let error = fixture
        .store
        .save_message_with_first_title(&message, "invalid sequence must not leak title")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("sequence"), "{error}");
    assert!(
        fixture
            .store
            .messages_for_conversation(fixture.conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    let conversation = fixture
        .store
        .conversations_for_project(fixture.project.id)
        .await
        .unwrap()
        .into_iter()
        .find(|conversation| conversation.id == fixture.conversation.id)
        .unwrap();
    assert_eq!(conversation.title, fixture.conversation.title);
}

#[tokio::test]
async fn legacy_approval_rolls_back_materialization_and_mode_on_injected_failure() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let model_profile_id = Uuid::new_v4();
    let legacy_plan = plan("legacy atomic failure");
    let legacy_hash = legacy_plan.canonical_hash().unwrap();
    let run_value = json!({
        "run_id": run_id,
        "project_id": fixture.project.id,
        "conversation_id": fixture.conversation.id,
        "model_profile_id": model_profile_id,
        "objective": "legacy atomic failure",
        "status": "awaiting_approval",
        "plan": legacy_plan.clone(),
        "plan_hash": legacy_hash,
        "compute_selection": null,
        "approval_hash": null,
        "plan_revision": null,
        "spec": null
    });
    fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "awaiting_approval",
            &run_value,
        )
        .await
        .unwrap();
    let spec = minimal_spec(&fixture, run_id, &legacy_plan);
    let error = fixture
        .store
        .approve_legacy_plan_revision_v4_with_options(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            legacy_plan,
            "# legacy atomic failure".into(),
            run_value["plan_hash"].as_str().unwrap(),
            &spec,
            &run_value,
            ApprovalOptionsV4 {
                fail_after_step: Some(1),
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("injected"), "{error}");
    assert!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .store
            .get_conversation_agent_mode(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        omicsops_dto::SessionAgentModeV4::Agent
    );
    assert!(
        !fixture
            .store
            .is_conversation_locked_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
    );
    assert_eq!(
        fixture.store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "awaiting_approval"
    );
}

#[tokio::test]
async fn fresh_proposed_plan_schema_requires_run_and_agent_run_foreign_key() {
    let fixture = fixture().await;
    let columns = sqlx::query("PRAGMA table_info(proposed_plans)")
        .fetch_all(fixture.store.pool())
        .await
        .unwrap();
    let run_column = columns
        .iter()
        .find(|row| row.try_get::<String, _>(1).unwrap() == "run_id")
        .expect("proposed_plans.run_id schema column");
    assert_eq!(run_column.try_get::<i64, _>(3).unwrap(), 1);
    let foreign_keys = sqlx::query("PRAGMA foreign_key_list(proposed_plans)")
        .fetch_all(fixture.store.pool())
        .await
        .unwrap();
    assert!(foreign_keys.iter().any(|row| {
        row.try_get::<String, _>(2).unwrap() == "agent_runs_v4"
            && row.try_get::<String, _>(3).unwrap() == "run_id"
            && row.try_get::<String, _>(4).unwrap() == "run_id"
    }));
}

#[tokio::test]
async fn terminal_middle_valid_hash_chain_is_rejected_on_load() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let first = AgentEventV4::first(
        run_id,
        fixture.project.id,
        fixture.conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    let terminal = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::RunCancelled);
    let after = AgentEventV4::next(
        &terminal,
        Utc::now(),
        AgentEventKindV4::ModelText {
            text: "post-terminal".into(),
        },
    );
    for event in [&first, &terminal, &after] {
        sqlx::query(
            "INSERT INTO agent_events_v4
             (run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json,occurred_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        )
        .bind(event.run_id.to_string())
        .bind(event.project_id.to_string())
        .bind(event.conversation_id.to_string())
        .bind(i64::try_from(event.sequence).unwrap())
        .bind(&event.previous_hash)
        .bind(&event.event_hash)
        .bind(serde_json::to_string(event).unwrap())
        .bind(event.occurred_at.timestamp_millis())
        .execute(fixture.store.pool())
        .await
        .unwrap();
    }
    let error = fixture.store.agent_events_v4(run_id).await.unwrap_err();
    assert!(error.to_string().contains("terminal") || error.to_string().contains("chain"));
}

#[tokio::test]
async fn resume_acquire_requires_revising_latest_revision_and_allocates_revision_two() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let first = save_revision(
        &fixture,
        run_id,
        1,
        &plan("resume first"),
        PlanRevisionStatusV4::Pending,
    )
    .await;
    fixture
        .store
        .request_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            &first.plan_hash,
            "change step",
        )
        .await
        .unwrap();
    let generation = fixture
        .store
        .acquire_plan_revision_resume_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            "resume first",
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(generation.revision, 2);
    assert_eq!(generation.status, PlanRevisionStatusV4::Generating);

    let pending = fixture
        .store
        .finalize_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            generation.revision,
            plan("resume second"),
            "# resume second".into(),
            plan("resume second").canonical_hash().unwrap(),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(pending.revision, 2);
    assert_eq!(
        fixture
            .store
            .proposed_plan_revisions_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

fn minimal_spec(fixture: &Fixture, run_id: Uuid, plan: &ExecutionPlanV4) -> RunSpecV4 {
    let selection = omicsops_protocol::ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "local".into(),
        backend_kind: omicsops_protocol::ComputeBackendKindV4::Local,
        autonomy_mode: omicsops_protocol::AutonomyModeV4::Supervised,
        approval_policy: omicsops_protocol::ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: omicsops_protocol::NetworkPolicyV4::HostInherited,
        container_image: None,
    };
    let profile_id = Uuid::new_v4();
    let approval_hash = RunSpecV4::approval_hash_for(
        run_id,
        fixture.project.id,
        fixture.conversation.id,
        profile_id,
        plan,
        &selection,
    )
    .unwrap();
    RunSpecV4::freeze_with_compute(
        run_id,
        fixture.project.id,
        fixture.conversation.id,
        profile_id,
        plan.clone(),
        selection,
        &approval_hash,
        Utc::now(),
    )
    .unwrap()
}

#[test]
fn revision_request_event_keeps_old_event_json_readable() {
    let original = AgentEventV4::first(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Utc::now(),
        AgentEventKindV4::PlanRevisionRequested {
            plan_hash: "abc".into(),
            feedback: "fix".into(),
        },
    );
    let mut value = serde_json::to_value(&original).unwrap();
    value["event"]["kind"] = json!("plan_revision_request");
    let event: AgentEventV4 = serde_json::from_value(value).unwrap();
    event.verify().unwrap();
    assert_eq!(event, original);
    let old = serde_json::to_value(AgentEventKindV4::RunCreated {
        mode: RunModeV4::Plan,
    })
    .unwrap();
    assert_eq!(old["kind"], "run_created");
}

#[tokio::test]
async fn real_legacy_revision_request_hash_loads_and_chains_to_canonical_writes() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&fixture, run_id).await;
    let first = AgentEventV4::first(
        run_id,
        fixture.project.id,
        fixture.conversation.id,
        Utc::now(),
        AgentEventKindV4::PlanRevisionRequested {
            plan_hash: "legacy-plan-hash".into(),
            feedback: "legacy feedback".into(),
        },
    );
    let mut raw = serde_json::to_value(&first).unwrap();
    raw["event"]["kind"] = json!("plan_revision_request");
    let envelope = json!({
        "schema_version": raw["schema_version"],
        "run_id": raw["run_id"],
        "project_id": raw["project_id"],
        "conversation_id": raw["conversation_id"],
        "sequence": raw["sequence"],
        "occurred_at": raw["occurred_at"],
        "previous_hash": raw["previous_hash"],
        "event": raw["event"],
    });
    raw["event_hash"] = json!(hex::encode(Sha256::digest(
        serde_json::to_vec(&envelope).unwrap(),
    )));
    let event_json = serde_json::to_string(&raw).unwrap();
    sqlx::query(
        "INSERT INTO agent_events_v4
         (run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json,occurred_at)
         VALUES (?1,?2,?3,1,?4,?5,?6,?7)",
    )
    .bind(run_id.to_string())
    .bind(fixture.project.id.to_string())
    .bind(fixture.conversation.id.to_string())
    .bind("")
    .bind(raw["event_hash"].as_str().unwrap())
    .bind(event_json)
    .bind(first.occurred_at.timestamp_millis())
    .execute(fixture.store.pool())
    .await
    .unwrap();

    let loaded = fixture.store.agent_events_v4(run_id).await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert!(matches!(
        loaded[0].event,
        AgentEventKindV4::PlanRevisionRequested { .. }
    ));
    let next = AgentEventV4::next(
        &loaded[0],
        Utc::now(),
        AgentEventKindV4::PlanRevisionRequested {
            plan_hash: "canonical-next".into(),
            feedback: "canonical feedback".into(),
        },
    );
    fixture.store.append_agent_event_v4(&next).await.unwrap();
    let serialized: String =
        sqlx::query_scalar("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 AND sequence=2")
            .bind(run_id.to_string())
            .fetch_one(fixture.store.pool())
            .await
            .unwrap();
    assert_eq!(
        serialized[serialized.find("\"kind\"").unwrap()..]
            .starts_with("\"kind\":\"plan_revision_requested\""),
        true
    );
    fixture.store.agent_events_v4(run_id).await.unwrap();
}

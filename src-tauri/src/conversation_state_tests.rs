use std::collections::BTreeSet;

use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::SessionAgentModeV4;
use omicsops_protocol::ExecutionPlanV4;
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

use crate::agent_v4::conversation_state_response;

struct Fixture {
    store: Store,
    project: Project,
    conversation: Conversation,
    other_project: Project,
}

async fn fixture() -> Fixture {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "command conversation state",
        r"C:\data\command-conversation-state",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let other_project = Project::new(
        Uuid::new_v4(),
        "other command project",
        r"C:\data\other-command-conversation-state",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_project(&other_project).await.unwrap();
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "command conversation",
        Utc::now(),
    );
    store.save_conversation(&conversation).await.unwrap();
    Fixture {
        store,
        project,
        conversation,
        other_project,
    }
}

fn plan() -> ExecutionPlanV4 {
    ExecutionPlanV4 {
        schema_version: 4,
        objective: "command snapshot plan".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["verified".into()],
        requested_capabilities: BTreeSet::new(),
    }
}

#[tokio::test]
async fn command_helper_returns_a_snake_case_agent_state_without_tauri_runtime() {
    let fixture = fixture().await;
    let state =
        conversation_state_response(&fixture.store, fixture.project.id, fixture.conversation.id)
            .await
            .unwrap();
    assert_eq!(state.project_id, fixture.project.id);
    assert_eq!(state.conversation_id, fixture.conversation.id);
    assert_eq!(state.mode, SessionAgentModeV4::Agent);
    assert!(!state.locked);
    assert_eq!(state.latest_plan_revision, None);
    assert_eq!(state.latest_run, None);

    let wire = serde_json::to_value(state).unwrap();
    assert!(wire.get("project_id").is_some());
    assert!(wire.get("conversation_id").is_some());
    assert!(wire.get("latest_plan_revision").is_some());
    assert!(wire.get("latest_run").is_some());
    assert_eq!(wire["mode"], "agent");
}

#[tokio::test]
async fn command_helper_maps_the_latest_run_and_revision_consistently() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    let proposed_plan = plan();
    let plan_hash = proposed_plan.canonical_hash().unwrap();
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
                "objective": "command snapshot plan",
                "status": "awaiting_approval",
                "plan": proposed_plan,
                "plan_hash": plan_hash,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": 1,
                "spec": null
            }),
        )
        .await
        .unwrap();
    let revision = fixture
        .store
        .create_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            run_id,
            1,
            plan(),
            "# command snapshot plan".into(),
            plan_hash.clone(),
            omicsops_dto::PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();

    let state =
        conversation_state_response(&fixture.store, fixture.project.id, fixture.conversation.id)
            .await
            .unwrap();
    assert_eq!(state.mode, SessionAgentModeV4::Plan);
    assert!(state.locked);
    assert_eq!(state.latest_plan_revision, Some(revision.clone()));
    let summary = state.latest_run.unwrap();
    assert_eq!(summary.run_id, run_id);
    assert_eq!(summary.status, "awaiting_approval");
    assert_eq!(summary.plan_revision, Some(revision.revision));
    assert_eq!(summary.plan_hash, Some(plan_hash));
    assert_eq!(summary.session_mode, Some(SessionAgentModeV4::Plan));
}

#[tokio::test]
async fn command_helper_keeps_a_historical_plan_separate_from_a_later_direct_run() {
    let fixture = fixture().await;
    let plan_run_id = Uuid::new_v4();
    let proposed_plan = plan();
    let plan_hash = proposed_plan.canonical_hash().unwrap();
    fixture
        .store
        .save_agent_run_v4(
            plan_run_id,
            fixture.project.id,
            fixture.conversation.id,
            "awaiting_approval",
            &json!({
                "run_id": plan_run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "historical plan",
                "status": "awaiting_approval",
                "plan": proposed_plan,
                "plan_hash": plan_hash,
                "compute_selection": null,
                "approval_hash": null,
                "plan_revision": 1,
                "spec": null
            }),
        )
        .await
        .unwrap();
    let historical = fixture
        .store
        .create_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            plan_run_id,
            1,
            plan(),
            "# historical plan".into(),
            plan_hash,
            omicsops_dto::PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();
    fixture
        .store
        .cancel_plan_v4(
            fixture.project.id,
            fixture.conversation.id,
            plan_run_id,
        )
        .await
        .unwrap();
    fixture
        .store
        .set_conversation_agent_mode(
            fixture.project.id,
            fixture.conversation.id,
            SessionAgentModeV4::Agent,
        )
        .await
        .unwrap();

    let direct_run_id = Uuid::new_v4();
    fixture
        .store
        .save_agent_run_v4(
            direct_run_id,
            fixture.project.id,
            fixture.conversation.id,
            "running",
            &json!({
                "run_id": direct_run_id,
                "project_id": fixture.project.id,
                "conversation_id": fixture.conversation.id,
                "model_profile_id": Uuid::new_v4(),
                "objective": "later direct run",
                "status": "running",
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

    let state =
        conversation_state_response(&fixture.store, fixture.project.id, fixture.conversation.id)
            .await
            .unwrap();
    assert_eq!(state.mode, SessionAgentModeV4::Agent);
    assert!(!state.locked);
    let latest_revision = state.latest_plan_revision.unwrap();
    assert_eq!(latest_revision.id, historical.id);
    assert_eq!(latest_revision.status, omicsops_dto::PlanRevisionStatusV4::Cancelled);
    let summary = state.latest_run.unwrap();
    assert_eq!(summary.run_id, direct_run_id);
    assert_eq!(summary.status, "running");
    assert_eq!(summary.plan, None);
    assert_eq!(summary.plan_hash, None);
    assert_eq!(summary.plan_revision, None);
    assert_eq!(summary.session_mode, Some(SessionAgentModeV4::Agent));
}

#[tokio::test]
async fn command_helper_rejects_cross_project_conversation_requests() {
    let fixture = fixture().await;
    let error = conversation_state_response(
        &fixture.store,
        fixture.other_project.id,
        fixture.conversation.id,
    )
    .await
    .unwrap_err();
    assert!(error.contains("conversation") && error.contains("project"));
}

#[tokio::test]
async fn command_helper_rejects_an_untyped_legacy_run_json() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "running",
            &json!({"run_id": run_id}),
        )
        .await
        .unwrap();

    let error =
        conversation_state_response(&fixture.store, fixture.project.id, fixture.conversation.id)
            .await
            .unwrap_err();
    assert!(!error.is_empty());
}

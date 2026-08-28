use std::collections::BTreeSet;

use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::{PlanRevisionStatusV4, SessionAgentModeV4};
use omicsops_protocol::ExecutionPlanV4;
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

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
        "conversation state project",
        r"C:\data\conversation-state",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let other_project = Project::new(
        Uuid::new_v4(),
        "other project",
        r"C:\data\other-conversation-state",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_project(&other_project).await.unwrap();
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "conversation state", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    Fixture {
        store,
        project,
        conversation,
        other_project,
    }
}

fn plan(objective: &str) -> ExecutionPlanV4 {
    ExecutionPlanV4 {
        schema_version: 4,
        objective: objective.into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["verified".into()],
        requested_capabilities: BTreeSet::new(),
    }
}

fn run_value(
    run_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    status: &str,
) -> serde_json::Value {
    json!({
        "run_id": run_id,
        "project_id": project_id,
        "conversation_id": conversation_id,
        "model_profile_id": Uuid::new_v4(),
        "objective": "state snapshot",
        "status": status,
        "plan": null,
        "plan_hash": null,
        "compute_selection": null,
        "approval_hash": null,
        "plan_revision": null,
        "spec": null
    })
}

#[tokio::test]
async fn snapshot_reads_mode_lock_latest_revision_and_latest_run_together() {
    let fixture = fixture().await;
    let first_run = Uuid::new_v4();
    fixture
        .store
        .save_agent_run_v4(
            first_run,
            fixture.project.id,
            fixture.conversation.id,
            "completed",
            &run_value(
                first_run,
                fixture.project.id,
                fixture.conversation.id,
                "completed",
            ),
        )
        .await
        .unwrap();
    let latest_run = Uuid::new_v4();
    fixture
        .store
        .save_agent_run_v4(
            latest_run,
            fixture.project.id,
            fixture.conversation.id,
            "awaiting_approval",
            &run_value(
                latest_run,
                fixture.project.id,
                fixture.conversation.id,
                "awaiting_approval",
            ),
        )
        .await
        .unwrap();
    let proposal = plan("latest proposal");
    let proposal = fixture
        .store
        .create_proposed_plan_revision_v4(
            fixture.project.id,
            fixture.conversation.id,
            latest_run,
            1,
            proposal.clone(),
            "# latest proposal".into(),
            proposal.canonical_hash().unwrap(),
            PlanRevisionStatusV4::Pending,
            None,
            Utc::now(),
        )
        .await
        .unwrap();

    let snapshot = fixture
        .store
        .conversation_agent_state_v4(fixture.project.id, fixture.conversation.id)
        .await
        .unwrap();
    assert_eq!(snapshot.project_id, fixture.project.id);
    assert_eq!(snapshot.conversation_id, fixture.conversation.id);
    assert_eq!(snapshot.mode, SessionAgentModeV4::Plan);
    assert!(snapshot.locked);
    assert_eq!(snapshot.latest_plan_revision, Some(proposal));
    assert_eq!(
        snapshot
            .latest_run_json
            .as_ref()
            .and_then(|value| value.get("run_id").and_then(|value| value.as_str())),
        Some(latest_run.to_string().as_str())
    );
}

#[tokio::test]
async fn snapshot_defaults_legacy_mode_and_has_no_optional_rows() {
    let fixture = fixture().await;
    let snapshot = fixture
        .store
        .conversation_agent_state_v4(fixture.project.id, fixture.conversation.id)
        .await
        .unwrap();
    assert_eq!(snapshot.mode, SessionAgentModeV4::Agent);
    assert!(!snapshot.locked);
    assert_eq!(snapshot.latest_plan_revision, None);
    assert_eq!(snapshot.latest_run_json, None);
}

#[tokio::test]
async fn snapshot_rejects_a_conversation_owned_by_another_project() {
    let fixture = fixture().await;
    let error = fixture
        .store
        .conversation_agent_state_v4(fixture.other_project.id, fixture.conversation.id)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("conversation") && error.contains("project"));
}

#[tokio::test]
async fn snapshot_rejects_an_invalid_persisted_mode_without_writing() {
    let fixture = fixture().await;
    sqlx::query(
        "INSERT INTO settings (scope,key,value_json,updated_at) VALUES ('global',?1,'\"invalid\"',0)",
    )
    .bind(format!("conversation_agent_mode:{}", fixture.conversation.id))
    .execute(fixture.store.pool())
    .await
    .unwrap();

    let error = fixture
        .store
        .conversation_agent_state_v4(fixture.project.id, fixture.conversation.id)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("conversation agent mode"));
}

#[tokio::test]
async fn snapshot_rejects_latest_run_json_with_a_mismatched_identity() {
    let fixture = fixture().await;
    let run_id = Uuid::new_v4();
    fixture
        .store
        .save_agent_run_v4(
            run_id,
            fixture.project.id,
            fixture.conversation.id,
            "running",
            &run_value(
                Uuid::new_v4(),
                fixture.project.id,
                fixture.conversation.id,
                "running",
            ),
        )
        .await
        .unwrap();

    let error = fixture
        .store
        .conversation_agent_state_v4(fixture.project.id, fixture.conversation.id)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("inconsistent run_id"));
}

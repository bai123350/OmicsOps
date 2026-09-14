use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::ConversationAgentPreferencesV4;
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

async fn fixture() -> (Store, Project, Conversation, Project) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "preferences project",
        r"C:\data\preferences-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let other_project = Project::new(
        Uuid::new_v4(),
        "other project",
        r"C:\data\other-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_project(&other_project).await.unwrap();
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "preferences conversation",
        Utc::now(),
    );
    store.save_conversation(&conversation).await.unwrap();
    (store, project, conversation, other_project)
}

fn preferences() -> ConversationAgentPreferencesV4 {
    ConversationAgentPreferencesV4 {
        delegation_enabled: false,
        auto_review: true,
        memory_enabled: false,
        ..Default::default()
    }
}

fn setting_key(conversation_id: Uuid) -> String {
    format!("conversation_agent_preferences:{conversation_id}")
}

#[tokio::test]
async fn missing_preferences_default_to_all_enabled_without_creating_a_row() {
    let (store, project, conversation, _) = fixture().await;

    assert_eq!(
        store
            .get_conversation_agent_preferences(project.id, conversation.id)
            .await
            .unwrap(),
        ConversationAgentPreferencesV4::default()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE scope='global' AND key=?1")
            .bind(setting_key(conversation.id))
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn preferences_persist_across_a_file_store_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.sqlite");
    let project = Project::new(
        Uuid::new_v4(),
        "preferences project",
        r"C:\data\preferences-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "preferences conversation",
        Utc::now(),
    );
    let store = Store::open(&path).await.unwrap();
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    store
        .set_conversation_agent_preferences(project.id, conversation.id, preferences())
        .await
        .unwrap();
    drop(store);

    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .get_conversation_agent_preferences(project.id, conversation.id)
            .await
            .unwrap(),
        preferences()
    );
}

#[tokio::test]
async fn preferences_reject_cross_project_access_without_writing() {
    let (store, project, conversation, other_project) = fixture().await;

    let get_error = store
        .get_conversation_agent_preferences(other_project.id, conversation.id)
        .await
        .unwrap_err()
        .to_string();
    assert!(get_error.contains("does not belong"), "{get_error}");

    let set_error = store
        .set_conversation_agent_preferences(other_project.id, conversation.id, preferences())
        .await
        .unwrap_err()
        .to_string();
    assert!(set_error.contains("does not belong"), "{set_error}");
    assert_eq!(
        store
            .get_conversation_agent_preferences(project.id, conversation.id)
            .await
            .unwrap(),
        ConversationAgentPreferencesV4::default()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE scope='global' AND key=?1")
            .bind(setting_key(conversation.id))
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn tampered_preferences_are_rejected_instead_of_defaulting() {
    let (store, project, conversation, _) = fixture().await;
    sqlx::query("INSERT INTO settings(scope,key,value_json,updated_at) VALUES('global',?1,?2,?3)")
        .bind(setting_key(conversation.id))
        .bind(
            json!({
                "delegation_enabled": true,
                "auto_review": true,
                "memory_enabled": true,
                "unexpected": false,
            })
            .to_string(),
        )
        .bind(Utc::now().timestamp())
        .execute(store.pool())
        .await
        .unwrap();

    let error = store
        .get_conversation_agent_preferences(project.id, conversation.id)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("invalid conversation agent preferences"),
        "{error}"
    );
}

#[tokio::test]
async fn deleting_a_conversation_removes_preferences_in_the_same_transaction() {
    let (store, project, conversation, _) = fixture().await;
    store
        .set_conversation_agent_preferences(project.id, conversation.id, preferences())
        .await
        .unwrap();

    assert!(
        store
            .delete_conversation(project.id, conversation.id)
            .await
            .unwrap()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE scope='global' AND key=?1")
            .bind(setting_key(conversation.id))
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn locked_conversation_delete_rolls_back_preference_removal() {
    let (store, project, conversation, _) = fixture().await;
    store
        .set_conversation_agent_preferences(project.id, conversation.id, preferences())
        .await
        .unwrap();
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
                "status": "planning",
            }),
            "locked conversation",
            Utc::now(),
        )
        .await
        .unwrap();

    let update_error = store
        .set_conversation_agent_preferences(
            project.id,
            conversation.id,
            ConversationAgentPreferencesV4 {
                delegation_enabled: true,
                auto_review: false,
                memory_enabled: true,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(
        update_error.to_string().contains("locked"),
        "{update_error}"
    );
    assert!(
        store
            .delete_conversation(project.id, conversation.id)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .get_conversation_agent_preferences(project.id, conversation.id)
            .await
            .unwrap(),
        preferences()
    );
}

#[tokio::test]
async fn preferences_cannot_change_while_an_ordinary_run_is_active() {
    let (store, project, conversation, _) = fixture().await;
    let run_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json)
         VALUES (?1,?2,?3,?4,?5)",
    )
    .bind(run_id.to_string())
    .bind(project.id.to_string())
    .bind(conversation.id.to_string())
    .bind("running")
    .bind(json!({"status":"running"}).to_string())
    .execute(store.pool())
    .await
    .unwrap();

    let error = store
        .set_conversation_agent_preferences(project.id, conversation.id, preferences())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("active run"), "{error}");
    assert_eq!(
        store
            .get_conversation_agent_preferences(project.id, conversation.id)
            .await
            .unwrap(),
        ConversationAgentPreferencesV4::default()
    );
}

#[tokio::test]
async fn direct_run_rejects_a_preference_snapshot_changed_before_atomic_insert() {
    let (store, project, conversation, _) = fixture().await;
    store
        .set_conversation_agent_preferences(project.id, conversation.id, preferences())
        .await
        .unwrap();
    let run_id = Uuid::new_v4();
    let stale = json!({
        "status": "running",
        "conversation_preferences": {
            "delegation_enabled": true,
            "auto_review": true,
            "memory_enabled": false,
        },
    });

    let error = store
        .save_agent_run_v4_if_unlocked(run_id, project.id, conversation.id, "running", &stale)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("preferences changed"), "{error}");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs_v4 WHERE run_id=?1")
        .bind(run_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn planning_start_rejects_a_preference_snapshot_changed_before_atomic_insert() {
    let (store, project, conversation, _) = fixture().await;
    store
        .set_conversation_agent_preferences(project.id, conversation.id, preferences())
        .await
        .unwrap();
    let run_id = Uuid::new_v4();
    let stale_preferences = json!({
        "delegation_enabled": true,
        "auto_review": true,
        "memory_enabled": false,
    });
    let value = json!({
        "status": "planning",
        "conversation_preferences": stale_preferences,
    });

    let error = store
        .start_plan_run_v4(
            run_id,
            project.id,
            conversation.id,
            "planning",
            &value,
            "stale preferences",
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("preferences changed"), "{error}");
    let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs_v4 WHERE run_id=?1")
        .bind(run_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
    let plan_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM proposed_plans WHERE project_id=?1 AND frame_id=?2",
    )
    .bind(project.id.to_string())
    .bind(conversation.id.to_string())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(run_count, 0);
    assert_eq!(plan_count, 0);
}

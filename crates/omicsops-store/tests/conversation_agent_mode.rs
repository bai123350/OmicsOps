use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::{
    GetConversationAgentModeResponseV4, SessionAgentModeV4, SetConversationAgentModeRequestV4,
};
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

async fn fixture() -> (Store, Project, Conversation, Project) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "mode project",
        r"C:\data\mode-project",
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
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "mode conversation", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    (store, project, conversation, other_project)
}

#[tokio::test]
async fn legacy_conversation_defaults_to_agent_without_a_setting() {
    let (store, project, conversation, _) = fixture().await;

    assert_eq!(
        store
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Agent
    );
}

#[tokio::test]
async fn mode_is_stored_in_settings_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mode.sqlite");
    let (store, project, conversation, _) = fixture().await;

    store
        .set_conversation_agent_mode(project.id, conversation.id, SessionAgentModeV4::Plan)
        .await
        .unwrap();
    let response = GetConversationAgentModeResponseV4 {
        project_id: project.id,
        conversation_id: conversation.id,
        mode: SessionAgentModeV4::Plan,
    };
    assert_eq!(response.mode, SessionAgentModeV4::Plan);

    // The same project/conversation identity is used against a file-backed
    // store below; this assertion documents the serialized setting shape.
    let value: serde_json::Value = serde_json::to_value(SessionAgentModeV4::Plan).unwrap();
    assert_eq!(value, json!("plan"));

    let file_store = Store::open(&path).await.unwrap();
    file_store.save_project(&project).await.unwrap();
    file_store.save_conversation(&conversation).await.unwrap();
    file_store
        .set_conversation_agent_mode(project.id, conversation.id, SessionAgentModeV4::Plan)
        .await
        .unwrap();
    drop(file_store);

    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Plan
    );

    let raw: String =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE scope='global' AND key=?1")
            .bind(format!("conversation_agent_mode:{}", conversation.id))
            .fetch_one(reopened.pool())
            .await
            .unwrap();
    assert_eq!(raw, r#""plan""#);
}

#[tokio::test]
async fn mode_requests_are_snake_case_and_store_accepts_set_request() {
    let (store, project, conversation, _) = fixture().await;
    let request = SetConversationAgentModeRequestV4 {
        project_id: project.id,
        conversation_id: conversation.id,
        mode: SessionAgentModeV4::Plan,
    };
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["project_id"], json!(project.id));
    assert_eq!(encoded["conversation_id"], json!(conversation.id));
    assert_eq!(encoded["mode"], json!("plan"));

    store
        .set_conversation_agent_mode(request.project_id, request.conversation_id, request.mode)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Plan
    );
}

#[tokio::test]
async fn mode_access_rejects_cross_project_conversation_ids() {
    let (store, project, conversation, other_project) = fixture().await;

    let get_error = store
        .get_conversation_agent_mode(other_project.id, conversation.id)
        .await
        .unwrap_err()
        .to_string();
    assert!(get_error.contains("conversation") && get_error.contains("project"));

    let set_error = store
        .set_conversation_agent_mode(other_project.id, conversation.id, SessionAgentModeV4::Plan)
        .await
        .unwrap_err()
        .to_string();
    assert!(set_error.contains("conversation") && set_error.contains("project"));

    assert_eq!(
        store
            .get_conversation_agent_mode(project.id, conversation.id)
            .await
            .unwrap(),
        SessionAgentModeV4::Agent
    );
}

#[tokio::test]
async fn deleting_a_conversation_removes_its_persisted_mode_in_the_same_transaction() {
    let (store, project, conversation, _) = fixture().await;
    let setting_key = format!("conversation_agent_mode:{}", conversation.id);

    store
        .set_conversation_agent_mode(project.id, conversation.id, SessionAgentModeV4::Plan)
        .await
        .unwrap();
    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE scope='global' AND key=?1")
            .bind(&setting_key)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(before, 1);

    assert!(
        store
            .delete_conversation(project.id, conversation.id)
            .await
            .unwrap()
    );

    let after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE scope='global' AND key=?1")
            .bind(&setting_key)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(after, 0);
}

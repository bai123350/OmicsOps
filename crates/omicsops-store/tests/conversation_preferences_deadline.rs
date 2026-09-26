use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::ConversationAgentPreferencesV4;
use omicsops_store::Store;
use std::time::Duration;
use uuid::Uuid;

async fn fixture() -> (tempfile::TempDir, Store, Uuid, Uuid) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("preferences.sqlite"))
        .await
        .unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "preference fence",
        r"C:\data\preference-fence",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "fenced conversation",
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    (directory, store, project.id, conversation.id)
}

#[tokio::test]
async fn expired_save_cannot_write_after_a_fenced_read() {
    let (_directory, store, project_id, conversation_id) = fixture().await;
    let deadline = Utc::now().timestamp_millis() - 1;
    let candidate = ConversationAgentPreferencesV4 {
        delegation_enabled: false,
        ..Default::default()
    };
    let current = store
        .reconcile_conversation_agent_preferences_after_deadline(
            project_id,
            conversation_id,
            deadline,
        )
        .await
        .unwrap();
    assert_eq!(current, ConversationAgentPreferencesV4::default());
    assert!(
        store
            .set_conversation_agent_preferences_with_deadline(
                project_id,
                conversation_id,
                candidate,
                deadline
            )
            .await
            .is_err()
    );
    assert_eq!(
        store
            .get_conversation_agent_preferences(project_id, conversation_id)
            .await
            .unwrap(),
        ConversationAgentPreferencesV4::default()
    );
}

#[tokio::test]
async fn a_save_waiting_for_the_write_lock_expires_before_it_can_commit() {
    let (_directory, store, project_id, conversation_id) = fixture().await;
    let writer = store.pool().begin_with("BEGIN IMMEDIATE").await.unwrap();
    let deadline = Utc::now().timestamp_millis() + 250;
    let candidate = ConversationAgentPreferencesV4 {
        delegation_enabled: false,
        ..Default::default()
    };
    let pending_store = store.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let pending = tokio::spawn(async move {
        let _ = started_tx.send(());
        pending_store
            .set_conversation_agent_preferences_with_deadline(
                project_id,
                conversation_id,
                candidate,
                deadline,
            )
            .await
    });
    started_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!pending.is_finished());
    tokio::time::sleep(Duration::from_millis(250)).await;
    writer.commit().await.unwrap();
    assert!(pending
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("preference save deadline expired"));
    assert_eq!(
        store
            .reconcile_conversation_agent_preferences_after_deadline(
                project_id,
                conversation_id,
                deadline
            )
            .await
            .unwrap(),
        ConversationAgentPreferencesV4::default()
    );
}

#[tokio::test]
async fn fenced_read_waits_for_a_writer_and_returns_its_committed_value() {
    let (_directory, store, project_id, conversation_id) = fixture().await;
    let mut writer = store.pool().begin_with("BEGIN IMMEDIATE").await.unwrap();
    let candidate = ConversationAgentPreferencesV4 {
        delegation_enabled: false,
        ..Default::default()
    };
    sqlx::query(
        "INSERT INTO settings (scope,key,value_json,updated_at) VALUES ('global',?1,?2,?3)",
    )
    .bind(format!("conversation_agent_preferences:{conversation_id}"))
    .bind(serde_json::to_string(&candidate).unwrap())
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *writer)
    .await
    .unwrap();
    let deadline = Utc::now().timestamp_millis() - 1;
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(1),
            store.get_conversation_agent_preferences(project_id, conversation_id)
        )
        .await
        .unwrap()
        .unwrap(),
        ConversationAgentPreferencesV4::default()
    );
    let reader_store = store.clone();
    let reader = tokio::spawn(async move {
        reader_store
            .reconcile_conversation_agent_preferences_after_deadline(
                project_id,
                conversation_id,
                deadline,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!reader.is_finished());
    writer.commit().await.unwrap();
    assert_eq!(reader.await.unwrap().unwrap(), candidate);
}

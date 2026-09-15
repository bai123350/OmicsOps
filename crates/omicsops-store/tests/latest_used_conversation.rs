use chrono::{TimeZone, Utc};
use omicsops_core::workspace::{Conversation, Message, MessageRole, Project, ProjectTemplate};
use omicsops_store::Store;
use uuid::Uuid;

fn instant(second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 15, 8, 0, 0).single().unwrap()
        + chrono::Duration::seconds(i64::from(second))
}

fn project(id: u128, name: &str) -> Project {
    Project::new(
        Uuid::from_u128(id),
        name,
        format!(r"C:\data\{name}"),
        ProjectTemplate::Blank,
        instant(0),
    )
}

fn conversation(id: u128, project_id: Uuid, title: &str, updated_at: u32) -> Conversation {
    let mut conversation = Conversation::new(Uuid::from_u128(id), project_id, title, instant(0));
    conversation.updated_at = instant(updated_at);
    conversation
}

fn message(
    id: u128,
    conversation: &Conversation,
    sequence: u64,
    role: MessageRole,
    markdown: &str,
    created_at: u32,
) -> Message {
    Message::markdown(
        Uuid::from_u128(id),
        conversation.project_id,
        conversation.id,
        sequence,
        role,
        markdown,
        instant(created_at),
    )
}

#[tokio::test]
async fn returns_none_when_project_has_no_user_message() {
    let store = Store::open_in_memory().await.unwrap();
    let project = project(1, "empty-project");
    store.save_project(&project).await.unwrap();

    let draft = conversation(10, project.id, "Named draft", 30);
    let system_only = conversation(11, project.id, "System only", 20);
    let assistant_only = conversation(12, project.id, "Assistant only", 10);
    for conversation in [&draft, &system_only, &assistant_only] {
        store.save_conversation(conversation).await.unwrap();
    }
    store
        .save_message(&message(
            100,
            &system_only,
            0,
            MessageRole::System,
            "system",
            20,
        ))
        .await
        .unwrap();
    store
        .save_message(&message(
            101,
            &assistant_only,
            0,
            MessageRole::Assistant,
            "assistant",
            30,
        ))
        .await
        .unwrap();

    assert_eq!(
        store.latest_used_conversation(project.id).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn stays_in_project_and_ignores_newer_named_drafts() {
    let store = Store::open_in_memory().await.unwrap();
    let selected_project = project(1, "selected-project");
    let other_project = project(2, "other-project");
    store.save_project(&selected_project).await.unwrap();
    store.save_project(&other_project).await.unwrap();

    let used = conversation(10, selected_project.id, "Used", 1);
    let newer_draft = conversation(11, selected_project.id, "Newer named draft", 50);
    let other_used = conversation(12, other_project.id, "Other project", 60);
    for conversation in [&used, &newer_draft, &other_used] {
        store.save_conversation(conversation).await.unwrap();
    }
    store
        .save_message(&message(100, &used, 0, MessageRole::User, "question", 2))
        .await
        .unwrap();
    store
        .save_message(&message(
            101,
            &other_used,
            0,
            MessageRole::User,
            "other question",
            59,
        ))
        .await
        .unwrap();

    assert_eq!(
        store
            .latest_used_conversation(selected_project.id)
            .await
            .unwrap(),
        Some(used)
    );
}

#[tokio::test]
async fn ranks_by_latest_main_message_and_accepts_attachment_only_user_message() {
    let store = Store::open_in_memory().await.unwrap();
    let project = project(1, "activity-project");
    store.save_project(&project).await.unwrap();

    let mut assistant_recent = conversation(10, project.id, "Assistant recent", 1);
    let renamed_recently = conversation(11, project.id, "Renamed recently", 59);
    store.save_conversation(&assistant_recent).await.unwrap();
    store.save_conversation(&renamed_recently).await.unwrap();

    // Empty markdown is how an attachment-only user turn is represented in the
    // transcript. Its row still makes this conversation eligible.
    store
        .save_message(&message(
            100,
            &assistant_recent,
            0,
            MessageRole::User,
            "",
            2,
        ))
        .await
        .unwrap();
    store
        .save_message(&message(
            101,
            &assistant_recent,
            1,
            MessageRole::Assistant,
            "latest response",
            50,
        ))
        .await
        .unwrap();
    store
        .save_message(&message(
            102,
            &renamed_recently,
            0,
            MessageRole::User,
            "newer question",
            40,
        ))
        .await
        .unwrap();

    // A later rename/save must not affect message activity ordering.
    assistant_recent.title = "Assistant recent renamed".into();
    assistant_recent.updated_at = instant(60);
    store.save_conversation(&assistant_recent).await.unwrap();

    assert_eq!(
        store.latest_used_conversation(project.id).await.unwrap(),
        Some(assistant_recent)
    );
}

#[tokio::test]
async fn latest_tool_or_system_row_counts_as_activity_after_a_user_message() {
    let store = Store::open_in_memory().await.unwrap();
    let project = project(1, "role-activity-project");
    store.save_project(&project).await.unwrap();

    let stale = conversation(10, project.id, "Stale", 1);
    let current = conversation(11, project.id, "Current", 1);
    store.save_conversation(&stale).await.unwrap();
    store.save_conversation(&current).await.unwrap();
    store
        .save_message(&message(100, &stale, 0, MessageRole::User, "old", 10))
        .await
        .unwrap();
    store
        .save_message(&message(
            101,
            &stale,
            1,
            MessageRole::System,
            "late system row",
            50,
        ))
        .await
        .unwrap();
    store
        .save_message(&message(
            102,
            &stale,
            2,
            MessageRole::Tool,
            "late tool row",
            51,
        ))
        .await
        .unwrap();
    store
        .save_message(&message(103, &current, 0, MessageRole::User, "current", 20))
        .await
        .unwrap();

    assert_eq!(
        store.latest_used_conversation(project.id).await.unwrap(),
        Some(stale)
    );
}

#[tokio::test]
async fn breaks_equal_activity_ties_by_conversation_id() {
    let store = Store::open_in_memory().await.unwrap();
    let project = project(1, "tie-project");
    store.save_project(&project).await.unwrap();

    let lower_id = conversation(10, project.id, "Lower ID", 1);
    let higher_id = conversation(11, project.id, "Higher ID", 1);
    store.save_conversation(&higher_id).await.unwrap();
    store.save_conversation(&lower_id).await.unwrap();
    store
        .save_message(&message(
            100,
            &higher_id,
            0,
            MessageRole::User,
            "higher",
            20,
        ))
        .await
        .unwrap();
    store
        .save_message(&message(101, &lower_id, 0, MessageRole::User, "lower", 20))
        .await
        .unwrap();

    assert_eq!(
        store.latest_used_conversation(project.id).await.unwrap(),
        Some(lower_id)
    );
}

#[tokio::test]
async fn side_chat_activity_does_not_make_an_empty_conversation_used() {
    let store = Store::open_in_memory().await.unwrap();
    let project = project(1, "side-chat-project");
    let conversation = conversation(10, project.id, "Empty main transcript", 1);
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();

    sqlx::query(
        "INSERT INTO side_chat_turns_v4
         (request_id,project_id,conversation_id,request_hash,status,value_json,created_at,updated_at)
         VALUES (?1,?2,?3,?4,'completed','{}',?5,?6)",
    )
    .bind(Uuid::from_u128(100).to_string())
    .bind(project.id.to_string())
    .bind(conversation.id.to_string())
    .bind("a".repeat(64))
    .bind(instant(40).timestamp_millis())
    .bind(instant(50).timestamp_millis())
    .execute(store.pool())
    .await
    .unwrap();

    assert_eq!(
        store.latest_used_conversation(project.id).await.unwrap(),
        None
    );
}

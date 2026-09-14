use chrono::{TimeZone, Utc};
use omicsops_core::workspace::{Conversation, Message, MessageRole, Project, ProjectTemplate};
use omicsops_dto::{
    ComposerReference, SideChatSourceV4, SideChatSourceWatermarkV4, SideChatTurnStatusV4,
    SideChatTurnV4, side_chat_source_snapshot_hash,
};
use omicsops_store::{Store, StoreError};
use sha2::{Digest, Sha256};
use uuid::Uuid;

struct Fixture {
    store: Store,
    project: Project,
    conversation: Conversation,
    profile_id: Uuid,
}

async fn fixture() -> Fixture {
    let store = Store::open_in_memory().await.unwrap();
    let now = Utc.timestamp_millis_opt(1_000).single().unwrap();
    let project = Project::new(
        Uuid::from_u128(100),
        "Side chat project",
        r"C:\data\side-chat",
        ProjectTemplate::Blank,
        now,
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::from_u128(101), project.id, "Conversation", now);
    store.save_conversation(&conversation).await.unwrap();
    Fixture {
        store,
        project,
        conversation,
        profile_id: Uuid::from_u128(102),
    }
}

fn turn(fixture: &Fixture, request_id: Uuid, question: &str) -> SideChatTurnV4 {
    let message_id = Uuid::from_u128(103);
    let source = SideChatSourceV4 {
        source_id: format!("message:{message_id}"),
        message_id: Some(message_id),
        message_content_sha256: Some(hex::encode(Sha256::digest(b"A persisted question"))),
        run_id: None,
        event_sequence: None,
        event_hash: None,
        sequence: 1,
        role: "user".into(),
        label: "User message".into(),
        excerpt: "A persisted question".into(),
    };
    let source_watermark = SideChatSourceWatermarkV4 {
        message_count: 1,
        event_count: 0,
        message_head_sequence: Some(1),
        event_heads: vec![],
    };
    SideChatTurnV4 {
        id: request_id,
        request_id,
        parent_request_id: None,
        project_id: fixture.project.id,
        conversation_id: fixture.conversation.id,
        model_profile_id: fixture.profile_id,
        model_label: "Side model".into(),
        question_markdown: question.into(),
        references: vec![ComposerReference::Skill {
            id: Uuid::from_u128(104),
        }],
        attachments: vec![],
        source_snapshot_sha256: side_chat_source_snapshot_hash(
            &source_watermark,
            &[source.clone()],
        )
        .unwrap(),
        source_watermark,
        sources: vec![source],
        status: SideChatTurnStatusV4::Queued,
        answer_markdown: None,
        cited_source_ids: vec![],
        failure_code: None,
        usage: None,
        created_at: Utc.timestamp_millis_opt(2_000).single().unwrap(),
        updated_at: Utc.timestamp_millis_opt(2_000).single().unwrap(),
    }
}

async fn add_source(fixture: &Fixture) {
    fixture
        .store
        .save_message(&Message::markdown(
            Uuid::from_u128(103),
            fixture.project.id,
            fixture.conversation.id,
            1,
            MessageRole::User,
            "A persisted question",
            Utc.timestamp_millis_opt(1_001).single().unwrap(),
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn side_chat_duplicate_is_idempotent_and_conflict_is_rejected() -> Result<(), StoreError> {
    let fixture = fixture().await;
    add_source(&fixture).await;
    let request_id = Uuid::from_u128(200);
    let first = fixture
        .store
        .begin_side_chat_turn(turn(&fixture, request_id, "first"))
        .await?;
    assert!(first.acquired);
    let second = fixture
        .store
        .begin_side_chat_turn(turn(&fixture, request_id, "first"))
        .await?;
    assert!(!second.acquired);
    assert_eq!(second.turn, first.turn);
    let conflict = fixture
        .store
        .begin_side_chat_turn(turn(&fixture, request_id, "changed"))
        .await;
    assert!(
        matches!(conflict, Err(StoreError::InvalidInput(message)) if message.contains("reused"))
    );
    Ok(())
}

#[tokio::test]
async fn side_chat_scope_and_primary_busy_are_independent() -> Result<(), StoreError> {
    let fixture = fixture().await;
    add_source(&fixture).await;
    let first = fixture
        .store
        .begin_side_chat_turn(turn(&fixture, Uuid::from_u128(201), "first"))
        .await?;
    assert!(first.acquired);
    let blocked = fixture
        .store
        .begin_side_chat_turn(turn(&fixture, Uuid::from_u128(202), "second"))
        .await;
    assert!(
        matches!(blocked, Err(StoreError::InvalidInput(message)) if message.contains("side chat"))
    );
    let other_project = Project::new(
        Uuid::from_u128(300),
        "Other",
        r"C:\data\other",
        ProjectTemplate::Blank,
        Utc.timestamp_millis_opt(1_000).single().unwrap(),
    );
    fixture.store.save_project(&other_project).await?;
    let other_conversation = Conversation::new(
        Uuid::from_u128(301),
        other_project.id,
        "Other",
        Utc.timestamp_millis_opt(1_000).single().unwrap(),
    );
    fixture.store.save_conversation(&other_conversation).await?;
    let mut other = turn(&fixture, Uuid::from_u128(203), "other");
    other.project_id = other_project.id;
    other.conversation_id = other_conversation.id;
    assert!(fixture.store.begin_side_chat_turn(other).await.is_err());
    Ok(())
}

#[tokio::test]
async fn side_chat_complete_validates_citations_and_terminal_cas() -> Result<(), StoreError> {
    let fixture = fixture().await;
    add_source(&fixture).await;
    let request_id = Uuid::from_u128(204);
    fixture
        .store
        .begin_side_chat_turn(turn(&fixture, request_id, "question"))
        .await?;
    let completed = fixture
        .store
        .complete_side_chat_turn(
            fixture.project.id,
            fixture.conversation.id,
            request_id,
            "Answer",
            &[format!("message:{}", Uuid::from_u128(103))],
            None,
        )
        .await?;
    assert_eq!(completed.status, SideChatTurnStatusV4::Completed);
    let again = fixture
        .store
        .complete_side_chat_turn(
            fixture.project.id,
            fixture.conversation.id,
            request_id,
            "Answer",
            &[],
            None,
        )
        .await;
    assert!(again.is_err());
    Ok(())
}

#[tokio::test]
async fn side_chat_rejects_message_content_changed_after_native_snapshot() -> Result<(), StoreError>
{
    let fixture = fixture().await;
    add_source(&fixture).await;
    let request_id = Uuid::from_u128(205);
    let stale = turn(&fixture, request_id, "question");
    fixture
        .store
        .save_message(&Message::markdown(
            Uuid::from_u128(103),
            fixture.project.id,
            fixture.conversation.id,
            1,
            MessageRole::User,
            "The persisted question changed",
            Utc.timestamp_millis_opt(1_002).single().unwrap(),
        ))
        .await?;
    let result = fixture.store.begin_side_chat_turn(stale).await;
    assert!(
        matches!(result, Err(StoreError::InvalidInput(message)) if message.contains("source message changed"))
    );
    Ok(())
}

#[tokio::test]
async fn side_chat_accepts_and_cites_typed_material_sources() -> Result<(), StoreError> {
    let fixture = fixture().await;
    add_source(&fixture).await;
    let mut side = turn(&fixture, Uuid::from_u128(206), "material question");
    let excerpt = "Untrusted attachment excerpt";
    side.sources.push(SideChatSourceV4 {
        source_id: format!(
            "material:{}",
            hex::encode(Sha256::digest(excerpt.as_bytes()))
        ),
        message_id: None,
        message_content_sha256: None,
        run_id: None,
        event_sequence: None,
        event_hash: None,
        sequence: 0,
        role: "material".into(),
        label: "Attachment excerpt".into(),
        excerpt: excerpt.into(),
    });
    side.source_snapshot_sha256 =
        side_chat_source_snapshot_hash(&side.source_watermark, &side.sources).unwrap();
    let begun = fixture.store.begin_side_chat_turn(side).await?;
    assert!(begun.acquired);
    let completed = fixture
        .store
        .complete_side_chat_turn(
            fixture.project.id,
            fixture.conversation.id,
            Uuid::from_u128(206),
            "The attachment says so.",
            &[format!(
                "material:{}",
                hex::encode(Sha256::digest(excerpt.as_bytes()))
            )],
            None,
        )
        .await?;
    assert_eq!(completed.status, SideChatTurnStatusV4::Completed);
    Ok(())
}

#[tokio::test]
async fn side_chat_rejects_material_source_without_selected_material() -> Result<(), StoreError> {
    let fixture = fixture().await;
    add_source(&fixture).await;
    let mut side = turn(&fixture, Uuid::from_u128(207), "material question");
    side.references.clear();
    let excerpt = "Untrusted attachment excerpt";
    side.sources.push(SideChatSourceV4 {
        source_id: format!(
            "material:{}",
            hex::encode(Sha256::digest(excerpt.as_bytes()))
        ),
        message_id: None,
        message_content_sha256: None,
        run_id: None,
        event_sequence: None,
        event_hash: None,
        sequence: 0,
        role: "material".into(),
        label: "Attachment excerpt".into(),
        excerpt: excerpt.into(),
    });
    side.source_snapshot_sha256 =
        side_chat_source_snapshot_hash(&side.source_watermark, &side.sources).unwrap();
    let result = fixture.store.begin_side_chat_turn(side).await;
    assert!(
        matches!(result, Err(StoreError::InvalidInput(message)) if message.contains("source must be a message or evidence"))
    );
    Ok(())
}

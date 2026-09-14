use chrono::Utc;
use omicsops_core::workspace::{Conversation, Message, MessageRole, Project, ProjectTemplate};
use omicsops_dto::{ConversationBranchCheckpointKindV4, CreateConversationBranchRequestV4};
use omicsops_store::Store;
use uuid::Uuid;

struct Fixture {
    store: Store,
    project: Project,
    conversation: Conversation,
    messages: Vec<Message>,
}

async fn fixture() -> Fixture {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "branch project",
        r"C:\data\branch-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "source conversation",
        Utc::now(),
    );
    store.save_conversation(&conversation).await.unwrap();

    let messages = vec![
        Message::markdown(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            0,
            MessageRole::User,
            "first user",
            Utc::now(),
        ),
        Message::markdown(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            1,
            MessageRole::Assistant,
            "first answer",
            Utc::now(),
        ),
        Message::markdown(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            2,
            MessageRole::User,
            "second user",
            Utc::now(),
        ),
        Message::markdown(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            3,
            MessageRole::Assistant,
            "second answer",
            Utc::now(),
        ),
    ];
    for message in &messages {
        store.save_message(message).await.unwrap();
    }

    Fixture {
        store,
        project,
        conversation,
        messages,
    }
}

async fn checkpoint(
    fixture: &Fixture,
    source_message_id: Uuid,
    kind: ConversationBranchCheckpointKindV4,
) -> omicsops_dto::ConversationBranchCheckpointV4 {
    fixture
        .store
        .get_conversation_branch_checkpoint_v4(
            fixture.project.id,
            fixture.conversation.id,
            source_message_id,
            kind,
        )
        .await
        .unwrap()
}

fn request(
    fixture: &Fixture,
    checkpoint: &omicsops_dto::ConversationBranchCheckpointV4,
    kind: ConversationBranchCheckpointKindV4,
    title: &str,
) -> CreateConversationBranchRequestV4 {
    CreateConversationBranchRequestV4 {
        request_id: Uuid::new_v4(),
        project_id: fixture.project.id,
        source_conversation_id: fixture.conversation.id,
        source_message_id: checkpoint.source_message_id,
        checkpoint_kind: kind,
        expected_source_sequence: checkpoint.source_sequence,
        expected_head_sequence: checkpoint.source_head_sequence,
        expected_boundary_hash: checkpoint.boundary_hash.clone(),
        title: title.to_owned(),
    }
}

#[tokio::test]
async fn before_user_branch_copies_exact_prefix_with_fresh_ids() {
    let fixture = fixture().await;
    let kind = ConversationBranchCheckpointKindV4::BeforeUser;
    let checkpoint = checkpoint(&fixture, fixture.messages[2].id, kind).await;
    sqlx::query(
        "INSERT INTO agent_runs_v4
         (run_id,project_id,conversation_id,status,value_json)
         VALUES (?1,?2,?3,'completed','{\"status\":\"completed\"}')",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(fixture.project.id.to_string())
    .bind(fixture.conversation.id.to_string())
    .execute(fixture.store.pool())
    .await
    .unwrap();
    let branch = fixture
        .store
        .create_conversation_branch_v4(&request(&fixture, &checkpoint, kind, "draft branch"))
        .await
        .unwrap();

    let copied = fixture
        .store
        .messages_for_conversation(branch.branch_conversation_id)
        .await
        .unwrap();
    assert_eq!(
        copied
            .iter()
            .map(|message| message.markdown.as_str())
            .collect::<Vec<_>>(),
        vec!["first user", "first answer"]
    );
    assert_eq!(
        copied
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(copied.iter().all(|message| {
        !fixture
            .messages
            .iter()
            .any(|source| source.id == message.id)
    }));
    assert_eq!(branch.checkpoint_kind, kind);
    assert_eq!(branch.source_message_id, fixture.messages[2].id);
    let copied_runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2",
    )
    .bind(fixture.project.id.to_string())
    .bind(branch.branch_conversation_id.to_string())
    .fetch_one(fixture.store.pool())
    .await
    .unwrap();
    assert_eq!(copied_runs, 0);
    assert_eq!(
        fixture
            .store
            .conversation_branches_for_source_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap(),
        vec![branch]
    );
}

#[tokio::test]
async fn after_response_branch_copies_selected_turn_before_next_user() {
    let fixture = fixture().await;
    let kind = ConversationBranchCheckpointKindV4::AfterResponse;
    let checkpoint = checkpoint(&fixture, fixture.messages[1].id, kind).await;
    let branch = fixture
        .store
        .create_conversation_branch_v4(&request(&fixture, &checkpoint, kind, "after first"))
        .await
        .unwrap();

    assert_eq!(
        fixture
            .store
            .conversations_for_project(fixture.project.id)
            .await
            .unwrap()
            .iter()
            .find(|conversation| conversation.id == branch.branch_conversation_id)
            .unwrap()
            .title,
        "after first"
    );
    let copied = fixture
        .store
        .messages_for_conversation(branch.branch_conversation_id)
        .await
        .unwrap();
    assert_eq!(
        copied
            .iter()
            .map(|message| message.markdown.as_str())
            .collect::<Vec<_>>(),
        vec!["first user", "first answer"]
    );
}

#[tokio::test]
async fn changed_source_boundary_rolls_back_without_a_partial_conversation() {
    let fixture = fixture().await;
    let kind = ConversationBranchCheckpointKindV4::BeforeUser;
    let checkpoint = checkpoint(&fixture, fixture.messages[2].id, kind).await;
    let message = Message::markdown(
        Uuid::new_v4(),
        fixture.project.id,
        fixture.conversation.id,
        4,
        MessageRole::User,
        "appended after checkpoint",
        Utc::now(),
    );
    fixture.store.save_message(&message).await.unwrap();

    let error = fixture
        .store
        .create_conversation_branch_v4(&request(&fixture, &checkpoint, kind, "stale"))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("checkpoint") || error.contains("changed"),
        "{error}"
    );
    assert_eq!(
        fixture
            .store
            .conversations_for_project(fixture.project.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        fixture
            .store
            .conversation_branches_for_source_v4(fixture.project.id, fixture.conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn changed_source_content_is_rejected_even_when_sequence_is_unchanged() {
    let fixture = fixture().await;
    let kind = ConversationBranchCheckpointKindV4::BeforeUser;
    let checkpoint = checkpoint(&fixture, fixture.messages[2].id, kind).await;
    sqlx::query("UPDATE messages SET content=?1 WHERE id=?2")
        .bind("edited after checkpoint")
        .bind(fixture.messages[1].id.to_string())
        .execute(fixture.store.pool())
        .await
        .unwrap();

    let error = fixture
        .store
        .create_conversation_branch_v4(&request(&fixture, &checkpoint, kind, "content changed"))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("checkpoint") || error.contains("changed"),
        "{error}"
    );
    assert_eq!(
        fixture
            .store
            .conversations_for_project(fixture.project.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn foreign_and_branch_of_branch_sources_are_rejected() {
    let fixture = fixture().await;
    let other_project = Project::new(
        Uuid::new_v4(),
        "other project",
        r"C:\data\other-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    fixture.store.save_project(&other_project).await.unwrap();
    let kind = ConversationBranchCheckpointKindV4::BeforeUser;
    let source_checkpoint = checkpoint(&fixture, fixture.messages[2].id, kind).await;
    let mut foreign_request = request(&fixture, &source_checkpoint, kind, "foreign");
    foreign_request.project_id = other_project.id;
    let foreign_error = fixture
        .store
        .create_conversation_branch_v4(&foreign_request)
        .await
        .unwrap_err()
        .to_string();
    assert!(foreign_error.contains("project") || foreign_error.contains("conversation"));

    let branch = fixture
        .store
        .create_conversation_branch_v4(&request(
            &fixture,
            &source_checkpoint,
            kind,
            "first generation",
        ))
        .await
        .unwrap();
    let branch_message = fixture
        .store
        .messages_for_conversation(branch.branch_conversation_id)
        .await
        .unwrap()
        .first()
        .unwrap()
        .clone();
    let branch_checkpoint = fixture
        .store
        .get_conversation_branch_checkpoint_v4(
            fixture.project.id,
            branch.branch_conversation_id,
            branch_message.id,
            kind,
        )
        .await
        .unwrap();
    let mut branch_request = request(&fixture, &branch_checkpoint, kind, "nested");
    branch_request.source_conversation_id = branch.branch_conversation_id;
    let branch_error = fixture
        .store
        .create_conversation_branch_v4(&branch_request)
        .await
        .unwrap_err()
        .to_string();
    assert!(branch_error.contains("branch"), "{branch_error}");
    assert_eq!(
        fixture
            .store
            .conversations_for_project(fixture.project.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn active_run_blocks_branch_creation() {
    let fixture = fixture().await;
    let kind = ConversationBranchCheckpointKindV4::AfterResponse;
    let checkpoint = checkpoint(&fixture, fixture.messages[0].id, kind).await;
    let run_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_runs_v4
         (run_id,project_id,conversation_id,status,value_json,created_at,updated_at)
         VALUES (?1,?2,?3,'running','{}',?4,?4)",
    )
    .bind(run_id.to_string())
    .bind(fixture.project.id.to_string())
    .bind(fixture.conversation.id.to_string())
    .bind(Utc::now().timestamp_millis())
    .execute(fixture.store.pool())
    .await
    .unwrap();

    let error = fixture
        .store
        .create_conversation_branch_v4(&request(&fixture, &checkpoint, kind, "busy"))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("active run") || error.contains("locked"),
        "{error}"
    );
}

#[tokio::test]
async fn plan_review_and_stop_locks_block_branch_creation() {
    let plan_fixture = fixture().await;
    let kind = ConversationBranchCheckpointKindV4::AfterResponse;
    let plan_checkpoint = checkpoint(&plan_fixture, plan_fixture.messages[0].id, kind).await;
    let run_id = Uuid::new_v4();
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO agent_runs_v4
         (run_id,project_id,conversation_id,status,value_json,created_at,updated_at)
         VALUES (?1,?2,?3,'planning','{}',?4,?4)",
    )
    .bind(run_id.to_string())
    .bind(plan_fixture.project.id.to_string())
    .bind(plan_fixture.conversation.id.to_string())
    .bind(now)
    .execute(plan_fixture.store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO proposed_plans
         (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
         VALUES (?1,?2,?3,1,?4,'pending','{}','',NULL,?5,?6,?6)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(plan_fixture.project.id.to_string())
    .bind(plan_fixture.conversation.id.to_string())
    .bind("branch-plan-hash")
    .bind(run_id.to_string())
    .bind(now)
    .execute(plan_fixture.store.pool())
    .await
    .unwrap();
    let error = plan_fixture
        .store
        .create_conversation_branch_v4(&request(
            &plan_fixture,
            &plan_checkpoint,
            kind,
            "plan locked",
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("active plan") || error.contains("locked"),
        "{error}"
    );

    let review_fixture = fixture().await;
    let review_checkpoint = checkpoint(&review_fixture, review_fixture.messages[0].id, kind).await;
    sqlx::query(
        "INSERT INTO session_reviews
         (id,frame_id,reviewer,status,value_json,created_at,updated_at)
         VALUES (?1,?2,'reviewer','running','{}',?3,?3)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(review_fixture.conversation.id.to_string())
    .bind(now)
    .execute(review_fixture.store.pool())
    .await
    .unwrap();
    let error = review_fixture
        .store
        .create_conversation_branch_v4(&request(
            &review_fixture,
            &review_checkpoint,
            kind,
            "review locked",
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("session review") || error.contains("locked"),
        "{error}"
    );

    let stop_fixture = fixture().await;
    let stop_checkpoint = checkpoint(&stop_fixture, stop_fixture.messages[0].id, kind).await;
    let run_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_runs_v4
         (run_id,project_id,conversation_id,status,value_json,created_at,updated_at)
         VALUES (?1,?2,?3,'completed','{}',?4,?4)",
    )
    .bind(run_id.to_string())
    .bind(stop_fixture.project.id.to_string())
    .bind(stop_fixture.conversation.id.to_string())
    .bind(now)
    .execute(stop_fixture.store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO agent_run_stop_requests_v4
         (request_id,run_id,project_id,conversation_id,status,created_at,updated_at)
         VALUES (?1,?2,?3,?4,'requested',?5,?5)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(run_id.to_string())
    .bind(stop_fixture.project.id.to_string())
    .bind(stop_fixture.conversation.id.to_string())
    .bind(now)
    .execute(stop_fixture.store.pool())
    .await
    .unwrap();
    let error = stop_fixture
        .store
        .create_conversation_branch_v4(&request(
            &stop_fixture,
            &stop_checkpoint,
            kind,
            "stop locked",
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("stop") || error.contains("locked"),
        "{error}"
    );
}

#[tokio::test]
async fn parent_delete_is_blocked_until_child_is_deleted_and_metadata_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("branches.sqlite");
    let store = Store::open(&path).await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "persistent branch project",
        r"C:\data\persistent-branch",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "persistent source", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let source = Message::markdown(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        0,
        MessageRole::User,
        "persistent user",
        Utc::now(),
    );
    store.save_message(&source).await.unwrap();
    let kind = ConversationBranchCheckpointKindV4::AfterResponse;
    let checkpoint = store
        .get_conversation_branch_checkpoint_v4(project.id, conversation.id, source.id, kind)
        .await
        .unwrap();
    let request = CreateConversationBranchRequestV4 {
        request_id: Uuid::new_v4(),
        project_id: project.id,
        source_conversation_id: conversation.id,
        source_message_id: source.id,
        checkpoint_kind: kind,
        expected_source_sequence: checkpoint.source_sequence,
        expected_head_sequence: checkpoint.source_head_sequence,
        expected_boundary_hash: checkpoint.boundary_hash,
        title: "persistent branch".into(),
    };
    let branch = store.create_conversation_branch_v4(&request).await.unwrap();
    drop(store);

    let reopened = Store::open(&path).await.unwrap();
    let stored = reopened
        .get_conversation_branch_v4(project.id, branch.branch_conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.source_conversation_id, conversation.id);
    let delete_error = reopened
        .delete_conversation(project.id, conversation.id)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        delete_error.contains("child") || delete_error.contains("branch"),
        "{delete_error}"
    );
    assert!(
        reopened
            .delete_conversation(project.id, branch.branch_conversation_id)
            .await
            .unwrap()
    );
    assert!(
        reopened
            .delete_conversation(project.id, conversation.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn repeated_request_id_is_idempotent_but_cannot_change_payload() {
    let fixture = fixture().await;
    let kind = ConversationBranchCheckpointKindV4::AfterResponse;
    let checkpoint = checkpoint(&fixture, fixture.messages[0].id, kind).await;
    let mut create = request(&fixture, &checkpoint, kind, "retry me");
    let first_branch = fixture
        .store
        .create_conversation_branch_v4(&create)
        .await
        .unwrap();
    let retry_branch = fixture
        .store
        .create_conversation_branch_v4(&create)
        .await
        .unwrap();
    assert_eq!(
        retry_branch.branch_conversation_id,
        first_branch.branch_conversation_id
    );
    assert_eq!(retry_branch, first_branch);
    assert_eq!(
        fixture
            .store
            .conversations_for_project(fixture.project.id)
            .await
            .unwrap()
            .len(),
        2
    );

    create.project_id = Uuid::new_v4();
    let scope_error = fixture
        .store
        .create_conversation_branch_v4(&create)
        .await
        .unwrap_err()
        .to_string();
    assert!(scope_error.contains("another project") || scope_error.contains("scope"));

    create.project_id = fixture.project.id;
    create.title = "different payload".into();
    let error = fixture
        .store
        .create_conversation_branch_v4(&create)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("request") || error.contains("payload"),
        "{error}"
    );
}

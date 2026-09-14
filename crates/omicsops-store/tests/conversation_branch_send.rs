use chrono::Utc;
use omicsops_core::workspace::{
    Conversation, Message, MessageRole, ModelProfile, Project, ProjectTemplate,
};
use omicsops_dto::{
    ComposerQueueFrozenConfigV4, ComposerQueueMaterialSnapshotV4, ComposerQueueModeV4,
    ConversationBranchCheckpointKindV4, CreateConversationBranchAndSendRequestV4,
    CreateConversationBranchRequestV4, EnqueueComposerTurnRequestV4,
};
use omicsops_protocol::{
    ApprovalPolicyV4, AutonomyModeV4, ComputeBackendKindV4, ComputeSelectionV4, NetworkPolicyV4,
};
use omicsops_store::Store;
use sha2::Digest;
use uuid::Uuid;

struct Fixture {
    store: Store,
    project: Project,
    source: Conversation,
    source_message: Message,
    boundary: omicsops_dto::ConversationBranchCheckpointV4,
}

async fn fixture() -> Fixture {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "branch send project",
        r"C:\data\branch-send",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let source = Conversation::new(Uuid::new_v4(), project.id, "source", Utc::now());
    store.save_conversation(&source).await.unwrap();
    let source_message = Message::markdown(
        Uuid::new_v4(),
        project.id,
        source.id,
        0,
        MessageRole::User,
        "source turn",
        Utc::now(),
    );
    store.save_message(&source_message).await.unwrap();
    let boundary = store
        .get_conversation_branch_checkpoint_v4(
            project.id,
            source.id,
            source_message.id,
            ConversationBranchCheckpointKindV4::AfterResponse,
        )
        .await
        .unwrap();
    Fixture {
        store,
        project,
        source,
        source_message,
        boundary,
    }
}

fn selection() -> ComputeSelectionV4 {
    ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "local".into(),
        backend_kind: ComputeBackendKindV4::Local,
        autonomy_mode: AutonomyModeV4::Supervised,
        approval_policy: ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: NetworkPolicyV4::HostInherited,
        container_image: None,
    }
}

fn request(fixture: &Fixture) -> CreateConversationBranchAndSendRequestV4 {
    let branch = CreateConversationBranchRequestV4 {
        request_id: Uuid::new_v4(),
        project_id: fixture.project.id,
        source_conversation_id: fixture.source.id,
        source_message_id: fixture.source_message.id,
        checkpoint_kind: fixture.boundary.checkpoint_kind,
        expected_source_sequence: fixture.boundary.source_sequence,
        expected_head_sequence: fixture.boundary.source_head_sequence,
        expected_boundary_hash: fixture.boundary.boundary_hash.clone(),
        title: "sent branch".into(),
    };
    CreateConversationBranchAndSendRequestV4 {
        branch,
        message_markdown: "send this full draft".into(),
        mode: ComposerQueueModeV4::Agent,
        model_profile_id: Uuid::new_v4(),
        compute_selection: selection(),
        queue_request_id: Uuid::new_v4(),
        queue_message_id: Uuid::new_v4(),
        queue_run_id: Uuid::new_v4(),
        references: vec![],
        attachments: vec![],
    }
}

#[tokio::test]
async fn branch_send_prepare_is_idempotent_and_rejects_changed_payload() {
    let fixture = fixture().await;
    let mut request = request(&fixture);
    let first = fixture
        .store
        .prepare_conversation_branch_send_v4(&request)
        .await
        .unwrap();
    let retry = fixture
        .store
        .prepare_conversation_branch_send_v4(&request)
        .await
        .unwrap();
    assert_eq!(retry, first);
    assert_eq!(
        fixture
            .store
            .conversations_for_project(fixture.project.id)
            .await
            .unwrap()
            .len(),
        2
    );

    request.message_markdown = "changed after response loss".into();
    let error = fixture
        .store
        .prepare_conversation_branch_send_v4(&request)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("payload") || error.contains("request"),
        "{error}"
    );
}

#[tokio::test]
async fn branch_send_prepare_rejects_duplicate_reserved_ids_before_creating_branch() {
    let fixture = fixture().await;
    let mut request = request(&fixture);
    request.queue_message_id = request.queue_request_id;
    assert!(
        fixture
            .store
            .prepare_conversation_branch_send_v4(&request)
            .await
            .is_err()
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
async fn branch_send_intent_is_removed_when_created_branch_is_deleted() {
    let fixture = fixture().await;
    let request = request(&fixture);
    let branch = fixture
        .store
        .prepare_conversation_branch_send_v4(&request)
        .await
        .unwrap();
    let count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM app_objects WHERE kind='conversation_branch_send_v4'",
    )
    .fetch_one(fixture.store.pool())
    .await
    .unwrap();
    assert_eq!(count_before, 1);
    assert!(
        fixture
            .store
            .delete_conversation(fixture.project.id, branch.branch_conversation_id)
            .await
            .unwrap()
    );
    let count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM app_objects WHERE kind='conversation_branch_send_v4'",
    )
    .fetch_one(fixture.store.pool())
    .await
    .unwrap();
    assert_eq!(count_after, 0);
}

#[tokio::test]
async fn branch_send_keeps_long_initial_message_separate_from_bounded_title() {
    let fixture = fixture().await;
    let mut request = request(&fixture);
    request.message_markdown = "x".repeat(64 * 1024);
    let branch = fixture
        .store
        .prepare_conversation_branch_send_v4(&request)
        .await
        .unwrap();
    assert_ne!(branch.branch_conversation_id, fixture.source.id);
    let title: String =
        sqlx::query_scalar("SELECT title FROM conversation_records WHERE frame_id=?1")
            .bind(branch.branch_conversation_id.to_string())
            .fetch_one(fixture.store.pool())
            .await
            .unwrap();
    assert_eq!(title, "sent branch");
}

#[tokio::test]
async fn branch_send_retry_rejects_a_queue_row_with_same_reserved_ids_but_other_payload() {
    let fixture = fixture().await;
    let request = request(&fixture);
    let profile = ModelProfile {
        id: request.model_profile_id,
        label: "branch send queue profile".into(),
        provider: omicsops_core::workspace::ModelProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.com/v1".into(),
        model: "gpt-branch-send".into(),
        credential_reference: None,
        supports_tools: true,
        supports_vision: false,
        context_window_tokens: None,
        catalog_capabilities: None,
        reasoning_effort: None,
        fast_mode: None,
        delegated_model_profile_id: None,
    };
    fixture.store.save_model_profile(&profile).await.unwrap();
    let branch = fixture
        .store
        .prepare_conversation_branch_send_v4(&request)
        .await
        .unwrap();
    let queue_request = EnqueueComposerTurnRequestV4 {
        request_id: request.queue_request_id,
        message_id: request.queue_message_id,
        run_id: request.queue_run_id,
        project_id: branch.project_id,
        conversation_id: branch.branch_conversation_id,
        mode: request.mode,
        message_markdown: request.message_markdown.clone(),
        model_profile_id: request.model_profile_id,
        compute_selection: request.compute_selection.clone(),
        references: vec![],
        attachments: vec![],
    };
    let frozen = ComposerQueueFrozenConfigV4 {
        model_profile_id: request.model_profile_id,
        model_configuration_hash: profile.execution_configuration_hash(),
        conversation_preferences: Default::default(),
        service_tier: omicsops_protocol::RunServiceTierV4 { fast_mode: None },
        delegated_model: None,
        reviewer_model: None,
        compute_selection: request.compute_selection.clone(),
    };
    let material = ComposerQueueMaterialSnapshotV4 {
        reference_context: String::new(),
        reference_context_sha256: hex::encode(sha2::Sha256::digest([])),
        attachment_receipts: vec![],
        attachment_snapshot_sha256: hex::encode(sha2::Sha256::digest(b"[]")),
    };
    fixture
        .store
        .enqueue_composer_turn(&queue_request, &frozen, &material)
        .await
        .unwrap();

    let mut conflicting = queue_request;
    conflicting.message_markdown = "different queued payload".into();
    assert!(
        fixture
            .store
            .find_composer_enqueue_retry(&conflicting)
            .await
            .is_err()
    );
}

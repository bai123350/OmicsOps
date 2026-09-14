use chrono::Utc;
use omicsops_core::workspace::{Conversation, ModelProfile, Project, ProjectTemplate};
use omicsops_dto::{
    ComposerAttachmentReceipt, ComposerQueueActionRequestV4, ComposerQueueActionV4,
    ComposerQueueFrozenConfigV4, ComposerQueueMaterialSnapshotV4, ComposerQueueModeV4,
    ComposerQueueStatusV4, ComposerReference, EnqueueComposerTurnRequestV4, StopRunRequestV4,
};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ApprovalPolicyV4, AutonomyModeV4, ComputeBackendKindV4,
    ComputeSelectionV4, ExecutionPlanV4, NetworkPolicyV4, RunModeV4, RunServiceTierV4, RunSpecV4,
};
use omicsops_store::Store;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

const PROFILE_ID: Uuid = Uuid::from_u128(0x501);

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

fn profile() -> ModelProfile {
    ModelProfile {
        id: PROFILE_ID,
        label: "cut-in profile".into(),
        provider: omicsops_core::workspace::ModelProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.com/v1".into(),
        model: "gpt-cutin".into(),
        credential_reference: None,
        supports_tools: true,
        supports_vision: false,
        context_window_tokens: None,
        catalog_capabilities: None,
        reasoning_effort: None,
        fast_mode: None,
        delegated_model_profile_id: None,
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hash_json<T: Serialize>(value: &T) -> String {
    hash_bytes(&serde_json::to_vec(value).unwrap())
}

fn request(
    project_id: Uuid,
    conversation_id: Uuid,
    mode: ComposerQueueModeV4,
    message: &str,
) -> EnqueueComposerTurnRequestV4 {
    EnqueueComposerTurnRequestV4 {
        request_id: Uuid::new_v4(),
        message_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        project_id,
        conversation_id,
        mode,
        message_markdown: message.into(),
        model_profile_id: PROFILE_ID,
        compute_selection: selection(),
        references: vec![],
        attachments: vec![],
    }
}

fn frozen(request: &EnqueueComposerTurnRequestV4) -> ComposerQueueFrozenConfigV4 {
    ComposerQueueFrozenConfigV4 {
        model_profile_id: request.model_profile_id,
        model_configuration_hash: profile().execution_configuration_hash(),
        conversation_preferences: omicsops_protocol::ConversationAgentPreferencesV4::default(),
        service_tier: RunServiceTierV4 { fast_mode: None },
        delegated_model: None,
        reviewer_model: None,
        compute_selection: request.compute_selection.clone(),
    }
}

fn material(
    context: &str,
    receipts: Vec<ComposerAttachmentReceipt>,
) -> ComposerQueueMaterialSnapshotV4 {
    ComposerQueueMaterialSnapshotV4 {
        reference_context: context.into(),
        reference_context_sha256: hash_bytes(context.as_bytes()),
        attachment_snapshot_sha256: hash_json(&receipts),
        attachment_receipts: receipts,
    }
}

async fn fixture() -> (Store, Project, Conversation, RunSpecV4) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "cut-in project",
        r"C:\data\cut-in-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "cut-in conversation",
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    store.save_model_profile(&profile()).await.unwrap();

    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "cut-in active run".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: BTreeSet::new(),
    };
    let run_id = Uuid::new_v4();
    let approval_hash = RunSpecV4::approval_hash_for(
        run_id,
        project.id,
        conversation.id,
        PROFILE_ID,
        &plan,
        &selection(),
    )
    .unwrap();
    let spec = RunSpecV4::freeze_ordinary_agent_with_compute(
        run_id,
        project.id,
        conversation.id,
        PROFILE_ID,
        plan,
        selection(),
        &approval_hash,
        Utc::now(),
    )
    .unwrap();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "running",
            &serde_json::json!({"status":"running","spec":spec}),
        )
        .await
        .unwrap();
    let event = AgentEventV4::first(
        run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&event).await.unwrap();
    (store, project, conversation, spec)
}

fn action(item: &omicsops_dto::ComposerQueueItemV4) -> ComposerQueueActionRequestV4 {
    ComposerQueueActionRequestV4 {
        project_id: item.project_id,
        conversation_id: item.conversation_id,
        request_id: item.request_id,
        expected_revision: item.revision,
        action: ComposerQueueActionV4::CutIn,
    }
}

#[tokio::test]
async fn cut_in_accepts_guidance_and_is_idempotent_without_fifo_dispatch() {
    let (store, project, conversation, spec) = fixture().await;
    let queued = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "  inspect the current result  ",
    );
    let item = store
        .enqueue_composer_turn(&queued, &frozen(&queued), &material("", vec![]))
        .await
        .unwrap();
    let request = action(&item);

    let (left, right) = tokio::join!(
        store.composer_queue_action(&request),
        store.composer_queue_action(&request)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_eq!(left, right);
    let cut = left
        .iter()
        .find(|row| row.request_id == queued.request_id)
        .unwrap();
    assert_eq!(cut.status, ComposerQueueStatusV4::Cancelled);
    let cutin_id = cut.cut_in_message_id.expect("cut-in receipt");
    assert_eq!(cut.revision, item.revision + 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM agent_guidance_v4 WHERE run_id=?1 AND message_id=?2",
        )
        .bind(spec.run_id.to_string())
        .bind(cutin_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0,
        "cut-in must not write a FIFO user message"
    );
}

#[tokio::test]
async fn cut_in_rolls_back_guidance_when_queue_update_fails() {
    let (store, project, conversation, spec) = fixture().await;
    let queued = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "rollback this cut-in",
    );
    let item = store
        .enqueue_composer_turn(&queued, &frozen(&queued), &material("", vec![]))
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_cut_in_queue_update
         BEFORE UPDATE OF cutin_message_id ON composer_queue_v4
         BEGIN SELECT RAISE(ABORT, 'test cut-in queue update failure'); END",
    )
    .execute(store.pool())
    .await
    .unwrap();

    assert!(store.cut_in_composer_queue(&action(&item)).await.is_err());
    let fresh = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.request_id == queued.request_id)
        .unwrap();
    assert_eq!(fresh.status, ComposerQueueStatusV4::Pending);
    assert_eq!(fresh.revision, item.revision);
    assert!(fresh.cut_in_message_id.is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_guidance_v4 WHERE run_id=?1",)
            .bind(spec.run_id.to_string())
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0,
        "guidance must roll back with the queue transition"
    );
}

#[tokio::test]
async fn cut_in_rejects_non_text_items_and_preserves_the_pending_row() {
    let (store, project, conversation, _) = fixture().await;
    let mut referenced = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "use this context",
    );
    referenced.references = vec![ComposerReference::Skill { id: Uuid::new_v4() }];
    let referenced_item = store
        .enqueue_composer_turn(
            &referenced,
            &frozen(&referenced),
            &material("skill context", vec![]),
        )
        .await
        .unwrap();
    let before = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    let error = store
        .cut_in_composer_queue(&action(&referenced_item))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("text-only"), "{error}");
    assert_eq!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap(),
        before
    );

    let attachment_id = Uuid::new_v4();
    let mut attached = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "inspect attachment",
    );
    attached.attachments = vec![attachment_id];
    let receipt = ComposerAttachmentReceipt {
        id: attachment_id,
        project_id: project.id,
        conversation_id: conversation.id,
        name: "input.txt".into(),
        relative_path: format!(".omicsops/attachments/{attachment_id}/bytes.txt"),
        size_bytes: 1,
        sha256: "b".repeat(64),
        media_type: "text/plain".into(),
    };
    let attached_item = store
        .enqueue_composer_turn(
            &attached,
            &frozen(&attached),
            &material("", vec![receipt.clone()]),
        )
        .await
        .unwrap();
    let error = store
        .cut_in_composer_queue(&action(&attached_item))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("text-only"), "{error}");
    assert_eq!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.request_id == attached.request_id)
            .unwrap()
            .status,
        ComposerQueueStatusV4::Pending
    );
}

#[tokio::test]
async fn cut_in_rejects_stale_revision_and_cross_scope_without_mutation() {
    let (store, project, conversation, _) = fixture().await;
    let queued = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "scope guarded cut-in",
    );
    let item = store
        .enqueue_composer_turn(&queued, &frozen(&queued), &material("", vec![]))
        .await
        .unwrap();

    let mut stale = action(&item);
    stale.expected_revision += 1;
    assert!(store.cut_in_composer_queue(&stale).await.is_err());

    let foreign_conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "foreign conversation",
        Utc::now(),
    );
    store
        .save_conversation(&foreign_conversation)
        .await
        .unwrap();
    let cross_scope = ComposerQueueActionRequestV4 {
        project_id: project.id,
        conversation_id: foreign_conversation.id,
        request_id: item.request_id,
        expected_revision: item.revision,
        action: ComposerQueueActionV4::CutIn,
    };
    assert!(store.cut_in_composer_queue(&cross_scope).await.is_err());

    let fresh = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.request_id == item.request_id)
        .unwrap();
    assert_eq!(fresh.status, ComposerQueueStatusV4::Pending);
    assert_eq!(fresh.revision, item.revision);
    assert!(fresh.cut_in_message_id.is_none());
}

#[tokio::test]
async fn cut_in_rejects_plan_and_needs_attention_conflicts() {
    let (store, project, conversation, _) = fixture().await;
    let plan_request = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "plan cannot cut in",
    );
    let plan_item = store
        .enqueue_composer_turn(&plan_request, &frozen(&plan_request), &material("", vec![]))
        .await
        .unwrap();
    let plan_error = store
        .cut_in_composer_queue(&action(&plan_item))
        .await
        .unwrap_err()
        .to_string();
    assert!(plan_error.contains("ordinary Agent"), "{plan_error}");

    let attention_request = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "attention cannot cut in",
    );
    let attention_item = store
        .enqueue_composer_turn(
            &attention_request,
            &frozen(&attention_request),
            &material("", vec![]),
        )
        .await
        .unwrap();
    sqlx::query("UPDATE agent_runs_v4 SET status='needs_attention'")
        .execute(store.pool())
        .await
        .unwrap();
    let attention_error = store
        .cut_in_composer_queue(&action(&attention_item))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        attention_error.contains("ordinary Agent"),
        "{attention_error}"
    );
    assert_eq!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.status == ComposerQueueStatusV4::Pending)
            .count(),
        2
    );
}

#[tokio::test]
async fn cut_in_rejects_plan_or_stopping_or_terminal_runs() {
    let (store, project, conversation, spec) = fixture().await;
    let queued = request(
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "cannot cut in now",
    );
    let item = store
        .enqueue_composer_turn(&queued, &frozen(&queued), &material("", vec![]))
        .await
        .unwrap();
    store
        .request_run_stop_v4(&StopRunRequestV4 {
            request_id: Uuid::new_v4(),
            project_id: project.id,
            conversation_id: conversation.id,
            run_id: spec.run_id,
        })
        .await
        .unwrap();
    let error = store
        .cut_in_composer_queue(&action(&item))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("stop"), "{error}");
    assert_eq!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .first()
            .unwrap()
            .status,
        ComposerQueueStatusV4::Pending
    );

    sqlx::query("DELETE FROM agent_run_stop_requests_v4 WHERE run_id=?1")
        .bind(spec.run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE agent_runs_v4 SET status='completed' WHERE run_id=?1")
        .bind(spec.run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let terminal_error = store
        .cut_in_composer_queue(&action(&item))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        terminal_error.contains("ordinary Agent"),
        "{terminal_error}"
    );
}

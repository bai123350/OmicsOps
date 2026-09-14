use chrono::Utc;
use omicsops_core::workspace::{Conversation, ModelProfile, Project, ProjectTemplate};
use omicsops_dto::{
    ComposerAttachmentReceipt, ComposerQueueActionRequestV4, ComposerQueueActionV4,
    ComposerQueueFrozenConfigV4, ComposerQueueMaterialSnapshotV4, ComposerQueueModeV4,
    ComposerQueueStatusV4, ComposerReference, EnqueueComposerTurnRequestV4,
    UpdateComposerQueueRequestV4,
};
use omicsops_protocol::{
    ApprovalPolicyV4, AutonomyModeV4, ComputeBackendKindV4, ComputeSelectionV4,
    ConversationAgentPreferencesV4, NetworkPolicyV4, RunServiceTierV4,
};
use omicsops_store::Store;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

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

const QUEUE_MODEL_PROFILE_ID: Uuid = Uuid::from_u128(0x100);

fn queue_profile() -> ModelProfile {
    ModelProfile {
        id: QUEUE_MODEL_PROFILE_ID,
        label: "queue profile".into(),
        provider: omicsops_core::workspace::ModelProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.com/v1".into(),
        model: "gpt-queue".into(),
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

fn request(
    project_id: Uuid,
    conversation_id: Uuid,
    message: impl Into<String>,
) -> EnqueueComposerTurnRequestV4 {
    EnqueueComposerTurnRequestV4 {
        request_id: Uuid::new_v4(),
        message_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        project_id,
        conversation_id,
        mode: ComposerQueueModeV4::Agent,
        message_markdown: message.into(),
        model_profile_id: QUEUE_MODEL_PROFILE_ID,
        compute_selection: selection(),
        references: vec![],
        attachments: vec![],
    }
}

fn frozen(request: &EnqueueComposerTurnRequestV4) -> ComposerQueueFrozenConfigV4 {
    ComposerQueueFrozenConfigV4 {
        model_profile_id: request.model_profile_id,
        model_configuration_hash: queue_profile().execution_configuration_hash(),
        conversation_preferences: ConversationAgentPreferencesV4::default(),
        service_tier: RunServiceTierV4 { fast_mode: None },
        delegated_model: None,
        reviewer_model: None,
        compute_selection: request.compute_selection.clone(),
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hash_json<T: Serialize>(value: &T) -> String {
    hash_bytes(&serde_json::to_vec(value).unwrap())
}

fn material(
    reference_context: &str,
    attachment_receipts: Vec<ComposerAttachmentReceipt>,
) -> ComposerQueueMaterialSnapshotV4 {
    ComposerQueueMaterialSnapshotV4 {
        reference_context: reference_context.into(),
        reference_context_sha256: hash_bytes(reference_context.as_bytes()),
        attachment_snapshot_sha256: hash_json(&attachment_receipts),
        attachment_receipts,
    }
}

fn receipt(
    project_id: Uuid,
    conversation_id: Uuid,
    id: Uuid,
    size_bytes: u64,
) -> ComposerAttachmentReceipt {
    ComposerAttachmentReceipt {
        id,
        project_id,
        conversation_id,
        name: "input.csv".into(),
        relative_path: format!(".omicsops/attachments/{id}/bytes.csv"),
        size_bytes,
        sha256: "b".repeat(64),
        media_type: "text/csv".into(),
    }
}

async fn fixture() -> (Store, Project, Conversation, Project, Conversation) {
    let store = Store::open_in_memory().await.unwrap();
    fixture_in(store).await
}

async fn fixture_in(store: Store) -> (Store, Project, Conversation, Project, Conversation) {
    let project = Project::new(
        Uuid::new_v4(),
        "queue project",
        r"C:\data\queue-project",
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
    store.save_model_profile(&queue_profile()).await.unwrap();
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "queue conversation", Utc::now());
    let other_conversation = Conversation::new(
        Uuid::new_v4(),
        other_project.id,
        "other conversation",
        Utc::now(),
    );
    store.save_conversation(&conversation).await.unwrap();
    store.save_conversation(&other_conversation).await.unwrap();
    (
        store,
        project,
        conversation,
        other_project,
        other_conversation,
    )
}

async fn replacement_fixture(
    store: &Store,
    project: Uuid,
    conversation: Uuid,
) -> omicsops_dto::ReplaceComposerTurnRequestV4 {
    use omicsops_protocol::{AgentEventKindV4, AgentEventV4, RunModeV4};
    let target = Uuid::new_v4();
    store
        .save_agent_run_v4(
            target,
            project,
            conversation,
            "running",
            &serde_json::json!({"status":"running"}),
        )
        .await
        .unwrap();
    let event = AgentEventV4::first(
        target,
        project,
        conversation,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&event).await.unwrap();
    omicsops_dto::ReplaceComposerTurnRequestV4 {
        turn: request(project, conversation, "replacement"),
        target_run_id: target,
        expected_event_sequence: event.sequence,
        expected_event_hash: event.event_hash,
        stop_request_id: Uuid::new_v4(),
    }
}

#[tokio::test]
async fn replacement_acceptance_is_atomic_idempotent_and_prioritized() {
    let (store, project, conversation, _, _) = fixture().await;
    let first = request(project.id, conversation.id, "first pending");
    let second = request(project.id, conversation.id, "second pending");
    let material = material("", vec![]);
    store
        .enqueue_composer_turn(&first, &frozen(&first), &material)
        .await
        .unwrap();
    store
        .enqueue_composer_turn(&second, &frozen(&second), &material)
        .await
        .unwrap();
    let replacement = replacement_fixture(&store, project.id, conversation.id).await;
    let receipt = store
        .replace_composer_turn(&replacement, &frozen(&replacement.turn), &material)
        .await
        .unwrap();
    assert_eq!(receipt.target_run_id, replacement.target_run_id);
    assert_eq!(receipt.stop.run_id, replacement.target_run_id);
    assert_eq!(
        store
            .replace_composer_turn(&replacement, &frozen(&replacement.turn), &material)
            .await
            .unwrap(),
        receipt
    );
    let items = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(
        items.iter().map(|item| item.request_id).collect::<Vec<_>>(),
        vec![
            replacement.turn.request_id,
            first.request_id,
            second.request_id
        ]
    );
    assert!(
        store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .ok()
            .flatten()
            .is_none()
    );
    let mut conflict = replacement.clone();
    conflict.turn.message_markdown = "different".into();
    assert!(
        store
            .replace_composer_turn(&conflict, &frozen(&conflict.turn), &material)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn replacement_rejection_never_stops_or_queues() {
    let (store, project, conversation, _, _) = fixture().await;
    let mut replacement = replacement_fixture(&store, project.id, conversation.id).await;
    replacement.expected_event_hash = "f".repeat(64);
    assert!(
        store
            .replace_composer_turn(
                &replacement,
                &frozen(&replacement.turn),
                &material("", vec![])
            )
            .await
            .is_err()
    );
    assert!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .get_run_stop_v4(project.id, conversation.id, replacement.target_run_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn replacement_accepts_normal_progress_after_the_observed_head() {
    use omicsops_protocol::{AgentEventKindV4, AgentEventV4, ToolEffectV4};
    let (store, project, conversation, _, _) = fixture().await;
    let replacement = replacement_fixture(&store, project.id, conversation.id).await;
    let previous = store
        .agent_events_v4(replacement.target_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let progress = AgentEventV4::next(
        &previous,
        Utc::now(),
        AgentEventKindV4::ToolDispatchStarted {
            call_id: "reading".into(),
            tool_id: "read".into(),
            effect: ToolEffectV4::ReadOnly,
            idempotency_key: "reading".into(),
        },
    );
    store.append_agent_event_v4(&progress).await.unwrap();
    let receipt = store
        .replace_composer_turn(
            &replacement,
            &frozen(&replacement.turn),
            &material("", vec![]),
        )
        .await
        .unwrap();
    assert_eq!(receipt.source_event_hash, replacement.expected_event_hash);
    assert_eq!(
        receipt.source_event_sequence,
        replacement.expected_event_sequence
    );
    assert_eq!(
        store
            .agent_events_v4(replacement.target_run_id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn replacement_waits_for_durable_safe_terminal_evidence() {
    use omicsops_protocol::{AgentEventKindV4, AgentEventV4, ToolEffectV4};
    for uncertain in [false, true] {
        let (store, project, conversation, _, _) = fixture().await;
        let replacement = replacement_fixture(&store, project.id, conversation.id).await;
        store
            .replace_composer_turn(
                &replacement,
                &frozen(&replacement.turn),
                &material("", vec![]),
            )
            .await
            .unwrap();
        let mut head = store
            .agent_events_v4(replacement.target_run_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        if uncertain {
            head = AgentEventV4::next(
                &head,
                Utc::now(),
                AgentEventKindV4::ToolDispatchStarted {
                    call_id: "remote-write".into(),
                    tool_id: "shell".into(),
                    idempotency_key: "remote-write".into(),
                    effect: ToolEffectV4::Mutating,
                },
            );
            store.append_agent_event_v4(&head).await.unwrap();
        }
        // A terminal status alone cannot release the replacement.
        store
            .save_agent_run_v4(
                replacement.target_run_id,
                project.id,
                conversation.id,
                "cancelled",
                &serde_json::json!({"status":"cancelled"}),
            )
            .await
            .unwrap();
        assert!(
            store
                .claim_next_composer_queue(project.id, conversation.id, Utc::now())
                .await
                .ok()
                .flatten()
                .is_none()
        );
        let terminal = AgentEventV4::next(&head, Utc::now(), AgentEventKindV4::RunCancelled);
        store.append_agent_event_v4(&terminal).await.unwrap();
        store
            .get_run_stop_v4(project.id, conversation.id, replacement.target_run_id)
            .await
            .unwrap();
        let claimed = store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .ok()
            .flatten();
        assert_eq!(claimed.is_some(), !uncertain);
        if let Some(claimed) = claimed {
            assert_eq!(claimed.item.request_id, replacement.turn.request_id);
        }
    }
}

#[tokio::test]
async fn replacement_payload_priority_and_stop_survive_queue_actions() {
    let (store, project, conversation, _, _) = fixture().await;
    let other = request(project.id, conversation.id, "pending");
    store
        .enqueue_composer_turn(&other, &frozen(&other), &material("", vec![]))
        .await
        .unwrap();
    let replacement = replacement_fixture(&store, project.id, conversation.id).await;
    store
        .replace_composer_turn(
            &replacement,
            &frozen(&replacement.turn),
            &material("", vec![]),
        )
        .await
        .unwrap();
    let items = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(
        items[0].replacement_target_run_id,
        Some(replacement.target_run_id)
    );
    for (item, action) in [
        (&items[0], ComposerQueueActionV4::MoveDown),
        (&items[1], ComposerQueueActionV4::MoveUp),
        (&items[0], ComposerQueueActionV4::CutIn),
    ] {
        assert!(
            store
                .composer_queue_action(&ComposerQueueActionRequestV4 {
                    project_id: project.id,
                    conversation_id: conversation.id,
                    request_id: item.request_id,
                    expected_revision: item.revision,
                    action
                })
                .await
                .is_err()
        );
    }
    assert!(
        store
            .update_composer_queue(
                &UpdateComposerQueueRequestV4 {
                    project_id: project.id,
                    conversation_id: conversation.id,
                    request_id: items[0].request_id,
                    expected_revision: items[0].revision,
                    message_markdown: "changed".into(),
                    references: vec![],
                    attachments: vec![]
                },
                &material("", vec![])
            )
            .await
            .is_err()
    );
    store
        .cancel_composer_queue(&ComposerQueueActionRequestV4 {
            project_id: project.id,
            conversation_id: conversation.id,
            request_id: items[0].request_id,
            expected_revision: items[0].revision,
            action: ComposerQueueActionV4::Cancel,
        })
        .await
        .unwrap();
    assert!(
        store
            .get_run_stop_v4(project.id, conversation.id, replacement.target_run_id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .find_composer_replacement_retry(&replacement)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn replacement_rolls_back_queue_when_stop_identity_conflicts() {
    let (store, project, conversation, other_project, other_conversation) = fixture().await;
    let other = replacement_fixture(&store, other_project.id, other_conversation.id).await;
    store
        .request_run_stop_v4(&omicsops_dto::StopRunRequestV4 {
            request_id: other.stop_request_id,
            project_id: other_project.id,
            conversation_id: other_conversation.id,
            run_id: other.target_run_id,
        })
        .await
        .unwrap();
    let mut replacement = replacement_fixture(&store, project.id, conversation.id).await;
    replacement.stop_request_id = other.stop_request_id;
    assert!(
        store
            .replace_composer_turn(
                &replacement,
                &frozen(&replacement.turn),
                &material("", vec![])
            )
            .await
            .is_err()
    );
    assert!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .get_run_stop_v4(project.id, conversation.id, replacement.target_run_id)
            .await
            .unwrap()
            .is_none()
    );
    let mut foreign = replacement.clone();
    foreign.target_run_id = other.target_run_id;
    assert!(
        store
            .replace_composer_turn(&foreign, &frozen(&foreign.turn), &material("", vec![]))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn concurrent_replacement_retries_accept_one_complete_payload() {
    let (store, project, conversation, _, _) = fixture().await;
    let mut replacement = replacement_fixture(&store, project.id, conversation.id).await;
    let file = receipt(project.id, conversation.id, Uuid::new_v4(), 4);
    replacement.turn.attachments.push(file.id);
    replacement
        .turn
        .references
        .push(ComposerReference::Skill { id: Uuid::new_v4() });
    let snapshot = material("Referenced evidence", vec![file.clone()]);
    let config = frozen(&replacement.turn);
    let (left, right) = tokio::join!(
        store.replace_composer_turn(&replacement, &config, &snapshot),
        store.replace_composer_turn(&replacement, &config, &snapshot)
    );
    assert_eq!(left.unwrap(), right.unwrap());
    let items = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].references, replacement.turn.references);
    assert_eq!(items[0].attachment_receipts, vec![file]);
    assert_eq!(items[0].frozen, config);
}

#[tokio::test]
async fn replacement_receipt_recovers_after_restart_without_reaccepting_mutable_config() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replacement.sqlite");
    let (store, project, conversation, _, _) = fixture_in(Store::open(&path).await.unwrap()).await;
    let replacement = replacement_fixture(&store, project.id, conversation.id).await;
    let receipt = store
        .replace_composer_turn(
            &replacement,
            &frozen(&replacement.turn),
            &material("", vec![]),
        )
        .await
        .unwrap();
    let mut changed = queue_profile();
    changed.model = "changed-model".into();
    store.save_model_profile(&changed).await.unwrap();
    drop(store);
    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .find_composer_replacement_retry(&replacement)
            .await
            .unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(
        reopened
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        reopened
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()[0]
            .replacement_receipt,
        Some(receipt)
    );
    assert!(
        reopened
            .get_run_stop_v4(project.id, conversation.id, replacement.target_run_id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        reopened
            .agent_events_v4(replacement.target_run_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn enqueue_is_pending_and_does_not_write_transcript_or_run_state() {
    let (store, project, conversation, _, _) = fixture().await;
    let request = request(project.id, conversation.id, "run QC");
    let frozen = frozen(&request);
    let material = material("", vec![]);

    let item = store
        .enqueue_composer_turn(&request, &frozen, &material)
        .await
        .unwrap();

    assert_eq!(item.status, ComposerQueueStatusV4::Pending);
    assert_eq!(item.position, 1);
    assert_eq!(item.revision, 1);
    assert_eq!(item.frozen, frozen);
    assert!(item.attachment_receipts.is_empty());
    assert_eq!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap(),
        vec![item]
    );

    for table in ["messages", "agent_runs_v4", "agent_events_v4"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "pending queue changed {table}");
    }
}

#[tokio::test]
async fn enqueue_rejects_empty_oversized_and_cross_project_payloads() {
    let (store, project, conversation, other_project, _) = fixture().await;
    let mut empty = request(project.id, conversation.id, "");
    empty.references.clear();
    empty.attachments.clear();
    let empty_error = store
        .enqueue_composer_turn(&empty, &frozen(&empty), &material("", vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(empty_error.contains("empty"), "{empty_error}");

    let oversized = request(project.id, conversation.id, "x".repeat(64 * 1024 + 1));
    let oversized_error = store
        .enqueue_composer_turn(&oversized, &frozen(&oversized), &material("", vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(oversized_error.contains("64 KiB"), "{oversized_error}");

    let mut foreign = request(project.id, conversation.id, "foreign reference");
    foreign.references = vec![ComposerReference::Project {
        project_id: other_project.id,
        id: other_project.id,
    }];
    let foreign_error = store
        .enqueue_composer_turn(&foreign, &frozen(&foreign), &material("", vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(foreign_error.contains("project"), "{foreign_error}");

    let mut too_many_references = request(project.id, conversation.id, "too many");
    too_many_references.references = (0..13)
        .map(|_| ComposerReference::Skill { id: Uuid::new_v4() })
        .collect();
    let references_error = store
        .enqueue_composer_turn(
            &too_many_references,
            &frozen(&too_many_references),
            &material("", vec![]),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(
        references_error.contains("references"),
        "{references_error}"
    );
}

#[tokio::test]
async fn duplicate_enqueue_returns_edited_row_without_rewriting_original_hash() {
    let (store, project, conversation, _, _) = fixture().await;
    let request = request(project.id, conversation.id, "original");
    let frozen = frozen(&request);
    let material = material("", vec![]);
    let first = store
        .enqueue_composer_turn(&request, &frozen, &material)
        .await
        .unwrap();
    let original_hash = hash_json(&request);

    let update = UpdateComposerQueueRequestV4 {
        project_id: project.id,
        conversation_id: conversation.id,
        request_id: request.request_id,
        expected_revision: first.revision,
        message_markdown: "edited while waiting".into(),
        references: request.references.clone(),
        attachments: request.attachments.clone(),
    };
    let edited = store
        .update_composer_queue(&update, &material)
        .await
        .unwrap();
    assert_eq!(edited.revision, 2);

    let retried = store
        .enqueue_composer_turn(&request, &frozen, &material)
        .await
        .unwrap();
    assert_eq!(retried.request_id, request.request_id);
    assert_eq!(retried.message_markdown, "edited while waiting");
    assert_eq!(retried.revision, edited.revision);

    sqlx::query("DELETE FROM model_profiles WHERE id=?1")
        .bind(request.model_profile_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let found = store
        .find_composer_enqueue_retry(&request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found, retried);

    let stored_hash: String =
        sqlx::query_scalar("SELECT request_hash FROM composer_queue_v4 WHERE request_id=?1")
            .bind(request.request_id.to_string())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(stored_hash, original_hash);

    let mut conflicting = request.clone();
    conflicting.message_markdown = "different logical request".into();
    let conflict_error = store
        .enqueue_composer_turn(&conflicting, &frozen, &material)
        .await
        .unwrap_err()
        .to_string();
    assert!(conflict_error.contains("request id"), "{conflict_error}");
}

#[tokio::test]
async fn enqueue_rejects_untrusted_attachment_paths_and_size_overflows() {
    let (store, project, conversation, _, _) = fixture().await;
    let attachment_id = Uuid::new_v4();
    let mut request = request(project.id, conversation.id, "attach");
    request.attachments = vec![attachment_id];

    let mut invalid_path = receipt(project.id, conversation.id, attachment_id, 1);
    invalid_path.relative_path = format!(".omicsops/attachments/{attachment_id}/../../outside.csv");
    let path_error = store
        .enqueue_composer_turn(
            &request,
            &frozen(&request),
            &material("", vec![invalid_path]),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(path_error.contains("path"), "{path_error}");

    let too_large = store
        .enqueue_composer_turn(
            &request,
            &frozen(&request),
            &material(
                "",
                vec![receipt(
                    project.id,
                    conversation.id,
                    attachment_id,
                    20 * 1024 * 1024 + 1,
                )],
            ),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(too_large.contains("per-file"), "{too_large}");

    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    request.attachments = ids.to_vec();
    let total_error = store
        .enqueue_composer_turn(
            &request,
            &frozen(&request),
            &material(
                "",
                ids.into_iter()
                    .map(|id| receipt(project.id, conversation.id, id, 20 * 1024 * 1024))
                    .collect(),
            ),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(total_error.contains("total"), "{total_error}");

    assert!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn enqueue_rejects_a_stale_frozen_profile_before_inserting() {
    let (store, project, conversation, _, _) = fixture().await;
    let request = request(project.id, conversation.id, "profile race");
    let frozen_snapshot = frozen(&request);
    let mut changed = queue_profile();
    changed.model = "gpt-queue-changed".into();
    store.save_model_profile(&changed).await.unwrap();

    let error = store
        .enqueue_composer_turn(&request, &frozen_snapshot, &material("", vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("profile changed"), "{error}");
    assert!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn pending_updates_are_scoped_and_compare_and_swap_by_revision() {
    let (store, project, conversation, other_project, other_conversation) = fixture().await;
    let request = request(project.id, conversation.id, "original");
    let frozen = frozen(&request);
    let first = store
        .enqueue_composer_turn(&request, &frozen, &material("", vec![]))
        .await
        .unwrap();

    let stale = UpdateComposerQueueRequestV4 {
        project_id: project.id,
        conversation_id: conversation.id,
        request_id: request.request_id,
        expected_revision: 99,
        message_markdown: "stale".into(),
        references: vec![],
        attachments: vec![],
    };
    let stale_error = store
        .update_composer_queue(&stale, &material("", vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(stale_error.contains("revision"), "{stale_error}");

    let wrong_scope = UpdateComposerQueueRequestV4 {
        project_id: other_project.id,
        conversation_id: other_conversation.id,
        ..stale.clone()
    };
    let scope_error = store
        .update_composer_queue(&wrong_scope, &material("", vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        scope_error.contains("scope") || scope_error.contains("conversation"),
        "{scope_error}"
    );

    let update = UpdateComposerQueueRequestV4 {
        expected_revision: first.revision,
        message_markdown: "fresh".into(),
        ..stale
    };
    let updated = store
        .update_composer_queue(&update, &material("", vec![]))
        .await
        .unwrap();
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.message_markdown, "fresh");
}

#[tokio::test]
async fn cancel_is_pending_only_and_preserves_the_full_payload() {
    let (store, project, conversation, _, _) = fixture().await;
    let attachment_id = Uuid::new_v4();
    let request = request(project.id, conversation.id, "cancel me");
    let mut request = request;
    request.references = vec![ComposerReference::Skill { id: Uuid::new_v4() }];
    request.attachments = vec![attachment_id];
    let receipt = ComposerAttachmentReceipt {
        id: attachment_id,
        project_id: project.id,
        conversation_id: conversation.id,
        name: "input.csv".into(),
        relative_path: format!(".omicsops/attachments/{attachment_id}/bytes.csv"),
        size_bytes: 4,
        sha256: "b".repeat(64),
        media_type: "text/csv".into(),
    };
    let first = store
        .enqueue_composer_turn(
            &request,
            &frozen(&request),
            &material("bounded reference", vec![receipt.clone()]),
        )
        .await
        .unwrap();
    let cancelled = store
        .cancel_composer_queue(&ComposerQueueActionRequestV4 {
            project_id: project.id,
            conversation_id: conversation.id,
            request_id: request.request_id,
            expected_revision: first.revision,
            action: ComposerQueueActionV4::Cancel,
        })
        .await
        .unwrap();
    assert_eq!(cancelled.status, ComposerQueueStatusV4::Cancelled);
    assert_eq!(cancelled.references, request.references);
    assert_eq!(cancelled.attachments, request.attachments);
    assert_eq!(cancelled.attachment_receipts, vec![receipt]);

    let update_error = store
        .update_composer_queue(
            &UpdateComposerQueueRequestV4 {
                project_id: project.id,
                conversation_id: conversation.id,
                request_id: request.request_id,
                expected_revision: cancelled.revision,
                message_markdown: "must remain".into(),
                references: vec![],
                attachments: vec![],
            },
            &material("", vec![]),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(update_error.contains("pending"), "{update_error}");
}

#[tokio::test]
async fn moving_pending_rows_swaps_positions_without_unique_conflicts() {
    let (store, project, conversation, _, _) = fixture().await;
    let first_request = request(project.id, conversation.id, "first");
    let second_request = request(project.id, conversation.id, "second");
    let third_request = request(project.id, conversation.id, "third");
    let first = store
        .enqueue_composer_turn(
            &first_request,
            &frozen(&first_request),
            &material("", vec![]),
        )
        .await
        .unwrap();
    let second = store
        .enqueue_composer_turn(
            &second_request,
            &frozen(&second_request),
            &material("", vec![]),
        )
        .await
        .unwrap();
    let third = store
        .enqueue_composer_turn(
            &third_request,
            &frozen(&third_request),
            &material("", vec![]),
        )
        .await
        .unwrap();

    let moved_up = store
        .move_composer_queue(&ComposerQueueActionRequestV4 {
            project_id: project.id,
            conversation_id: conversation.id,
            request_id: second_request.request_id,
            expected_revision: second.revision,
            action: ComposerQueueActionV4::MoveUp,
        })
        .await
        .unwrap();
    assert_eq!(
        moved_up
            .iter()
            .map(|item| item.message_markdown.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first", "third"]
    );
    let moved_first = moved_up
        .iter()
        .find(|item| item.request_id == first.request_id)
        .unwrap();
    let moved_second = moved_up
        .iter()
        .find(|item| item.request_id == second.request_id)
        .unwrap();
    assert_eq!(moved_first.revision, 2);
    assert_eq!(moved_second.revision, 2);
    assert_eq!(
        moved_up
            .iter()
            .map(|item| item.position)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    let moved_down = store
        .move_composer_queue(&ComposerQueueActionRequestV4 {
            project_id: project.id,
            conversation_id: conversation.id,
            request_id: second_request.request_id,
            expected_revision: moved_second.revision,
            action: ComposerQueueActionV4::MoveDown,
        })
        .await
        .unwrap();
    assert_eq!(
        moved_down
            .iter()
            .map(|item| item.message_markdown.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
    assert_eq!(
        moved_down
            .iter()
            .find(|item| item.request_id == third.request_id)
            .unwrap()
            .revision,
        third.revision
    );

    let stale_error = store
        .move_composer_queue(&ComposerQueueActionRequestV4 {
            project_id: project.id,
            conversation_id: conversation.id,
            request_id: second_request.request_id,
            expected_revision: moved_second.revision,
            action: ComposerQueueActionV4::MoveUp,
        })
        .await
        .unwrap_err()
        .to_string();
    assert!(stale_error.contains("revision"), "{stale_error}");
}

#[tokio::test]
async fn queue_rejects_the_101st_non_terminal_item() {
    let (store, project, conversation, _, _) = fixture().await;
    for index in 0..100 {
        let item = request(project.id, conversation.id, format!("queued {index}"));
        store
            .enqueue_composer_turn(&item, &frozen(&item), &material("", vec![]))
            .await
            .unwrap();
    }
    let overflow = request(project.id, conversation.id, "overflow");
    let error = store
        .enqueue_composer_turn(&overflow, &frozen(&overflow), &material("", vec![]))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("100"), "{error}");
    assert_eq!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .len(),
        100
    );
}

#[tokio::test]
async fn concurrent_enqueue_calls_receive_distinct_fifo_positions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("queue.sqlite");
    let store = Store::open(&path).await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "queue project",
        r"C:\data\queue-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "queue", Utc::now());
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    store.save_model_profile(&queue_profile()).await.unwrap();
    let left = request(project.id, conversation.id, "left");
    let right = request(project.id, conversation.id, "right");
    let left_frozen = frozen(&left);
    let right_frozen = frozen(&right);
    let left_material = material("", vec![]);
    let right_material = material("", vec![]);
    let left_store = store.clone();
    let right_store = store.clone();
    let (left_result, right_result) = tokio::join!(
        left_store.enqueue_composer_turn(&left, &left_frozen, &left_material),
        right_store.enqueue_composer_turn(&right, &right_frozen, &right_material),
    );
    left_result.unwrap();
    right_result.unwrap();
    let positions = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.position)
        .collect::<Vec<_>>();
    assert_eq!(positions, vec![1, 2]);
}

#[tokio::test]
async fn queue_rows_survive_store_reopen_and_cascade_on_conversation_delete() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("queue.sqlite");
    let project = Project::new(
        Uuid::new_v4(),
        "queue project",
        r"C:\data\queue-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "queue", Utc::now());
    let store = Store::open(&path).await.unwrap();
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    store.save_model_profile(&queue_profile()).await.unwrap();
    let queued = request(project.id, conversation.id, "persist me");
    store
        .enqueue_composer_turn(&queued, &frozen(&queued), &material("", vec![]))
        .await
        .unwrap();
    drop(store);

    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        reopened
            .delete_conversation(project.id, conversation.id)
            .await
            .unwrap()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM composer_queue_v4")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

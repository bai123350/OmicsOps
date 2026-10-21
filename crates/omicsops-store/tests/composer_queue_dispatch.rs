use chrono::{Duration, Utc};
use omicsops_core::workspace::{
    Conversation, Message, MessageRole, ModelProfile, ModelProviderKind, Project, ProjectTemplate,
};
use omicsops_dto::{
    ComposerQueueFailureCodeV4, ComposerQueueFrozenConfigV4, ComposerQueueItemV4,
    ComposerQueueMaterialSnapshotV4, ComposerQueueModeV4, ComposerQueueStatusV4,
    EnqueueComposerTurnRequestV4,
};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ApprovalPolicyV4, AutonomyModeV4, ComputeBackendKindV4,
    ComputeSelectionV4, ConversationAgentPreferencesV4, NetworkPolicyV4, RunModeV4,
    RunServiceTierV4, ToolEffectV4,
};
use omicsops_store::Store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
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

fn profile(model: &str) -> ModelProfile {
    ModelProfile {
        id: PROFILE_ID,
        label: "dispatch test profile".into(),
        provider: ModelProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.com/v1".into(),
        model: model.into(),
        credential_reference: None,
        supports_tools: true,
        supports_vision: false,
        context_window_tokens: None,
        catalog_capabilities: None,
        reasoning_effort: None,
        fast_mode: None,
        delegated_model_profile_id: None,
        cli_executable: None,
        subscription_account_ref: None,
    }
}

fn request(
    project_id: Uuid,
    conversation_id: Uuid,
    mode: ComposerQueueModeV4,
    text: &str,
) -> EnqueueComposerTurnRequestV4 {
    EnqueueComposerTurnRequestV4 {
        request_id: Uuid::new_v4(),
        message_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        project_id,
        conversation_id,
        mode,
        message_markdown: text.into(),
        model_profile_id: PROFILE_ID,
        compute_selection: selection(),
        references: vec![],
        attachments: vec![],
    }
}

fn frozen(request: &EnqueueComposerTurnRequestV4) -> ComposerQueueFrozenConfigV4 {
    ComposerQueueFrozenConfigV4 {
        model_profile_id: request.model_profile_id,
        model_configuration_hash: profile("dispatch-test").execution_configuration_hash(),
        conversation_preferences: ConversationAgentPreferencesV4::default(),
        service_tier: RunServiceTierV4 { fast_mode: None },
        delegated_model: None,
        reviewer_model: None,
        compute_selection: request.compute_selection.clone(),
    }
}

fn material(reference_context: &str) -> ComposerQueueMaterialSnapshotV4 {
    let hash = hex::encode(Sha256::digest(reference_context.as_bytes()));
    ComposerQueueMaterialSnapshotV4 {
        reference_context: reference_context.into(),
        reference_context_sha256: hash,
        attachment_receipts: vec![],
        attachment_snapshot_sha256: hex::encode(Sha256::digest(b"[]")),
    }
}

fn plan_value(item: &ComposerQueueItemV4, material: &ComposerQueueMaterialSnapshotV4) -> Value {
    json!({
        "run_id": item.run_id,
        "project_id": item.project_id,
        "conversation_id": item.conversation_id,
        "model_profile_id": item.frozen.model_profile_id,
        "objective": item.message_markdown,
        "reference_context": material.reference_context,
        "model_configuration_hash": item.frozen.model_configuration_hash,
        "conversation_preferences": item.frozen.conversation_preferences,
        "service_tier": item.frozen.service_tier,
        "compute_selection": item.frozen.compute_selection,
        "reviewer_model": item.frozen.reviewer_model,
        "delegated_model": item.frozen.delegated_model,
    })
}

fn plan_event(item: &ComposerQueueItemV4, at: chrono::DateTime<Utc>) -> AgentEventV4 {
    AgentEventV4::first(
        item.run_id,
        item.project_id,
        item.conversation_id,
        at,
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    )
}

async fn fixture() -> (Store, Project, Conversation) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "dispatch project",
        r"C:\data\dispatch-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "dispatch conversation",
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    store
        .save_model_profile(&profile("dispatch-test"))
        .await
        .unwrap();
    (store, project, conversation)
}

async fn enqueue(
    store: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
    mode: ComposerQueueModeV4,
    text: &str,
    reference_context: &str,
) -> ComposerQueueItemV4 {
    let request = request(project_id, conversation_id, mode, text);
    let frozen = frozen(&request);
    store
        .enqueue_composer_turn(&request, &frozen, &material(reference_context))
        .await
        .unwrap()
}

#[tokio::test]
async fn concurrent_claim_has_one_fifo_winner() {
    let directory = tempdir().unwrap();
    let store = Store::open(directory.path().join("dispatch.sqlite"))
        .await
        .unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "dispatch project",
        r"C:\data\dispatch-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "dispatch conversation",
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_conversation(&conversation).await.unwrap();
    store
        .save_model_profile(&profile("dispatch-test"))
        .await
        .unwrap();
    enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "first",
        "",
    )
    .await;
    enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "second",
        "",
    )
    .await;

    let now = Utc::now();
    let left = store.clone();
    let right = store.clone();
    let (left, right) = tokio::join!(
        left.claim_next_composer_queue(project.id, conversation.id, now),
        right.claim_next_composer_queue(project.id, conversation.id, now),
    );
    let winners = [left.unwrap(), right.unwrap()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0].item.message_markdown, "first");
    assert!(
        store
            .claim_next_composer_queue(project.id, conversation.id, now)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn expired_lease_reconciles_to_pending_and_revokes_old_token() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "reconcile me",
        "",
    )
    .await;
    let claimed_at = Utc::now();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, claimed_at)
        .await
        .unwrap()
        .unwrap();
    let expired_at = lease.expires_at + Duration::seconds(1);
    let stale = store
        .release_preparation(&lease, expired_at, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(stale.contains("stale"), "{stale}");

    let items = store
        .reconcile_composer_queue(project.id, conversation.id, expired_at)
        .await
        .unwrap();
    assert_eq!(items[0].request_id, item.request_id);
    assert_eq!(items[0].status, ComposerQueueStatusV4::Pending);
    assert_eq!(items[0].failure_code, None);
    assert!(
        store
            .release_preparation(&lease, expired_at, None)
            .await
            .is_err()
    );
    let replacement = store
        .claim_next_composer_queue(
            project.id,
            conversation.id,
            expired_at + Duration::seconds(1),
        )
        .await
        .unwrap()
        .unwrap();
    assert_ne!(replacement.lease_token, lease.lease_token);
    assert!(replacement.item.revision > lease.item.revision);
}

#[tokio::test]
async fn explicit_release_is_token_fenced_and_can_record_safe_failure_code() {
    let (store, project, conversation) = fixture().await;
    enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "release me",
        "",
    )
    .await;
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    let pending = store
        .release_composer_queue_preparation(&lease, Utc::now(), None)
        .await
        .unwrap();
    assert_eq!(pending.status, ComposerQueueStatusV4::Pending);
    assert_eq!(pending.failure_code, None);

    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    let failed = store
        .release_composer_queue_preparation(
            &lease,
            Utc::now(),
            Some(ComposerQueueFailureCodeV4::ConfigurationChanged),
        )
        .await
        .unwrap();
    assert_eq!(failed.status, ComposerQueueStatusV4::Failed);
    assert_eq!(
        failed.failure_code,
        Some(ComposerQueueFailureCodeV4::ConfigurationChanged)
    );
    assert!(
        store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn release_refuses_any_reserved_side_effect() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "side effect",
        "",
    )
    .await;
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    sqlx::query(
        "INSERT INTO messages(id,frame_id,project_id,conversation_id,seq,role,content,ts)
         VALUES (?1,?2,?3,?2,1,'user','reserved',0)",
    )
    .bind(item.message_id.to_string())
    .bind(conversation.id.to_string())
    .bind(project.id.to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let error = store
        .release_preparation(&lease, Utc::now(), None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("side-effect"), "{error}");
    let persisted = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(persisted[0].status, ComposerQueueStatusV4::Dispatching);
}

#[tokio::test]
async fn plan_commit_is_atomic_and_seeds_plan_revision() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "make a plan",
        "bounded context",
    )
    .await;
    let now = Utc::now();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, now)
        .await
        .unwrap()
        .unwrap();
    let value = plan_value(&item, &material("bounded context"));
    let first = plan_event(&item, now);
    let committed = store
        .commit_composer_queue_dispatch(&lease, &value, &[first], now)
        .await
        .unwrap();
    assert_eq!(committed.status, ComposerQueueStatusV4::Running);
    for table in [
        "messages",
        "agent_runs_v4",
        "agent_events_v4",
        "proposed_plans",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 1, "plan dispatch did not seed {table}");
    }
}

#[tokio::test]
async fn commit_uses_db_snapshot_instead_of_tampered_lease_payload() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "authoritative text",
        "stored context",
    )
    .await;
    let now = Utc::now();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, now)
        .await
        .unwrap()
        .unwrap();
    let mut tampered = lease.clone();
    tampered.item.message_markdown = "attacker text".into();
    tampered.material.reference_context = "attacker context".into();
    let value = plan_value(&item, &material("stored context"));
    let first = plan_event(&item, now);
    store
        .commit_composer_queue_dispatch(&tampered, &value, &[first], now)
        .await
        .unwrap();
    let content: String = sqlx::query_scalar("SELECT content FROM messages WHERE id=?1")
        .bind(item.message_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(content, "authoritative text");
}

#[tokio::test]
async fn tampered_lease_scope_cannot_retarget_a_claim() {
    let (store, project, conversation) = fixture().await;
    let other_project = Project::new(
        Uuid::new_v4(),
        "other dispatch project",
        r"C:\data\other-dispatch-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let other_conversation = Conversation::new(
        Uuid::new_v4(),
        other_project.id,
        "other dispatch conversation",
        Utc::now(),
    );
    store.save_project(&other_project).await.unwrap();
    store.save_conversation(&other_conversation).await.unwrap();
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "scope-bound",
        "",
    )
    .await;
    let now = Utc::now();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, now)
        .await
        .unwrap()
        .unwrap();
    let mut tampered = lease.clone();
    tampered.item.project_id = other_project.id;
    tampered.item.conversation_id = other_conversation.id;
    let value = plan_value(&item, &material(""));
    let first = plan_event(&item, now);
    let error = store
        .commit_composer_queue_dispatch(&tampered, &value, &[first], now)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("stale"), "{error}");
    assert!(
        store
            .list_composer_queue(project.id, conversation.id)
            .await
            .unwrap()
            .iter()
            .all(|queued| queued.status == ComposerQueueStatusV4::Dispatching)
    );
}

#[tokio::test]
async fn commit_rolls_back_message_when_reserved_run_collision_is_detected() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "rollback me",
        "",
    )
    .await;
    let now = Utc::now();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, now)
        .await
        .unwrap()
        .unwrap();
    sqlx::query(
        "INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json)
         VALUES (?1,?2,?3,'completed','{}')",
    )
    .bind(item.run_id.to_string())
    .bind(project.id.to_string())
    .bind(conversation.id.to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let value = plan_value(&item, &material(""));
    let first = plan_event(&item, now);
    assert!(
        store
            .commit_composer_queue_dispatch(&lease, &value, &[first], now)
            .await
            .is_err()
    );
    for table in ["messages", "agent_events_v4", "proposed_plans"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "failed commit left {table}");
    }
    let queue = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(queue[0].status, ComposerQueueStatusV4::Dispatching);
}

#[tokio::test]
async fn changed_profile_is_rejected_before_any_dispatch_side_effect() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "profile race",
        "",
    )
    .await;
    let now = Utc::now();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, now)
        .await
        .unwrap()
        .unwrap();
    store
        .save_model_profile(&profile("changed-model"))
        .await
        .unwrap();
    let value = plan_value(&item, &material(""));
    let first = plan_event(&item, now);
    let error = store
        .commit_composer_queue_dispatch(&lease, &value, &[first], now)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("profile changed"), "{error}");
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(messages, 0);
}

#[tokio::test]
async fn needs_attention_run_blocks_fifo_claim() {
    let (store, project, conversation) = fixture().await;
    enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "blocked",
        "",
    )
    .await;
    let active_run = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json)
         VALUES (?1,?2,?3,'needs_attention','{}')",
    )
    .bind(active_run.to_string())
    .bind(project.id.to_string())
    .bind(conversation.id.to_string())
    .execute(store.pool())
    .await
    .unwrap();
    assert!(
        store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("DELETE FROM agent_runs_v4 WHERE run_id=?1")
        .bind(active_run.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn safe_needs_attention_without_queue_does_not_block_claim_or_commit() {
    let (store, project, conversation) = fixture().await;
    let pending = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "next",
        "",
    )
    .await;
    let old_run = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,'needs_attention','{}')")
        .bind(old_run.to_string()).bind(project.id.to_string()).bind(conversation.id.to_string())
        .execute(store.pool()).await.unwrap();
    let created = AgentEventV4::first(
        old_run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&created).await.unwrap();
    let attention = AgentEventV4::next(
        &created,
        Utc::now(),
        AgentEventKindV4::RunNeedsAttention {
            message: "context limit".into(),
        },
    );
    store.append_agent_event_v4(&attention).await.unwrap();

    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .expect("safe historical run must allow FIFO");
    assert_eq!(lease.item.request_id, pending.request_id);
    let now = Utc::now();
    store
        .commit_composer_queue_dispatch(
            &lease,
            &plan_value(&pending, &material("")),
            &[plan_event(&pending, now)],
            now,
        )
        .await
        .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
        .bind(old_run.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(status, "needs_attention");
    assert_eq!(
        store.agent_events_v4(old_run).await.unwrap(),
        vec![created, attention]
    );
}

#[tokio::test]
async fn attention_evidence_fail_closed_and_read_only_dispatch_is_safe() {
    for effect in [
        ToolEffectV4::ReadOnly,
        ToolEffectV4::Mutating,
        ToolEffectV4::Runtime,
        ToolEffectV4::Network,
        ToolEffectV4::Delegation,
    ] {
        let (store, project, conversation) = fixture().await;
        let run = Uuid::new_v4();
        sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,'needs_attention','{}')")
            .bind(run.to_string()).bind(project.id.to_string()).bind(conversation.id.to_string()).execute(store.pool()).await.unwrap();
        let first = AgentEventV4::first(
            run,
            project.id,
            conversation.id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        store.append_agent_event_v4(&first).await.unwrap();
        let dispatch = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ToolDispatchStarted {
                call_id: "call".into(),
                tool_id: "tool".into(),
                effect,
                idempotency_key: "call".into(),
            },
        );
        store.append_agent_event_v4(&dispatch).await.unwrap();
        let terminal = AgentEventV4::next(
            &dispatch,
            Utc::now(),
            AgentEventKindV4::RunNeedsAttention {
                message: "context".into(),
            },
        );
        store.append_agent_event_v4(&terminal).await.unwrap();
        assert_eq!(
            store
                .safe_needs_attention_run(project.id, conversation.id, run)
                .await
                .unwrap(),
            effect == ToolEffectV4::ReadOnly,
            "{effect:?}"
        );
    }
    let (store, project, conversation) = fixture().await;
    let run = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,'needs_attention','{}')")
        .bind(run.to_string()).bind(project.id.to_string()).bind(conversation.id.to_string()).execute(store.pool()).await.unwrap();
    let first = AgentEventV4::first(
        run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    let uncertain = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::ToolDispatchUncertain {
            call_id: "orphan".into(),
            tool_id: "tool".into(),
        },
    );
    store.append_agent_event_v4(&uncertain).await.unwrap();
    let terminal = AgentEventV4::next(
        &uncertain,
        Utc::now(),
        AgentEventKindV4::RunNeedsAttention {
            message: "context".into(),
        },
    );
    store.append_agent_event_v4(&terminal).await.unwrap();
    assert!(
        !store
            .safe_needs_attention_run(project.id, conversation.id, run)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn attention_status_requires_valid_scoped_terminal_chain() {
    for damage in ["status_running", "hash", "scope"] {
        let (store, project, conversation) = fixture().await;
        let run = Uuid::new_v4();
        sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,'needs_attention','{}')")
            .bind(run.to_string()).bind(project.id.to_string()).bind(conversation.id.to_string()).execute(store.pool()).await.unwrap();
        let first = AgentEventV4::first(
            run,
            project.id,
            conversation.id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        store.append_agent_event_v4(&first).await.unwrap();
        let terminal = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::RunNeedsAttention {
                message: "context".into(),
            },
        );
        store.append_agent_event_v4(&terminal).await.unwrap();
        match damage {
            "status_running" => {
                sqlx::query("UPDATE agent_runs_v4 SET status='running' WHERE run_id=?1")
                    .bind(run.to_string())
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
            "hash" => {
                sqlx::query(
                    "UPDATE agent_events_v4 SET value_json='{}' WHERE run_id=?1 AND sequence=2",
                )
                .bind(run.to_string())
                .execute(store.pool())
                .await
                .unwrap();
            }
            "scope" => {
                let other = Conversation::new(Uuid::new_v4(), project.id, "other", Utc::now());
                store.save_conversation(&other).await.unwrap();
                sqlx::query(
                    "UPDATE agent_events_v4 SET conversation_id=?1 WHERE run_id=?2 AND sequence=2",
                )
                .bind(other.id.to_string())
                .bind(run.to_string())
                .execute(store.pool())
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            !store
                .safe_needs_attention_run(project.id, conversation.id, run)
                .await
                .unwrap(),
            "{damage}"
        );
        enqueue(
            &store,
            project.id,
            conversation.id,
            ComposerQueueModeV4::Plan,
            "blocked",
            "",
        )
        .await;
        assert!(
            store
                .claim_next_composer_queue(project.id, conversation.id, Utc::now())
                .await
                .unwrap()
                .is_none(),
            "{damage}"
        );
    }
}

#[tokio::test]
async fn reopened_safe_needs_attention_queue_settles_and_four_pending_remain_fifo() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("attention-restart.sqlite");
    let (project, conversation, old, pending, evidence) = {
        let store = Store::open(&path).await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "restart project",
            r"C:\data\restart-project",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        let conversation = Conversation::new(
            Uuid::new_v4(),
            project.id,
            "restart conversation",
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        store.save_conversation(&conversation).await.unwrap();
        store
            .save_model_profile(&profile("dispatch-test"))
            .await
            .unwrap();
        let old = enqueue(
            &store,
            project.id,
            conversation.id,
            ComposerQueueModeV4::Plan,
            "old",
            "",
        )
        .await;
        let now = Utc::now();
        let lease = store
            .claim_next_composer_queue(project.id, conversation.id, now)
            .await
            .unwrap()
            .unwrap();
        let created = plan_event(&old, now);
        store
            .commit_composer_queue_dispatch(
                &lease,
                &plan_value(&old, &material("")),
                &[created.clone()],
                now,
            )
            .await
            .unwrap();
        let mut pending = Vec::new();
        for i in 1..=4 {
            pending.push(
                enqueue(
                    &store,
                    project.id,
                    conversation.id,
                    ComposerQueueModeV4::Plan,
                    &format!("queued {i}"),
                    "",
                )
                .await,
            );
        }
        let attention = AgentEventV4::next(
            &created,
            now + Duration::seconds(1),
            AgentEventKindV4::RunNeedsAttention {
                message: "context limit".into(),
            },
        );
        store.append_agent_event_v4(&attention).await.unwrap();
        sqlx::query("UPDATE agent_runs_v4 SET status='needs_attention' WHERE run_id=?1")
            .bind(old.run_id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE proposed_plans SET status='cancelled' WHERE run_id=?1")
            .bind(old.run_id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        // Mimic an older database in which the event observer left the queue row running.
        sqlx::query(
            "UPDATE composer_queue_v4 SET status='running',failure_code=NULL WHERE request_id=?1",
        )
        .bind(old.request_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
        (
            project,
            conversation,
            old,
            pending,
            vec![created, attention],
        )
    };
    let store = Store::open(&path).await.unwrap();
    let rows = store
        .reconcile_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Failed);
    assert_eq!(
        rows[0].failure_code,
        Some(ComposerQueueFailureCodeV4::RunFailed)
    );
    for (row, expected) in rows.iter().skip(1).zip(&pending) {
        assert_eq!(
            (
                row.request_id,
                row.message_id,
                row.run_id,
                row.message_markdown.as_str(),
                row.status
            ),
            (
                expected.request_id,
                expected.message_id,
                expected.run_id,
                expected.message_markdown.as_str(),
                ComposerQueueStatusV4::Pending
            )
        );
    }
    assert_eq!(
        store
            .reconcile_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap(),
        rows
    );
    assert_eq!(store.agent_events_v4(old.run_id).await.unwrap(), evidence);
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.item.request_id, pending[0].request_id);
    let now = Utc::now();
    store
        .commit_composer_queue_dispatch(
            &lease,
            &plan_value(&pending[0], &material("")),
            &[plan_event(&pending[0], now)],
            now,
        )
        .await
        .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
        .bind(old.run_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(status, "needs_attention");
}

#[tokio::test]
async fn contradictory_expired_side_effects_become_uncertain() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "contradictory",
        "",
    )
    .await;
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    sqlx::query(
        "INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json)
         VALUES (?1,?2,?3,'running','{}')",
    )
    .bind(item.run_id.to_string())
    .bind(project.id.to_string())
    .bind(conversation.id.to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let rows = store
        .reconcile_composer_queue(
            project.id,
            conversation.id,
            lease.expires_at + Duration::seconds(1),
        )
        .await
        .unwrap();
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Uncertain);
    assert_eq!(
        rows[0].failure_code,
        Some(ComposerQueueFailureCodeV4::LeaseUncertain)
    );
}

#[tokio::test]
async fn public_terminal_event_updates_linked_queue_atomically() {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Plan,
        "terminal hook",
        "",
    )
    .await;
    let now = Utc::now();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, now)
        .await
        .unwrap()
        .unwrap();
    let first = plan_event(&item, now);
    let value = plan_value(&item, &material(""));
    store
        .commit_composer_queue_dispatch(&lease, &value, std::slice::from_ref(&first), now)
        .await
        .unwrap();
    let terminal = AgentEventV4::next(
        &first,
        now + Duration::seconds(1),
        AgentEventKindV4::RunCancelled,
    );
    store.append_agent_event_v4(&terminal).await.unwrap();
    let queue = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(queue[0].status, ComposerQueueStatusV4::Cancelled);
    assert_eq!(
        queue[0].failure_code,
        Some(ComposerQueueFailureCodeV4::CancelledByUser)
    );
}

async fn stopped_runtime_queue_fixture(
    resolved_before_stop: bool,
) -> (
    Store,
    Project,
    Conversation,
    ComposerQueueItemV4,
    ComposerQueueItemV4,
) {
    let (store, project, conversation) = fixture().await;
    let item = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "inspect environment",
        "",
    )
    .await;
    let pending = enqueue(
        &store,
        project.id,
        conversation.id,
        ComposerQueueModeV4::Agent,
        "next task",
        "",
    )
    .await;
    // Seed an already dispatched ordinary turn; dispatch preparation is covered
    // separately. These tests exercise Stop and its durable queue projection.
    store
        .save_message(&Message::markdown(
            item.message_id,
            project.id,
            conversation.id,
            1,
            MessageRole::User,
            &item.message_markdown,
            Utc::now(),
        ))
        .await
        .unwrap();
    store
        .save_agent_run_v4(
            item.run_id,
            project.id,
            conversation.id,
            "running",
            &json!({"run_id":item.run_id,"project_id":project.id,"conversation_id":conversation.id,"status":"running"}),
        )
        .await
        .unwrap();
    sqlx::query("UPDATE composer_queue_v4 SET status='running' WHERE request_id=?1")
        .bind(item.request_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let created = AgentEventV4::first(
        item.run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&created).await.unwrap();
    let started = AgentEventV4::next(
        &created,
        Utc::now(),
        AgentEventKindV4::ToolDispatchStarted {
            call_id: "runtime-inspection".into(),
            tool_id: "runtime.execute".into(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: "runtime-inspection".into(),
        },
    );
    store.append_agent_event_v4(&started).await.unwrap();
    if resolved_before_stop {
        let resolved = AgentEventV4::next(
            &started,
            Utc::now(),
            AgentEventKindV4::ToolDispatchResolved {
                call_id: "runtime-inspection".into(),
                resolution: omicsops_protocol::UncertainResolutionV4::SideEffectObserved,
                evidence: "fixture: execution finished and its result was independently verified"
                    .into(),
            },
        );
        store.append_agent_event_v4(&resolved).await.unwrap();
        // Safe historical attention runs can result from a later model failure.
        // Keep resolution before the terminal event: terminal chains are immutable.
        let terminal = AgentEventV4::next(
            &resolved,
            Utc::now(),
            AgentEventKindV4::RunNeedsAttention {
                message: "fixture: model stopped after the verified result".into(),
            },
        );
        store.append_agent_event_v4(&terminal).await.unwrap();
        store
            .save_agent_run_v4(
                item.run_id,
                project.id,
                conversation.id,
                "needs_attention",
                &json!({"run_id":item.run_id,"project_id":project.id,"conversation_id":conversation.id,"status":"needs_attention"}),
            )
            .await
            .unwrap();
    }
    store
        .request_run_stop_v4(&omicsops_dto::StopRunRequestV4 {
            request_id: Uuid::new_v4(),
            project_id: project.id,
            conversation_id: conversation.id,
            run_id: item.run_id,
        })
        .await
        .unwrap();
    store
        .finalize_inactive_run_stop_v4(project.id, conversation.id, item.run_id)
        .await
        .unwrap();
    (store, project, conversation, item, pending)
}

#[tokio::test]
async fn stopped_unresolved_dispatch_waits_for_observed_stop_reconciliation() {
    let (store, project, conversation, item, pending) = stopped_runtime_queue_fixture(false).await;
    let rows = store
        .list_composer_queue(project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Uncertain);
    assert_eq!(
        rows[0].failure_code,
        Some(ComposerQueueFailureCodeV4::LeaseUncertain)
    );
    assert_eq!(rows[1], pending);
    assert!(
        store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.agent_run_v4(item.run_id).await.unwrap().unwrap()["status"],
        "needs_attention"
    );
    assert_eq!(
        store
            .get_run_stop_v4(project.id, conversation.id, item.run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        omicsops_dto::StopRunStatusV4::Observed
    );
    let evidence = store.agent_events_v4(item.run_id).await.unwrap();
    assert!(omicsops_store::has_unresolved_side_effect_dispatch(
        &evidence
    ));
    let reconciled = store
        .reconcile_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(reconciled[0].status, ComposerQueueStatusV4::Failed);
    assert_eq!(reconciled[1], pending);
    assert_eq!(store.agent_events_v4(item.run_id).await.unwrap(), evidence);
}

#[tokio::test]
async fn unresolved_legacy_running_queue_without_stop_reconciles_without_advancing() {
    let (store, project, conversation, item, pending) = stopped_runtime_queue_fixture(false).await;
    sqlx::query("DELETE FROM agent_run_stop_requests_v4 WHERE run_id=?1")
        .bind(item.run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    // Older versions left stopped, unresolved queue rows labelled running.
    sqlx::query(
        "UPDATE composer_queue_v4 SET status='running',failure_code=NULL WHERE request_id=?1",
    )
    .bind(item.request_id.to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let evidence = store.agent_events_v4(item.run_id).await.unwrap();
    let rows = store
        .reconcile_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Uncertain);
    assert_eq!(
        rows[0].failure_code,
        Some(ComposerQueueFailureCodeV4::LeaseUncertain)
    );
    assert_eq!(rows[1], pending);
    assert_eq!(store.agent_events_v4(item.run_id).await.unwrap(), evidence);
    assert!(
        store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .reconcile_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap(),
        rows
    );
}

#[tokio::test]
async fn stopped_unresolved_queue_with_verified_safe_history_can_settle_and_advance() {
    let (store, project, conversation, item, pending) = stopped_runtime_queue_fixture(true).await;
    let evidence = store.agent_events_v4(item.run_id).await.unwrap();
    assert!(!omicsops_store::has_unresolved_side_effect_dispatch(
        &evidence
    ));
    sqlx::query(
        "UPDATE composer_queue_v4 SET status='uncertain',failure_code='lease_uncertain' WHERE request_id=?1",
    )
    .bind(item.request_id.to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let rows = store
        .reconcile_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Failed);
    assert_eq!(
        rows[0].failure_code,
        Some(ComposerQueueFailureCodeV4::RunFailed)
    );
    assert_eq!(
        store.agent_run_v4(item.run_id).await.unwrap().unwrap()["status"],
        "needs_attention"
    );
    assert_eq!(store.agent_events_v4(item.run_id).await.unwrap(), evidence);
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.item.request_id, pending.request_id);
    assert_eq!(lease.item.run_id, pending.run_id);
}

fn ordinary_agent_payload(item: &ComposerQueueItemV4) -> (Value, Vec<AgentEventV4>) {
    use omicsops_protocol::{ExecutionPlanV4, RunSpecV4};
    let now = Utc::now();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: item.message_markdown.clone(),
        steps: vec!["handle the new request".into()],
        completion_criteria: vec!["answer the new request".into()],
        requested_capabilities: Default::default(),
    };
    let approval = RunSpecV4::approval_hash_for(
        item.run_id,
        item.project_id,
        item.conversation_id,
        PROFILE_ID,
        &plan,
        &item.frozen.compute_selection,
    )
    .unwrap();
    let mut spec = RunSpecV4::freeze_ordinary_agent_with_compute(
        item.run_id,
        item.project_id,
        item.conversation_id,
        PROFILE_ID,
        plan,
        item.frozen.compute_selection.clone(),
        &approval,
        now,
    )
    .unwrap();
    spec.model_configuration_hash = Some(item.frozen.model_configuration_hash.clone());
    spec.conversation_preferences = Some(item.frozen.conversation_preferences);
    spec.service_tier = Some(item.frozen.service_tier);
    spec.spec_hash = Some(spec.calculate_spec_hash().unwrap());
    let created = AgentEventV4::first(
        item.run_id,
        item.project_id,
        item.conversation_id,
        now,
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    let frozen = AgentEventV4::next(
        &created,
        now,
        AgentEventKindV4::RunSpecFrozen {
            approval_hash: approval,
            spec_hash: spec.spec_hash.clone().unwrap(),
        },
    );
    let mut value = plan_value(item, &material(""));
    value["status"] = json!("running");
    value["spec"] = serde_json::to_value(spec).unwrap();
    (value, vec![created, frozen])
}

#[tokio::test]
async fn observed_stop_releases_attention_queue_for_a_new_agent_run_without_replay() {
    let (store, project, conversation, old, pending) = stopped_runtime_queue_fixture(false).await;
    let old_run = store.agent_run_v4(old.run_id).await.unwrap();
    let old_events = store.agent_events_v4(old.run_id).await.unwrap();
    let old_stop = store
        .get_run_stop_v4(project.id, conversation.id, old.run_id)
        .await
        .unwrap();
    assert!(
        store
            .can_advance_after_attention_run(project.id, conversation.id, old.run_id)
            .await
            .unwrap()
    );
    assert!(
        !store
            .safe_needs_attention_run(project.id, conversation.id, old.run_id)
            .await
            .unwrap()
    );
    assert!(
        store
            .conversation_agent_state_v4(project.id, conversation.id)
            .await
            .unwrap()
            .composer_blocking_run_ids
            .is_empty()
    );
    assert_eq!(
        store
            .stopped_unresolved_run_ids_for_context(project.id, conversation.id, 8)
            .await
            .unwrap(),
        vec![old.run_id]
    );
    let job_id = Uuid::new_v4();
    let job_json = json!({"job_id":job_id,"context":{"project_id":project.id,"run_id":old.run_id,"backend_id":"ssh-fixture","language":"python","environment":"system"},"call_id":"runtime-inspection","request_sha256":"a".repeat(64),"state":"unknown","remote_root":"/fixture/job","remote_host_key":"fixture","session_id":null,"result_request_id":null,"result_sha256":null}).to_string();
    sqlx::query("INSERT INTO runtime_jobs_v4(job_id,run_id,call_id,value_json) VALUES (?1,?2,'runtime-inspection',?3)")
        .bind(job_id.to_string()).bind(old.run_id.to_string()).bind(&job_json).execute(store.pool()).await.unwrap();
    let rows = store
        .reconcile_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Failed);
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.item.run_id, pending.run_id);
    assert_ne!(lease.item.run_id, old.run_id);
    let (value, events) = ordinary_agent_payload(&lease.item);
    let committed = store
        .commit_composer_queue_dispatch(&lease, &value, &events, Utc::now())
        .await
        .unwrap();
    assert_eq!(committed.status, ComposerQueueStatusV4::Running);
    assert_eq!(store.agent_events_v4(pending.run_id).await.unwrap(), events);
    assert_eq!(store.agent_run_v4(old.run_id).await.unwrap(), old_run);
    assert_eq!(store.agent_events_v4(old.run_id).await.unwrap(), old_events);
    assert_eq!(
        store
            .get_run_stop_v4(project.id, conversation.id, old.run_id)
            .await
            .unwrap(),
        old_stop
    );
    assert!(omicsops_store::has_unresolved_side_effect_dispatch(
        &old_events
    ));
    let retained_job: String =
        sqlx::query_scalar("SELECT value_json FROM runtime_jobs_v4 WHERE job_id=?1")
            .bind(job_id.to_string())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(retained_job, job_json);
}

#[tokio::test]
async fn observed_stop_release_rechecks_authority_at_commit() {
    let (store, project, conversation, old, _) = stopped_runtime_queue_fixture(false).await;
    store
        .reconcile_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap();
    let lease = store
        .claim_next_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("DELETE FROM agent_run_stop_requests_v4 WHERE run_id=?1")
        .bind(old.run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let (value, events) = ordinary_agent_payload(&lease.item);
    assert!(
        store
            .commit_composer_queue_dispatch(&lease, &value, &events, Utc::now())
            .await
            .is_err()
    );
    assert!(
        store
            .agent_run_v4(lease.item.run_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .messages_for_conversation(conversation.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn observed_stop_release_requires_trusted_exact_scope_and_terminal_evidence() {
    for damage in [
        "no_stop",
        "requested",
        "wrong_scope",
        "nil_request",
        "hash",
        "event_scope",
        "json",
        "active_run",
    ] {
        let (store, project, conversation, old, _) = stopped_runtime_queue_fixture(false).await;
        match damage {
            "no_stop" => {
                sqlx::query("DELETE FROM agent_run_stop_requests_v4 WHERE run_id=?1")
                    .bind(old.run_id.to_string())
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
            "requested" => {
                sqlx::query(
                    "UPDATE agent_run_stop_requests_v4 SET status='requested' WHERE run_id=?1",
                )
                .bind(old.run_id.to_string())
                .execute(store.pool())
                .await
                .unwrap();
            }
            "wrong_scope" | "event_scope" => {
                let other = Conversation::new(Uuid::new_v4(), project.id, "other", Utc::now());
                store.save_conversation(&other).await.unwrap();
                let sql = if damage == "wrong_scope" {
                    "UPDATE agent_run_stop_requests_v4 SET conversation_id=?1 WHERE run_id=?2"
                } else {
                    "UPDATE agent_events_v4 SET conversation_id=?1 WHERE run_id=?2 AND sequence=3"
                };
                sqlx::query(sql)
                    .bind(other.id.to_string())
                    .bind(old.run_id.to_string())
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
            "nil_request" => {
                sqlx::query("UPDATE agent_run_stop_requests_v4 SET request_id=?1 WHERE run_id=?2")
                    .bind(Uuid::nil().to_string())
                    .bind(old.run_id.to_string())
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
            "hash" => {
                sqlx::query(
                    "UPDATE agent_events_v4 SET event_hash='wrong' WHERE run_id=?1 AND sequence=3",
                )
                .bind(old.run_id.to_string())
                .execute(store.pool())
                .await
                .unwrap();
            }
            "json" => {
                sqlx::query(
                    "UPDATE agent_events_v4 SET value_json='{}' WHERE run_id=?1 AND sequence=3",
                )
                .bind(old.run_id.to_string())
                .execute(store.pool())
                .await
                .unwrap();
            }
            "active_run" => {
                sqlx::query("UPDATE agent_runs_v4 SET status='running',value_json=json_set(value_json,'$.status','running') WHERE run_id=?1").bind(old.run_id.to_string()).execute(store.pool()).await.unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            !store
                .can_advance_after_attention_run(project.id, conversation.id, old.run_id)
                .await
                .unwrap(),
            "{damage}"
        );
        assert!(
            store
                .stopped_unresolved_run_ids_for_context(project.id, conversation.id, 8)
                .await
                .unwrap()
                .is_empty(),
            "{damage}"
        );
        let snapshot = store
            .conversation_agent_state_v4(project.id, conversation.id)
            .await
            .unwrap();
        assert_eq!(
            snapshot.composer_blocking_run_ids,
            if damage == "active_run" {
                vec![]
            } else {
                vec![old.run_id]
            },
            "{damage}"
        );
        let rows = store
            .reconcile_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap();
        assert_eq!(rows[0].status, ComposerQueueStatusV4::Uncertain, "{damage}");
        assert!(
            store
                .claim_next_composer_queue(project.id, conversation.id, Utc::now())
                .await
                .ok()
                .flatten()
                .is_none(),
            "{damage}"
        );
    }
}

#[tokio::test]
async fn observed_stop_release_reconciles_legacy_uncertain_queue_after_reopen() {
    let (store, project, conversation, old, pending) = stopped_runtime_queue_fixture(false).await;
    let evidence = store.agent_events_v4(old.run_id).await.unwrap();
    let directory = tempdir().unwrap();
    let path = directory.path().join("stopped-queue.sqlite");
    sqlx::query("VACUUM INTO ?1")
        .bind(path.to_string_lossy().as_ref())
        .execute(store.pool())
        .await
        .unwrap();
    store.pool().close().await;
    let reopened = Store::open(&path).await.unwrap();
    let rows = reopened
        .reconcile_composer_queue(project.id, conversation.id, Utc::now())
        .await
        .unwrap();
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Failed);
    assert_eq!(rows[1], pending);
    assert_eq!(
        reopened.agent_events_v4(old.run_id).await.unwrap(),
        evidence
    );
    assert_eq!(
        reopened
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap()
            .unwrap()
            .item
            .run_id,
        pending.run_id
    );
}

#[tokio::test]
async fn observed_stop_release_and_safe_history_allow_post_terminal_browser_cleanup() {
    for safe in [false, true] {
        let (store, project, conversation, old, _) = stopped_runtime_queue_fixture(safe).await;
        let events = store.agent_events_v4(old.run_id).await.unwrap();
        let cleanup = AgentEventV4::next(
            events.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::BrowserTabCleanupRequired {
                sessions: vec![omicsops_protocol::BrowserSessionKindV4::Shared],
                tabs: vec![],
                message: "fixture cleanup".into(),
            },
        );
        store.append_agent_event_v4(&cleanup).await.unwrap();
        if safe {
            sqlx::query("DELETE FROM agent_run_stop_requests_v4 WHERE run_id=?1")
                .bind(old.run_id.to_string())
                .execute(store.pool())
                .await
                .unwrap();
        }
        assert_eq!(
            store
                .safe_needs_attention_run(project.id, conversation.id, old.run_id)
                .await
                .unwrap(),
            safe
        );
        assert!(
            store
                .can_advance_after_attention_run(project.id, conversation.id, old.run_id)
                .await
                .unwrap()
        );
        assert_eq!(
            store
                .reconcile_composer_queue(project.id, conversation.id, Utc::now())
                .await
                .unwrap()[0]
                .status,
            ComposerQueueStatusV4::Failed
        );
    }
}

async fn seed_attention_context_run(
    store: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
    unresolved: bool,
    stopped: bool,
) -> Uuid {
    let run_id = Uuid::new_v4();
    store.save_agent_run_v4(run_id, project_id, conversation_id, "needs_attention", &json!({"run_id":run_id,"project_id":project_id,"conversation_id":conversation_id,"status":"needs_attention"})).await.unwrap();
    let mut event = AgentEventV4::first(
        run_id,
        project_id,
        conversation_id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&event).await.unwrap();
    if unresolved {
        event = AgentEventV4::next(
            &event,
            Utc::now(),
            AgentEventKindV4::ToolDispatchStarted {
                call_id: "unknown".into(),
                tool_id: "runtime.execute".into(),
                effect: ToolEffectV4::Runtime,
                idempotency_key: "unknown".into(),
            },
        );
        store.append_agent_event_v4(&event).await.unwrap();
    }
    event = AgentEventV4::next(
        &event,
        Utc::now(),
        AgentEventKindV4::RunNeedsAttention {
            message: "fixture".into(),
        },
    );
    store.append_agent_event_v4(&event).await.unwrap();
    if stopped {
        store
            .request_run_stop_v4(&omicsops_dto::StopRunRequestV4 {
                request_id: Uuid::new_v4(),
                project_id,
                conversation_id,
                run_id,
            })
            .await
            .unwrap();
    }
    run_id
}

#[tokio::test]
async fn observed_stop_context_is_bounded_and_does_not_hide_older_unresolved_runs() {
    let (store, project, conversation) = fixture().await;
    let old = seed_attention_context_run(&store, project.id, conversation.id, true, true).await;
    for _ in 0..17 {
        seed_attention_context_run(&store, project.id, conversation.id, false, true).await;
    }
    assert_eq!(
        store
            .stopped_unresolved_run_ids_for_context(project.id, conversation.id, 1)
            .await
            .unwrap(),
        vec![old]
    );
    assert!(
        store
            .stopped_unresolved_run_ids_for_context(project.id, conversation.id, 0)
            .await
            .unwrap()
            .is_empty()
    );
    let blocked =
        seed_attention_context_run(&store, project.id, conversation.id, true, false).await;
    for _ in 0..17 {
        seed_attention_context_run(&store, project.id, conversation.id, true, true).await;
    }
    assert_eq!(
        store
            .stopped_unresolved_run_ids_for_context(project.id, conversation.id, u32::MAX)
            .await
            .unwrap()
            .len(),
        16
    );
    assert_eq!(
        store
            .conversation_agent_state_v4(project.id, conversation.id)
            .await
            .unwrap()
            .composer_blocking_run_ids,
        vec![blocked]
    );
    assert!(
        store
            .stopped_unresolved_run_ids_for_context(Uuid::new_v4(), conversation.id, 8)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn claimed_row_reconciles_after_store_restart() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("restart.sqlite");
    let (project, conversation, request_id) = {
        let store = Store::open(&path).await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "restart project",
            r"C:\data\restart-project",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        let conversation = Conversation::new(
            Uuid::new_v4(),
            project.id,
            "restart conversation",
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        store.save_conversation(&conversation).await.unwrap();
        store
            .save_model_profile(&profile("dispatch-test"))
            .await
            .unwrap();
        let queued = enqueue(
            &store,
            project.id,
            conversation.id,
            ComposerQueueModeV4::Agent,
            "restart",
            "",
        )
        .await;
        let _lease = store
            .claim_next_composer_queue(project.id, conversation.id, Utc::now())
            .await
            .unwrap()
            .unwrap();
        (project, conversation, queued.request_id)
    };
    let reopened = Store::open(&path).await.unwrap();
    let future = Utc::now() + Duration::seconds(121);
    let rows = reopened
        .reconcile_composer_queue(project.id, conversation.id, future)
        .await
        .unwrap();
    assert_eq!(rows[0].request_id, request_id);
    assert_eq!(rows[0].status, ComposerQueueStatusV4::Pending);
}

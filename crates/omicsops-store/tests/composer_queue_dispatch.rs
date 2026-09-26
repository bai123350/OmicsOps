use chrono::{Duration, Utc};
use omicsops_core::workspace::{
    Conversation, ModelProfile, ModelProviderKind, Project, ProjectTemplate,
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

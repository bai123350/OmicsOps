use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_protocol::*;
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

async fn fixture(store: &Store) -> (ExecutionContextKeyV4, ToolCallV4) {
    let project = Project::new(
        Uuid::new_v4(),
        "jobs",
        "synthetic",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "jobs", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "test".into(),
        steps: vec!["compute".into()],
        completion_criteria: vec!["result".into()],
        requested_capabilities: Default::default(),
    };
    let spec = RunSpecV4::freeze(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        Uuid::new_v4(),
        plan.clone(),
        &plan.canonical_hash().unwrap(),
        Utc::now(),
    )
    .unwrap();
    store
        .save_agent_run_v4_if_unlocked(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            "running",
            &json!({"spec":spec}),
        )
        .await
        .unwrap();
    let first = AgentEventV4::first(
        spec.run_id,
        spec.project_id,
        spec.conversation_id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    let call = ToolCallV4 {
        call_id: "cell-1".into(),
        tool_id: "runtime.execute".into(),
        arguments: json!({"language":"python", "code":"synthetic marker code"}),
    };
    append(
        store,
        spec.run_id,
        AgentEventKindV4::ToolRequested { call: call.clone() },
    )
    .await;
    append(
        store,
        spec.run_id,
        AgentEventKindV4::ToolDispatchStarted {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: call.call_id.clone(),
        },
    )
    .await;
    (
        ExecutionContextKeyV4 {
            project_id: spec.project_id,
            run_id: spec.run_id,
            backend_id: "local".into(),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        },
        call,
    )
}

async fn append(store: &Store, run_id: Uuid, event: AgentEventKindV4) {
    let previous = store.agent_events_v4(run_id).await.unwrap().pop().unwrap();
    store
        .append_agent_event_v4(&AgentEventV4::next(&previous, Utc::now(), event))
        .await
        .unwrap();
}

fn result(session_id: Uuid, succeeded: bool) -> RuntimeResultV4 {
    RuntimeResultV4 {
        request_id: Uuid::new_v4(),
        session_id,
        process_identity: "synthetic-process".into(),
        stdout: "synthetic private output".into(),
        stderr: String::new(),
        stdout_capture: None,
        stderr_capture: None,
        succeeded,
        artifacts: vec![],
        software_versions: Default::default(),
    }
}

#[tokio::test]
async fn concurrent_reservations_grant_one_launch_and_survive_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("jobs.sqlite");
    let store = Store::open(&path).await.unwrap();
    let (key, call) = fixture(&store).await;
    let hash = call.canonical_hash().unwrap();
    let (a, b) = tokio::join!(
        store.reserve_runtime_job_v4(&key, &call.call_id, &hash),
        store.reserve_runtime_job_v4(&key, &call.call_id, &hash)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.1, b.1);
    assert_eq!(a.0, b.0);
    store.pool().close().await;
    let reopened = Store::open(&path).await.unwrap();
    let (again, acquired) = reopened
        .reserve_runtime_job_v4(&key, &call.call_id, &hash)
        .await
        .unwrap();
    assert!(!acquired);
    assert_eq!(again, a.0);
}

#[tokio::test]
async fn identities_and_recorded_dispatch_are_required() {
    let store = Store::open_in_memory().await.unwrap();
    let (key, call) = fixture(&store).await;
    let hash = call.canonical_hash().unwrap();
    assert!(
        store
            .reserve_runtime_job_v4(&key, "missing", &hash)
            .await
            .is_err()
    );
    assert!(
        store
            .reserve_runtime_job_v4(&key, &call.call_id, &"a".repeat(64))
            .await
            .is_err()
    );
    let wrong = ExecutionContextKeyV4 {
        project_id: Uuid::new_v4(),
        ..key.clone()
    };
    assert!(
        store
            .reserve_runtime_job_v4(&wrong, &call.call_id, &hash)
            .await
            .is_err()
    );
    let wrong = ExecutionContextKeyV4 {
        language: KernelLanguageV4::R,
        ..key.clone()
    };
    assert!(
        store
            .reserve_runtime_job_v4(&wrong, &call.call_id, &hash)
            .await
            .is_err()
    );
    store
        .reserve_runtime_job_v4(&key, &call.call_id, &hash)
        .await
        .unwrap();
    assert!(store.runtime_job_v4(&wrong, &call.call_id).await.is_err());
    assert!(
        store
            .reserve_runtime_job_v4(&key, &call.call_id, &"b".repeat(64))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn completion_is_session_bound_terminal_and_stores_only_digest() {
    for succeeded in [true, false] {
        let store = Store::open_in_memory().await.unwrap();
        let (key, call) = fixture(&store).await;
        let hash = call.canonical_hash().unwrap();
        let (reserved, _) = store
            .reserve_runtime_job_v4(&key, &call.call_id, &hash)
            .await
            .unwrap();
        let session = Uuid::new_v4();
        let running = store
            .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Running, Some(session), None)
            .await
            .unwrap();
        assert!(
            store
                .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Unknown, None, None)
                .await
                .is_err()
        );
        let state = if succeeded {
            RuntimeJobStateV4::Succeeded
        } else {
            RuntimeJobStateV4::Failed
        };
        assert!(
            store
                .advance_runtime_job_v4(
                    &running,
                    state,
                    None,
                    Some(&result(Uuid::new_v4(), succeeded))
                )
                .await
                .is_err()
        );
        let output = result(session, succeeded);
        let complete = store
            .advance_runtime_job_v4(&running, state, None, Some(&output))
            .await
            .unwrap();
        assert_eq!(complete.result_request_id, Some(output.request_id));
        assert_eq!(complete.result_sha256.as_ref().unwrap().len(), 64);
        assert!(
            store
                .advance_runtime_job_v4(&complete, RuntimeJobStateV4::Running, Some(session), None)
                .await
                .is_err()
        );
        assert!(
            !store
                .reserve_runtime_job_v4(&key, &call.call_id, &hash)
                .await
                .unwrap()
                .1
        );
        let json: String = sqlx::query_scalar("SELECT value_json FROM runtime_jobs_v4")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert!(!json.contains("synthetic private output"));
        assert!(!json.contains("synthetic marker code"));
        assert!(!json.contains("synthetic-process"));
    }
}

#[tokio::test]
async fn uncertainty_and_cancel_block_late_completion_and_never_grant_relaunch() {
    for cancelled in [false, true] {
        let store = Store::open_in_memory().await.unwrap();
        let (key, call) = fixture(&store).await;
        let hash = call.canonical_hash().unwrap();
        let (reserved, _) = store
            .reserve_runtime_job_v4(&key, &call.call_id, &hash)
            .await
            .unwrap();
        let session = Uuid::new_v4();
        let running = store
            .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Running, Some(session), None)
            .await
            .unwrap();
        append(
            &store,
            key.run_id,
            if cancelled {
                AgentEventKindV4::RunCancelled
            } else {
                AgentEventKindV4::ToolDispatchUncertain {
                    call_id: call.call_id.clone(),
                    tool_id: call.tool_id.clone(),
                }
            },
        )
        .await;
        let job = store
            .runtime_job_v4(&key, &call.call_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.state, RuntimeJobStateV4::Unknown);
        assert!(
            store
                .advance_runtime_job_v4(
                    &running,
                    RuntimeJobStateV4::Succeeded,
                    None,
                    Some(&result(session, true))
                )
                .await
                .is_err()
        );
        assert!(
            !store
                .reserve_runtime_job_v4(&key, &call.call_id, &hash)
                .await
                .unwrap()
                .1
        );
    }
}

#[tokio::test]
async fn terminal_legacy_dispatch_without_job_cannot_be_relaunched() {
    let store = Store::open_in_memory().await.unwrap();
    let (key, call) = fixture(&store).await;
    append(
        &store,
        key.run_id,
        AgentEventKindV4::ToolFinished {
            outcome: ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: true,
                model_content: "done".into(),
                data: json!({}),
                provenance: vec![],
            },
        },
    )
    .await;
    assert!(
        store
            .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn cancellation_and_job_state_commit_or_roll_back_together() {
    let store = Store::open_in_memory().await.unwrap();
    let (key, call) = fixture(&store).await;
    let (reserved, _) = store
        .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
        .await
        .unwrap();
    let previous = store
        .agent_events_v4(key.run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let cancelled = AgentEventV4::next(&previous, Utc::now(), AgentEventKindV4::RunCancelled);
    sqlx::query("CREATE TRIGGER reject_job_update BEFORE UPDATE ON runtime_jobs_v4 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(store.pool()).await.unwrap();
    assert!(store.append_agent_event_v4(&cancelled).await.is_err());
    assert_eq!(
        store
            .runtime_job_v4(&key, &call.call_id)
            .await
            .unwrap()
            .unwrap(),
        reserved
    );
    assert_eq!(
        store
            .agent_events_v4(key.run_id)
            .await
            .unwrap()
            .last()
            .unwrap(),
        &previous
    );
    sqlx::query("DROP TRIGGER reject_job_update")
        .execute(store.pool())
        .await
        .unwrap();
    store.append_agent_event_v4(&cancelled).await.unwrap();
    assert_eq!(
        store
            .runtime_job_v4(&key, &call.call_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        RuntimeJobStateV4::Unknown
    );
}

#[tokio::test]
async fn result_receipt_survives_reopen_is_verified_and_retires_with_tool_event() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("receipts.sqlite");
    let store = Store::open(&path).await.unwrap();
    let (key, call) = fixture(&store).await;
    let (reserved, _) = store
        .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
        .await
        .unwrap();
    let session = Uuid::new_v4();
    let running = store
        .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Running, Some(session), None)
        .await
        .unwrap();
    let output = result(session, true);
    store
        .advance_runtime_job_v4(&running, RuntimeJobStateV4::Succeeded, None, Some(&output))
        .await
        .unwrap();
    store.pool().close().await;
    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store
            .recover_runtime_result_v4(&key, &call)
            .await
            .unwrap()
            .unwrap()
            .1,
        output
    );
    let mut changed = call.clone();
    changed.arguments["code"] = json!("different");
    assert!(
        store
            .recover_runtime_result_v4(&key, &changed)
            .await
            .is_err()
    );
    let original: String = sqlx::query_scalar("SELECT result_json FROM runtime_job_results_v4")
        .fetch_one(store.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE runtime_job_results_v4 SET result_json='{}'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(store.recover_runtime_result_v4(&key, &call).await.is_err());
    sqlx::query("UPDATE runtime_job_results_v4 SET result_json=?1")
        .bind(original)
        .execute(store.pool())
        .await
        .unwrap();
    let previous = store
        .agent_events_v4(key.run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let event = AgentEventV4::next(
        &previous,
        Utc::now(),
        AgentEventKindV4::ToolFinished {
            outcome: ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: true,
                model_content: "42".into(),
                data: serde_json::to_value(&output).unwrap(),
                provenance: vec![],
            },
        },
    );
    sqlx::query("CREATE TRIGGER reject_receipt_delete BEFORE DELETE ON runtime_job_results_v4 BEGIN SELECT RAISE(ABORT,'synthetic deletion failure'); END").execute(store.pool()).await.unwrap();
    assert!(store.append_agent_event_v4(&event).await.is_err());
    assert!(
        store
            .recover_runtime_result_v4(&key, &call)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store.agent_events_v4(key.run_id).await.unwrap().last(),
        Some(&previous)
    );
    sqlx::query("DROP TRIGGER reject_receipt_delete")
        .execute(store.pool())
        .await
        .unwrap();
    store.append_agent_event_v4(&event).await.unwrap();
    assert!(
        store
            .recover_runtime_result_v4(&key, &call)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn receipt_failure_and_size_limit_never_commit_terminal_metadata() {
    let store = Store::open_in_memory().await.unwrap();
    let (key, call) = fixture(&store).await;
    let (reserved, _) = store
        .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
        .await
        .unwrap();
    let session = Uuid::new_v4();
    let running = store
        .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Running, Some(session), None)
        .await
        .unwrap();
    let mut output = result(session, true);
    output.stdout = "x".repeat(1024 * 1024);
    assert!(
        store
            .advance_runtime_job_v4(&running, RuntimeJobStateV4::Succeeded, None, Some(&output))
            .await
            .is_err()
    );
    output.stdout = "small".into();
    sqlx::query("CREATE TRIGGER reject_receipt BEFORE INSERT ON runtime_job_results_v4 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(store.pool()).await.unwrap();
    assert!(
        store
            .advance_runtime_job_v4(&running, RuntimeJobStateV4::Succeeded, None, Some(&output))
            .await
            .is_err()
    );
    assert_eq!(
        store
            .runtime_job_v4(&key, &call.call_id)
            .await
            .unwrap()
            .unwrap(),
        running
    );
    assert!(
        store
            .recover_runtime_result_v4(&key, &call)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn recovery_offer_is_atomic_and_idempotent_and_requires_all_receipts() {
    let store = Store::open_in_memory().await.unwrap();
    let (key, call) = fixture(&store).await;
    assert!(
        store
            .prepare_runtime_recovery_v4(key.run_id)
            .await
            .unwrap()
            .is_none()
    );
    let (reserved, _) = store
        .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
        .await
        .unwrap();
    let session = Uuid::new_v4();
    let running = store
        .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Running, Some(session), None)
        .await
        .unwrap();
    store
        .advance_runtime_job_v4(
            &running,
            RuntimeJobStateV4::Succeeded,
            None,
            Some(&result(session, true)),
        )
        .await
        .unwrap();
    let previous = store
        .agent_events_v4(key.run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_recovery_offer BEFORE UPDATE ON agent_runs_v4 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(store.pool()).await.unwrap();
    assert!(store.prepare_runtime_recovery_v4(key.run_id).await.is_err());
    assert_eq!(
        store.agent_events_v4(key.run_id).await.unwrap().last(),
        Some(&previous)
    );
    sqlx::query("DROP TRIGGER reject_recovery_offer")
        .execute(store.pool())
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        store.prepare_runtime_recovery_v4(key.run_id),
        store.prepare_runtime_recovery_v4(key.run_id)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.is_some(), b.is_some());
    let event = a.or(b).unwrap();
    assert!(
        matches!(event.event, AgentEventKindV4::RuntimeRecoveryAvailable { call_ids } if call_ids == vec![call.call_id])
    );
    let status: String = sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
        .bind(key.run_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(status, "waiting_for_input");
}

#[tokio::test]
async fn incomplete_or_uncertain_dispatches_do_not_receive_recovery_offer() {
    for uncertain in [false, true] {
        let store = Store::open_in_memory().await.unwrap();
        let (key, call) = fixture(&store).await;
        let (reserved, _) = store
            .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
            .await
            .unwrap();
        let session = Uuid::new_v4();
        let running = store
            .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Running, Some(session), None)
            .await
            .unwrap();
        store
            .advance_runtime_job_v4(
                &running,
                RuntimeJobStateV4::Succeeded,
                None,
                Some(&result(session, true)),
            )
            .await
            .unwrap();
        append(
            &store,
            key.run_id,
            if uncertain {
                AgentEventKindV4::ToolDispatchUncertain {
                    call_id: call.call_id,
                    tool_id: call.tool_id,
                }
            } else {
                AgentEventKindV4::ToolDispatchStarted {
                    call_id: "other".into(),
                    tool_id: "mutate".into(),
                    effect: ToolEffectV4::Mutating,
                    idempotency_key: "other".into(),
                }
            },
        )
        .await;
        assert!(
            store
                .prepare_runtime_recovery_v4(key.run_id)
                .await
                .unwrap()
                .is_none()
        );
    }
}

async fn ready_recovery(store: &Store) -> (ExecutionContextKeyV4, ToolCallV4) {
    let (key, call) = fixture(store).await;
    let (reserved, _) = store
        .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
        .await
        .unwrap();
    let session = Uuid::new_v4();
    let running = store
        .advance_runtime_job_v4(&reserved, RuntimeJobStateV4::Running, Some(session), None)
        .await
        .unwrap();
    store
        .advance_runtime_job_v4(
            &running,
            RuntimeJobStateV4::Succeeded,
            None,
            Some(&result(session, true)),
        )
        .await
        .unwrap();
    store
        .prepare_runtime_recovery_v4(key.run_id)
        .await
        .unwrap()
        .unwrap();
    (key, call)
}

#[tokio::test]
async fn recovery_cancellation_is_atomic_idempotent_and_preserves_receipts() {
    let store = Store::open_in_memory().await.unwrap();
    let (key, call) = ready_recovery(&store).await;
    let before = store.agent_events_v4(key.run_id).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_cancel BEFORE UPDATE ON agent_runs_v4 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(store.pool()).await.unwrap();
    assert!(store.cancel_runtime_recovery_v4(key.run_id).await.is_err());
    assert_eq!(store.agent_events_v4(key.run_id).await.unwrap(), before);
    assert_eq!(
        store.agent_run_v4(key.run_id).await.unwrap().unwrap()["status"],
        "waiting_for_input"
    );
    sqlx::query("DROP TRIGGER reject_cancel")
        .execute(store.pool())
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        store.cancel_runtime_recovery_v4(key.run_id),
        store.cancel_runtime_recovery_v4(key.run_id)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    let events = store.agent_events_v4(key.run_id).await.unwrap();
    assert_eq!(events.len(), before.len() + 1);
    assert!(matches!(
        events.last().unwrap().event,
        AgentEventKindV4::RunCancelled
    ));
    let row: (String, String) =
        sqlx::query_as("SELECT status,value_json FROM agent_runs_v4 WHERE run_id=?1")
            .bind(key.run_id.to_string())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(row.0, "cancelled");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&row.1).unwrap()["status"],
        "cancelled"
    );
    assert!(
        store
            .recover_runtime_result_v4(&key, &call)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        !store
            .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
            .await
            .unwrap()
            .1
    );
    assert!(
        store
            .prepare_runtime_recovery_v4(key.run_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn recovery_cancellation_rejects_other_pause_and_terminal_states() {
    let store = Store::open_in_memory().await.unwrap();
    assert!(
        store
            .cancel_runtime_recovery_v4(Uuid::new_v4())
            .await
            .is_err()
    );
    let (key, _) = fixture(&store).await;
    assert!(store.cancel_runtime_recovery_v4(key.run_id).await.is_err());
    sqlx::query("UPDATE agent_runs_v4 SET status='waiting_for_input' WHERE run_id=?1")
        .bind(key.run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert!(store.cancel_runtime_recovery_v4(key.run_id).await.is_err());
    for resumed in [false, true] {
        let store = Store::open_in_memory().await.unwrap();
        let (key, _) = ready_recovery(&store).await;
        if resumed {
            sqlx::query("UPDATE agent_runs_v4 SET status='running' WHERE run_id=?1")
                .bind(key.run_id.to_string())
                .execute(store.pool())
                .await
                .unwrap();
        } else {
            append(
                &store,
                key.run_id,
                AgentEventKindV4::RunFailed {
                    message: "synthetic terminal".into(),
                },
            )
            .await;
        }
        let before = store.agent_events_v4(key.run_id).await.unwrap();
        assert!(store.cancel_runtime_recovery_v4(key.run_id).await.is_err());
        assert_eq!(store.agent_events_v4(key.run_id).await.unwrap(), before);
    }
}

use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::{StopRunRequestV4, StopRunStatusV4, SubmitGuidanceV4Request};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ExecutionPlanV4, RunModeV4, RunSpecV4, ToolEffectV4,
    UncertainResolutionV4,
};
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

async fn running_run() -> (Store, Project, Conversation, Uuid) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "stop fixture",
        "C:\\data\\stop-fixture",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "stop", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "running",
            &json!({"status":"running"}),
        )
        .await
        .unwrap();
    let first = AgentEventV4::first(
        run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    (store, project, conversation, run_id)
}

async fn running_run_with_spec() -> (Store, Project, Conversation, RunSpecV4) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "guidance stop fixture",
        "C:\\data\\guidance-stop-fixture",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "stop", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "stop guidance fixture".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: Default::default(),
    };
    let mut spec = RunSpecV4::freeze(
        run_id,
        project.id,
        conversation.id,
        Uuid::new_v4(),
        plan.clone(),
        &plan.canonical_hash().unwrap(),
        Utc::now(),
    )
    .unwrap();
    spec.execution_kind = omicsops_protocol::RunExecutionKindV4::OrdinaryAgent;
    store
        .save_agent_run_v4_if_unlocked(
            run_id,
            project.id,
            conversation.id,
            "running",
            &json!({"spec":spec,"status":"running"}),
        )
        .await
        .unwrap();
    let first = AgentEventV4::first(
        run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    (store, project, conversation, spec)
}

fn stop_request(project: &Project, conversation: &Conversation, run_id: Uuid) -> StopRunRequestV4 {
    StopRunRequestV4 {
        request_id: Uuid::new_v4(),
        project_id: project.id,
        conversation_id: conversation.id,
        run_id,
    }
}

async fn append(store: &Store, run_id: Uuid, event: AgentEventKindV4) -> AgentEventV4 {
    let previous = store.agent_events_v4(run_id).await.unwrap().pop().unwrap();
    let next = AgentEventV4::next(&previous, Utc::now(), event);
    store.append_agent_event_v4(&next).await.unwrap();
    next
}

#[tokio::test]
async fn stop_requests_have_a_scoped_idempotent_receipt() {
    let (store, project, conversation, run_id) = running_run().await;
    let request = StopRunRequestV4 {
        request_id: Uuid::new_v4(),
        project_id: project.id,
        conversation_id: conversation.id,
        run_id,
    };
    let first = store.request_run_stop_v4(&request).await.unwrap();
    assert_eq!(first.request_id, request.request_id);
    assert_eq!(store.request_run_stop_v4(&request).await.unwrap(), first);
    assert!(store.has_run_stop_request_v4(run_id).await.unwrap());
}

#[tokio::test]
async fn same_run_uses_one_canonical_intent_even_for_a_new_request_id() {
    let (store, project, conversation, run_id) = running_run().await;
    let first = store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    let second = store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    assert_eq!(second, first);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_run_stop_requests_v4")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn request_and_get_reject_cross_scope_and_terminal_state_is_observed() {
    let (store, project, conversation, run_id) = running_run().await;
    let request = stop_request(&project, &conversation, run_id);
    let first = store.request_run_stop_v4(&request).await.unwrap();

    let other_project = Project::new(
        Uuid::new_v4(),
        "other stop fixture",
        "C:\\data\\other-stop-fixture",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&other_project).await.unwrap();
    let other_conversation =
        Conversation::new(Uuid::new_v4(), other_project.id, "other", Utc::now());
    store.save_conversation(&other_conversation).await.unwrap();
    let mut cross_scope = request.clone();
    cross_scope.project_id = other_project.id;
    cross_scope.conversation_id = other_conversation.id;
    assert!(store.request_run_stop_v4(&cross_scope).await.is_err());
    assert!(
        store
            .get_run_stop_v4(other_project.id, other_conversation.id, run_id)
            .await
            .is_err()
    );

    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "completed",
            &json!({"status":"completed"}),
        )
        .await
        .unwrap();
    let observed = store
        .get_run_stop_v4(project.id, conversation.id, run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(observed.request_id, first.request_id);
    assert_eq!(observed.status, StopRunStatusV4::Observed);
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT status FROM agent_run_stop_requests_v4 WHERE run_id=?1",
        )
        .bind(run_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap(),
        "observed"
    );
}

#[tokio::test]
async fn ordinary_sends_and_guidance_are_blocked_after_stop_intent() {
    let (store, project, conversation, spec) = running_run_with_spec().await;
    let request = stop_request(&project, &conversation, spec.run_id);
    store.request_run_stop_v4(&request).await.unwrap();

    let error = store
        .ensure_conversation_unlocked_v4(project.id, conversation.id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("stop"));
    let new_run = Uuid::new_v4();
    let error = store
        .save_agent_run_v4_if_unlocked(
            new_run,
            project.id,
            conversation.id,
            "running",
            &json!({"status":"running"}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("stop"));

    let guidance = SubmitGuidanceV4Request {
        message_id: Uuid::new_v4(),
        run_id: spec.run_id,
        project_id: project.id,
        conversation_id: conversation.id,
        markdown: "continue with the bounded check".into(),
    };
    let error = store.accept_guidance_v4(&guidance).await.unwrap_err();
    assert!(error.to_string().contains("stop"));
}

#[tokio::test]
async fn finalize_appends_one_hash_chained_cancel_and_is_idempotent() {
    let (store, project, conversation, run_id) = running_run().await;
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    let cancelled = store
        .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(cancelled.event, AgentEventKindV4::RunCancelled));
    assert_eq!(cancelled.sequence, 2);
    assert!(cancelled.verify().is_ok());
    let events = store.agent_events_v4(run_id).await.unwrap();
    assert_eq!(events.last(), Some(&cancelled));
    assert_eq!(cancelled.previous_hash, events[0].event_hash);
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    assert_eq!(
        store
            .get_run_stop_v4(project.id, conversation.id, run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        StopRunStatusV4::Observed
    );
    assert!(
        store
            .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.agent_events_v4(run_id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn terminal_event_is_preserved_and_stop_without_intent_is_a_noop() {
    let (store, project, conversation, run_id) = running_run().await;
    assert!(
        store
            .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
            .await
            .unwrap()
            .is_none()
    );
    append(&store, run_id, AgentEventKindV4::RunCancelled).await;
    let request = stop_request(&project, &conversation, run_id);
    let receipt = store.request_run_stop_v4(&request).await.unwrap();
    assert_eq!(receipt.status, StopRunStatusV4::Observed);
    assert!(
        store
            .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.agent_events_v4(run_id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn unresolved_side_effect_dispatch_becomes_needs_attention() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::ToolDispatchStarted {
            call_id: "runtime-1".into(),
            tool_id: "runtime.execute".into(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: "runtime-1".into(),
        },
    )
    .await;
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    let event = store
        .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event.event,
        AgentEventKindV4::RunNeedsAttention { .. }
    ));
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "needs_attention"
    );
}

#[tokio::test]
async fn standalone_uncertain_dispatch_marker_also_preserves_attention() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::ToolDispatchUncertain {
            call_id: "unknown-1".into(),
            tool_id: "remote.execute".into(),
        },
    )
    .await;
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    let event = store
        .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event.event,
        AgentEventKindV4::RunNeedsAttention { .. }
    ));
}

#[tokio::test]
async fn read_only_uncertain_dispatch_does_not_claim_a_side_effect() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::ToolDispatchStarted {
            call_id: "read-1".into(),
            tool_id: "search.read".into(),
            effect: ToolEffectV4::ReadOnly,
            idempotency_key: "read-1".into(),
        },
    )
    .await;
    append(
        &store,
        run_id,
        AgentEventKindV4::ToolDispatchUncertain {
            call_id: "read-1".into(),
            tool_id: "search.read".into(),
        },
    )
    .await;
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    let event = store
        .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event.event, AgentEventKindV4::RunCancelled));
}

#[tokio::test]
async fn resolved_side_effect_dispatch_can_finish_as_local_cancel() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::ToolDispatchStarted {
            call_id: "runtime-1".into(),
            tool_id: "runtime.execute".into(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: "runtime-1".into(),
        },
    )
    .await;
    append(
        &store,
        run_id,
        AgentEventKindV4::ToolDispatchResolved {
            call_id: "runtime-1".into(),
            resolution: UncertainResolutionV4::SideEffectNotObserved,
            evidence: "synthetic resolution".into(),
        },
    )
    .await;
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    let event = store
        .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event.event, AgentEventKindV4::RunCancelled));
}

#[tokio::test]
async fn stop_request_survives_reopen_and_deletes_with_conversation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("stop.sqlite");
    let store = Store::open(&path).await.unwrap();
    let (project, conversation, run_id) = {
        let project = Project::new(
            Uuid::new_v4(),
            "durable stop",
            "C:\\data\\durable-stop",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "stop", Utc::now());
        store.save_conversation(&conversation).await.unwrap();
        let run_id = Uuid::new_v4();
        store
            .save_agent_run_v4(
                run_id,
                project.id,
                conversation.id,
                "running",
                &json!({"status":"running"}),
            )
            .await
            .unwrap();
        let first = AgentEventV4::first(
            run_id,
            project.id,
            conversation.id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        store.append_agent_event_v4(&first).await.unwrap();
        (project, conversation, run_id)
    };
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    store.pool().close().await;
    let reopened = Store::open(&path).await.unwrap();
    assert!(
        reopened
            .has_table("agent_run_stop_requests_v4")
            .await
            .unwrap()
    );
    assert_eq!(
        reopened
            .get_run_stop_v4(project.id, conversation.id, run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        StopRunStatusV4::Requested
    );
    assert!(
        reopened
            .delete_conversation(project.id, conversation.id)
            .await
            .unwrap()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_run_stop_requests_v4")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn stop_request_deletes_with_its_project() {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "project cascade",
        "C:\\data\\project-cascade",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "stop", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let run_id = Uuid::new_v4();
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "running",
            &json!({"status":"running"}),
        )
        .await
        .unwrap();
    let first = AgentEventV4::first(
        run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    assert!(store.delete_project(project.id).await.unwrap());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_run_stop_requests_v4")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn concurrent_stop_requests_have_one_row_and_terminal_saves_are_allowed() {
    let (store, project, conversation, run_id) = running_run().await;
    let first = stop_request(&project, &conversation, run_id);
    let second = stop_request(&project, &conversation, run_id);
    let (left, right) = tokio::join!(
        store.request_run_stop_v4(&first),
        store.request_run_stop_v4(&second)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_eq!(left, right);
    assert!(left.request_id == first.request_id || left.request_id == second.request_id);
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "cancelled",
            &json!({"status":"cancelled","final":"persisted"}),
        )
        .await
        .unwrap();
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["final"],
        "persisted"
    );
}

#[tokio::test]
async fn stop_after_terminal_event_rejects_delayed_nonterminal_snapshot() {
    let (store, project, conversation, run_id) = running_run().await;
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    append(&store, run_id, AgentEventKindV4::RunCancelled).await;
    let error = store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            "running",
            &json!({"status":"running","stale":true}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("terminal"));
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "running"
    );
    assert!(
        store
            .finalize_inactive_run_stop_v4(project.id, conversation.id, run_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    assert_eq!(store.agent_events_v4(run_id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn terminal_status_repair_updates_only_the_run_status_fields() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::RunFailed {
            message: "simulated terminal failure".into(),
        },
    )
    .await;
    sqlx::query("UPDATE agent_runs_v4 SET status='running',value_json=?1 WHERE run_id=?2")
        .bind(
            json!({
                "status": "running",
                "objective": "keep this",
                "nested": {"preserve": true}
            })
            .to_string(),
        )
        .bind(run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let repaired = store
        .repair_agent_run_terminal_status_v4(project.id, conversation.id, run_id)
        .await
        .unwrap();
    assert_eq!(repaired.as_deref(), Some("failed"));
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap(),
        json!({
            "status": "failed",
            "objective": "keep this",
            "nested": {"preserve": true}
        })
    );
    assert_eq!(store.agent_events_v4(run_id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn terminal_status_repair_resolves_failed_row_against_attention_event() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::RunNeedsAttention {
            message: "verify the dispatch".into(),
        },
    )
    .await;
    sqlx::query("UPDATE agent_runs_v4 SET status='failed',value_json=?1 WHERE run_id=?2")
        .bind(json!({"status":"failed","retained":42}).to_string())
        .bind(run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let repaired = store
        .repair_agent_run_terminal_status_v4(project.id, conversation.id, run_id)
        .await
        .unwrap();
    assert_eq!(repaired.as_deref(), Some("needs_attention"));
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap(),
        json!({"status":"needs_attention","retained":42})
    );
}

#[tokio::test]
async fn terminal_status_repair_works_behind_a_stop_intent_and_is_idempotent() {
    let (store, project, conversation, run_id) = running_run().await;
    store
        .request_run_stop_v4(&stop_request(&project, &conversation, run_id))
        .await
        .unwrap();
    append(
        &store,
        run_id,
        AgentEventKindV4::RunNeedsAttention {
            message: "stop left an unresolved dispatch".into(),
        },
    )
    .await;
    sqlx::query("UPDATE agent_runs_v4 SET status='failed',value_json=?1 WHERE run_id=?2")
        .bind(json!({"status":"failed","kept":true}).to_string())
        .bind(run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let first = store
        .repair_agent_run_terminal_status_v4(project.id, conversation.id, run_id)
        .await
        .unwrap();
    let after_first = store.agent_run_v4(run_id).await.unwrap().unwrap();
    let second = store
        .repair_agent_run_terminal_status_v4(project.id, conversation.id, run_id)
        .await
        .unwrap();
    assert_eq!(first, Some("needs_attention".into()));
    assert_eq!(second, first);
    assert_eq!(
        store.agent_run_v4(run_id).await.unwrap().unwrap(),
        after_first
    );
    assert_eq!(store.agent_events_v4(run_id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn terminal_status_repair_returns_none_without_terminal_and_preserves_row() {
    let (store, project, conversation, run_id) = running_run().await;
    let before = store.agent_run_v4(run_id).await.unwrap().unwrap();
    let repaired = store
        .repair_agent_run_terminal_status_v4(project.id, conversation.id, run_id)
        .await
        .unwrap();
    assert_eq!(repaired, None);
    assert_eq!(store.agent_run_v4(run_id).await.unwrap().unwrap(), before);
}

#[tokio::test]
async fn terminal_status_repair_rejects_cross_scope_without_mutation() {
    let (store, _project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::RunFailed {
            message: "simulated terminal failure".into(),
        },
    )
    .await;
    let before = store.agent_run_v4(run_id).await.unwrap().unwrap();
    let error = store
        .repair_agent_run_terminal_status_v4(Uuid::new_v4(), conversation.id, run_id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not belong"));
    assert_eq!(store.agent_run_v4(run_id).await.unwrap().unwrap(), before);
}

#[tokio::test]
async fn terminal_status_repair_rejects_a_valid_but_wrong_event_scope() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::RunFailed {
            message: "placeholder terminal".into(),
        },
    )
    .await;
    let wrong_project = Uuid::new_v4();
    let first = AgentEventV4::first(
        run_id,
        wrong_project,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    let terminal = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::RunFailed {
            message: "wrong event scope".into(),
        },
    );
    for event in [first, terminal] {
        let changed = sqlx::query(
            "UPDATE agent_events_v4 SET value_json=?1
             WHERE run_id=?2 AND sequence=?3",
        )
        .bind(serde_json::to_string(&event).unwrap())
        .bind(run_id.to_string())
        .bind(i64::try_from(event.sequence).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
        assert_eq!(changed.rows_affected(), 1);
    }
    assert!(
        store
            .agent_events_v4(run_id)
            .await
            .unwrap()
            .iter()
            .any(|event| matches!(event.event, AgentEventKindV4::RunFailed { .. }))
    );
    let before = store.agent_run_v4(run_id).await.unwrap().unwrap();
    let error = store
        .repair_agent_run_terminal_status_v4(project.id, conversation.id, run_id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("scope"));
    assert_eq!(store.agent_run_v4(run_id).await.unwrap().unwrap(), before);
}

#[tokio::test]
async fn terminal_status_repair_rejects_a_broken_event_hash_without_mutation() {
    let (store, project, conversation, run_id) = running_run().await;
    append(
        &store,
        run_id,
        AgentEventKindV4::RunFailed {
            message: "simulated terminal failure".into(),
        },
    )
    .await;
    sqlx::query("UPDATE agent_events_v4 SET value_json=?1 WHERE run_id=?2 AND sequence=2")
        .bind(json!({"tampered":true}).to_string())
        .bind(run_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let before = store.agent_run_v4(run_id).await.unwrap().unwrap();
    let error = store
        .repair_agent_run_terminal_status_v4(project.id, conversation.id, run_id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("event") || error.to_string().contains("hash"));
    assert_eq!(store.agent_run_v4(run_id).await.unwrap().unwrap(), before);
}

use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::SubmitGuidanceV4Request;
use omicsops_protocol::*;
use omicsops_store::{Store, StoreError};
use serde_json::json;
use uuid::Uuid;

async fn fixture(store: &Store, ordinary: bool) -> RunSpecV4 {
    let project = Project::new(Uuid::new_v4(), "guidance", "synthetic-guidance", ProjectTemplate::Blank, Utc::now());
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "guidance", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let plan = ExecutionPlanV4 { schema_version: 4, objective: "synthetic work".into(), steps: vec!["inspect".into()], completion_criteria: vec!["evidence".into()], requested_capabilities: Default::default() };
    let mut spec = RunSpecV4::freeze(Uuid::new_v4(), project.id, conversation.id, Uuid::new_v4(), plan.clone(), &plan.canonical_hash().unwrap(), Utc::now()).unwrap();
    if ordinary { spec.execution_kind = RunExecutionKindV4::OrdinaryAgent; }
    store.save_agent_run_v4_if_unlocked(spec.run_id, spec.project_id, spec.conversation_id, "running", &json!({"spec":spec,"status":"running"})).await.unwrap();
    let first = AgentEventV4::first(spec.run_id, spec.project_id, spec.conversation_id, Utc::now(), AgentEventKindV4::RunCreated { mode: RunModeV4::Execute });
    store.append_agent_event_v4(&first).await.unwrap();
    spec
}

fn request(spec: &RunSpecV4, text: &str) -> SubmitGuidanceV4Request {
    SubmitGuidanceV4Request { message_id: Uuid::new_v4(), run_id: spec.run_id, project_id: spec.project_id, conversation_id: spec.conversation_id, markdown: text.into() }
}

#[tokio::test]
async fn guidance_acceptance_is_idempotent_ordered_and_scope_checked() {
    let store = Store::open_in_memory().await.unwrap();
    let spec = fixture(&store, true).await;
    let first = request(&spec, "  preserve controls  ");
    let (a,b) = tokio::join!(store.accept_guidance_v4(&first), store.accept_guidance_v4(&first));
    let a = a.unwrap(); assert_eq!(a, b.unwrap()); assert_eq!(a.ordinal, 1);
    assert_eq!(a.markdown, "preserve controls");
    assert_eq!(store.agent_events_v4(spec.run_id).await.unwrap().len(), 1);
    let mut conflicting = first.clone(); conflicting.markdown = "different".into();
    assert!(store.accept_guidance_v4(&conflicting).await.is_err());
    conflicting = first.clone(); conflicting.conversation_id = Uuid::new_v4();
    assert!(store.accept_guidance_v4(&conflicting).await.is_err());
    assert!(store.list_guidance_v4(Uuid::new_v4(), spec.conversation_id, spec.run_id).await.is_err());
    let second = store.accept_guidance_v4(&request(&spec, "state uncertainty")).await.unwrap();
    assert_eq!(second.ordinal, 2);
    let consumed = store.consume_guidance_v4(&spec).await.unwrap();
    assert_eq!(consumed.len(), 2);
    assert!(matches!(&consumed[0].event, AgentEventKindV4::GuidanceConsumed {message_id, ..} if *message_id == first.message_id));
    assert!(store.consume_guidance_v4(&spec).await.unwrap().is_empty());
    assert!(!store.has_pending_guidance_v4(spec.run_id).await.unwrap());
    assert!(store.list_guidance_v4(spec.project_id, spec.conversation_id, spec.run_id).await.unwrap().iter().all(|row| row.consumed_at.is_some()));
}

#[tokio::test]
async fn guidance_survives_reopen_and_consumption_rolls_back_with_event_failure() {
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("guidance.sqlite");
    let store = Store::open(&path).await.unwrap();
    let spec = fixture(&store, true).await;
    store.accept_guidance_v4(&request(&spec,"durable guidance")).await.unwrap();
    store.pool().close().await;
    let store = Store::open(&path).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_guidance BEFORE INSERT ON agent_events_v4 WHEN json_extract(NEW.value_json,'$.event.kind')='guidance_consumed' BEGIN SELECT RAISE(ABORT,'injected consumption failure'); END").execute(store.pool()).await.unwrap();
    assert!(store.consume_guidance_v4(&spec).await.is_err());
    assert!(store.has_pending_guidance_v4(spec.run_id).await.unwrap());
    assert_eq!(store.agent_events_v4(spec.run_id).await.unwrap().len(), 1);
    sqlx::query("DROP TRIGGER reject_guidance").execute(store.pool()).await.unwrap();
    let (a,b) = tokio::join!(store.consume_guidance_v4(&spec), store.consume_guidance_v4(&spec));
    assert_eq!(a.unwrap().len()+b.unwrap().len(), 1);
    store.pool().close().await;
    let reopened = Store::open(&path).await.unwrap();
    assert!(reopened.consume_guidance_v4(&spec).await.unwrap().is_empty());
    assert_eq!(reopened.agent_events_v4(spec.run_id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn pending_guidance_prevents_completion_and_terminal_runs_reject_new_input() {
    let store = Store::open_in_memory().await.unwrap(); let spec = fixture(&store, true).await;
    let first = store.agent_events_v4(spec.run_id).await.unwrap().pop().unwrap();
    let proposal = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::CompletionProposalSubmitted { proposal: serde_json::from_value(json!({"schema_version":4,"summary":"done","answer_markdown":"answer","criteria":[]})).unwrap() });
    store.append_agent_event_v4(&proposal).await.unwrap();
    let req = request(&spec,"check uncertainty");
    store.accept_guidance_v4(&req).await.unwrap();
    let done = AgentEventV4::next(&proposal, Utc::now(), AgentEventKindV4::RunCompleted);
    assert!(matches!(store.append_agent_event_v4(&done).await, Err(StoreError::GuidancePending)));
    assert!(store.messages_for_conversation(spec.conversation_id).await.unwrap().is_empty());
    let consumed = store.consume_guidance_v4(&spec).await.unwrap();
    let done = AgentEventV4::next(consumed.last().unwrap(), Utc::now(), AgentEventKindV4::RunCompleted);
    store.append_agent_event_v4(&done).await.unwrap();
    assert!(store.accept_guidance_v4(&request(&spec,"too late")).await.is_err());
    assert!(store.accept_guidance_v4(&req).await.unwrap().consumed_at.is_some());
}

#[tokio::test]
async fn guidance_and_completion_race_has_only_one_winner() {
    for _ in 0..4 {
        let store = Store::open_in_memory().await.unwrap(); let spec = fixture(&store, true).await;
        let first = store.agent_events_v4(spec.run_id).await.unwrap().pop().unwrap();
        let proposal = AgentEventV4::next(&first,Utc::now(),AgentEventKindV4::CompletionProposalSubmitted {proposal:serde_json::from_value(json!({"schema_version":4,"summary":"done","answer_markdown":"answer","criteria":[]})).unwrap()});
        store.append_agent_event_v4(&proposal).await.unwrap();
        let done = AgentEventV4::next(&proposal,Utc::now(),AgentEventKindV4::RunCompleted);
        let req = request(&spec,"last instruction");
        let (accepted,completed) = tokio::join!(store.accept_guidance_v4(&req),store.append_agent_event_v4(&done));
        assert_ne!(accepted.is_ok(),completed.is_ok());
        if accepted.is_ok() { assert!(matches!(completed,Err(StoreError::GuidancePending))); }
    }
}

#[tokio::test]
async fn guidance_rejects_plans_and_enforces_byte_and_lifetime_limits() {
    let store = Store::open_in_memory().await.unwrap(); let plan = fixture(&store, false).await;
    assert!(store.accept_guidance_v4(&request(&plan,"cannot change a plan")).await.is_err());
    let spec = fixture(&store, true).await;
    assert!(store.accept_guidance_v4(&request(&spec,&"测".repeat(683))).await.is_err());
    assert!(store.accept_guidance_v4(&request(&spec,"  ")).await.is_err());
    for index in 0..16 { store.accept_guidance_v4(&request(&spec,&format!("guidance {index}"))).await.unwrap(); }
    store.consume_guidance_v4(&spec).await.unwrap();
    assert!(store.accept_guidance_v4(&request(&spec,"seventeenth")).await.is_err());
}

#[tokio::test]
async fn simultaneous_new_runs_cannot_acquire_the_same_conversation() {
    let store = Store::open_in_memory().await.unwrap(); let spec = fixture(&store, true).await;
    let result = store.save_agent_run_v4_if_unlocked(Uuid::new_v4(),spec.project_id,spec.conversation_id,"running",&json!({})).await;
    assert!(result.is_err());
    sqlx::query("UPDATE agent_runs_v4 SET status='completed' WHERE run_id=?1").bind(spec.run_id.to_string()).execute(store.pool()).await.unwrap();
    let value = json!({});
    let (a,b) = tokio::join!(store.save_agent_run_v4_if_unlocked(Uuid::new_v4(),spec.project_id,spec.conversation_id,"running",&value),store.save_agent_run_v4_if_unlocked(Uuid::new_v4(),spec.project_id,spec.conversation_id,"running",&value));
    assert_ne!(a.is_ok(),b.is_ok());
}

#[tokio::test]
async fn planning_and_ordinary_starts_share_the_conversation_lock() {
    for _ in 0..4 {
        let store = Store::open_in_memory().await.unwrap();
        let spec = fixture(&store, true).await;
        sqlx::query("UPDATE agent_runs_v4 SET status='completed' WHERE run_id=?1")
            .bind(spec.run_id.to_string()).execute(store.pool()).await.unwrap();
        let value = json!({});
        let (ordinary, plan) = tokio::join!(
            store.save_agent_run_v4_if_unlocked(Uuid::new_v4(), spec.project_id, spec.conversation_id, "running", &value),
            store.start_plan_run_v4(Uuid::new_v4(), spec.project_id, spec.conversation_id, "running", &value, "research objective", Utc::now())
        );
        assert_ne!(ordinary.is_ok(), plan.is_ok());
    }
}

use chrono::Utc;
use omicsops_core::workspace::{Conversation, Message, MessageRole, Project, ProjectTemplate};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, CompletionProposalV4, ContextCheckpointV4, RunModeV4,
    ToolOutcomeV4,
};
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

async fn fixture() -> (Store, Project, Conversation) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "fixture",
        "C:\\data\\fixture",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "ordered transcript", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    (store, project, conversation)
}

async fn save_run(store: &Store, run_id: Uuid, project_id: Uuid, conversation_id: Uuid) {
    store
        .save_agent_run_v4(
            run_id,
            project_id,
            conversation_id,
            "planning",
            &json!({"objective":"test"}),
        )
        .await
        .unwrap();
}

fn completion_proposal(summary: &str, answer_markdown: &str) -> CompletionProposalV4 {
    serde_json::from_value(json!({
        "schema_version": 4,
        "summary": summary,
        "criteria": [],
        "answer_markdown": answer_markdown
    }))
    .unwrap()
}

#[tokio::test]
async fn v4_run_and_hash_chained_events_round_trip_independently() {
    let (store, project, conversation) = fixture().await;
    let run = Uuid::new_v4();
    save_run(&store, run, project.id, conversation.id).await;
    let first = AgentEventV4::first(
        run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    let second = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::CompletionProposed);
    store.append_agent_event_v4(&first).await.unwrap();
    store.append_agent_event_v4(&second).await.unwrap();
    assert_eq!(
        store.agent_events_v4(run).await.unwrap(),
        vec![first, second]
    );
    assert_eq!(
        store.agent_run_v4(run).await.unwrap().unwrap()["objective"],
        "test"
    );
}

#[tokio::test]
async fn v4_tool_outcome_fractional_scores_keep_their_durable_hash() {
    let (store, project, conversation) = fixture().await;
    let run = Uuid::new_v4();
    save_run(&store, run, project.id, conversation.id).await;
    let first = AgentEventV4::first(
        run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    let finished = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::ToolFinished {
            outcome: ToolOutcomeV4 {
                call_id: "search".into(),
                tool_id: "search_mcp_tools".into(),
                succeeded: true,
                model_content: "search result".into(),
                data: json!({"tools":[{"score":6.0_f64 / 13.0_f64}]}),
                provenance: vec![],
            },
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    store.append_agent_event_v4(&finished).await.unwrap();
    assert_eq!(
        store.agent_events_v4(run).await.unwrap(),
        vec![first.clone(), finished.clone()]
    );
    assert_eq!(
        store
            .agent_events_for_context_v4(project.id, conversation.id)
            .await
            .unwrap(),
        vec![first, finished]
    );
}

#[tokio::test]
async fn v4_store_rejects_event_gaps() {
    let (store, project, conversation) = fixture().await;
    let run = Uuid::new_v4();
    save_run(&store, run, project.id, conversation.id).await;
    let first = AgentEventV4::first(
        run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    let second = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::CompletionProposed);
    assert!(store.append_agent_event_v4(&second).await.is_err());
}

#[tokio::test]
async fn v4_context_archive_is_durable_and_hash_addressed() {
    let (store, project, conversation) = fixture().await;
    let run_id = Uuid::new_v4();
    save_run(&store, run_id, project.id, conversation.id).await;
    let transcript = r#"[{"sequence":1,"event":"large"}]"#;
    let checkpoint = ContextCheckpointV4 {
        schema_version: 4,
        through_sequence: 17,
        completion_criteria: vec!["artifact verified".into()],
        unresolved_errors: vec!["one error".into()],
        recent_steps: vec!["inspect".into()],
        scientific_state: json!({}),
        task_shape: None,
        phase: None,
        task_revision: None,
        tasks: vec![],
        cycle_id: None,
    };
    let archive = store
        .archive_agent_context_v4(run_id, transcript, &checkpoint)
        .await
        .unwrap();
    assert_eq!(archive.size_bytes, transcript.len() as u64);
    assert_eq!(archive.sha256.len(), 64);
    assert_eq!(
        store
            .agent_context_archive_v4(archive.archive_id)
            .await
            .unwrap()
            .unwrap(),
        (transcript.into(), checkpoint)
    );
}

#[tokio::test]
async fn run_completion_atomically_persists_ordered_assistant_message() {
    let (store, project, conversation) = fixture().await;
    let run = Uuid::new_v4();
    save_run(&store, run, project.id, conversation.id).await;
    let user = Message::markdown(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        7,
        MessageRole::User,
        "run analysis",
        Utc::now(),
    );
    store.save_message(&user).await.unwrap();
    let first = AgentEventV4::first(
        run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    let proposal = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::CompletionProposalSubmitted {
            proposal: completion_proposal("internal verified summary", "## Verified result"),
        },
    );
    let completed = AgentEventV4::next(&proposal, Utc::now(), AgentEventKindV4::RunCompleted);
    store.append_agent_event_v4(&first).await.unwrap();
    store.append_agent_event_v4(&proposal).await.unwrap();
    store.append_agent_event_v4(&completed).await.unwrap();
    let messages = store
        .messages_for_conversation(conversation.id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].sequence, 7);
    assert_eq!(messages[1].id, run);
    assert_eq!(messages[1].sequence, 8);
    assert_eq!(messages[1].role, MessageRole::Assistant);
    assert_eq!(messages[1].markdown, "## Verified result");
    store.append_agent_event_v4(&completed).await.unwrap();
    assert_eq!(store.agent_events_v4(run).await.unwrap().len(), 3);
    assert_eq!(
        store
            .messages_for_conversation(conversation.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn legacy_completion_without_answer_markdown_falls_back_to_summary() {
    let (store, project, conversation) = fixture().await;
    let run = Uuid::new_v4();
    save_run(&store, run, project.id, conversation.id).await;
    let first = AgentEventV4::first(
        run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    let proposal = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::CompletionProposalSubmitted {
            proposal: completion_proposal("legacy summary", ""),
        },
    );
    let completed = AgentEventV4::next(&proposal, Utc::now(), AgentEventKindV4::RunCompleted);
    store.append_agent_event_v4(&first).await.unwrap();
    store.append_agent_event_v4(&proposal).await.unwrap();
    store.append_agent_event_v4(&completed).await.unwrap();
    let messages = store
        .messages_for_conversation(conversation.id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].markdown, "legacy summary");
}

#[tokio::test]
async fn empty_completion_answer_rolls_back_run_completed_event() {
    let (store, project, conversation) = fixture().await;
    let run = Uuid::new_v4();
    save_run(&store, run, project.id, conversation.id).await;
    let first = AgentEventV4::first(
        run,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    let proposal = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::CompletionProposalSubmitted {
            proposal: completion_proposal("  ", ""),
        },
    );
    let completed = AgentEventV4::next(&proposal, Utc::now(), AgentEventKindV4::RunCompleted);
    store.append_agent_event_v4(&first).await.unwrap();
    store.append_agent_event_v4(&proposal).await.unwrap();
    assert!(store.append_agent_event_v4(&completed).await.is_err());
    assert_eq!(store.agent_events_v4(run).await.unwrap().len(), 2);
    assert!(
        store
            .messages_for_conversation(conversation.id)
            .await
            .unwrap()
            .is_empty()
    );
}

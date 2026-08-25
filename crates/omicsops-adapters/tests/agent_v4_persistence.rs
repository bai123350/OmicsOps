use chrono::Utc;
use omicsops_adapters::persistence::Repository;
use omicsops_core::workspace::{Message, MessageRole};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, CompletionProposalV4, ContextCheckpointV4, RunModeV4,
    ToolOutcomeV4,
};
use serde_json::json;
use uuid::Uuid;

fn completion_proposal(summary: &str, answer_markdown: &str) -> CompletionProposalV4 {
    serde_json::from_value(json!({
        "schema_version": 4,
        "summary": summary,
        "criteria": [],
        "answer_markdown": answer_markdown
    }))
    .unwrap()
}

#[test]
fn v4_run_and_hash_chained_events_round_trip_independently() {
    let repository = Repository::open_in_memory().unwrap();
    let run = Uuid::new_v4();
    let project = Uuid::new_v4();
    let conversation = Uuid::new_v4();
    repository
        .save_agent_run_v4(
            run,
            project,
            conversation,
            "planning",
            &json!({"objective":"test"}),
        )
        .unwrap();
    let first = AgentEventV4::first(
        run,
        project,
        conversation,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    let second = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::CompletionProposed);
    repository.append_agent_event_v4(&first).unwrap();
    repository.append_agent_event_v4(&second).unwrap();
    assert_eq!(
        repository.agent_events_v4(run).unwrap(),
        vec![first, second]
    );
    assert_eq!(
        repository.agent_run_v4(run).unwrap().unwrap()["objective"],
        "test"
    );
}

#[test]
fn v4_tool_outcome_fractional_scores_keep_their_durable_hash() {
    let repository = Repository::open_in_memory().unwrap();
    let first = AgentEventV4::first(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
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
    repository.append_agent_event_v4(&first).unwrap();
    repository.append_agent_event_v4(&finished).unwrap();

    assert_eq!(
        repository.agent_events_v4(first.run_id).unwrap(),
        vec![first.clone(), finished.clone()]
    );
    assert_eq!(
        repository
            .agent_events_for_context_v4(first.project_id, first.conversation_id)
            .unwrap(),
        vec![first, finished]
    );
}

#[test]
fn v4_store_rejects_event_gaps() {
    let repository = Repository::open_in_memory().unwrap();
    let first = AgentEventV4::first(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    let second = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::CompletionProposed);
    assert!(repository.append_agent_event_v4(&second).is_err());
}

#[test]
fn v4_context_archive_is_durable_and_hash_addressed() {
    let repository = Repository::open_in_memory().unwrap();
    let run_id = Uuid::new_v4();
    let transcript = r#"[{"sequence":1,"event":"large"}]"#;
    let checkpoint = ContextCheckpointV4 {
        schema_version: 4,
        through_sequence: 17,
        completion_criteria: vec!["artifact verified".into()],
        unresolved_errors: vec!["one error".into()],
        recent_steps: vec!["inspect".into()],
        scientific_state: json!({}),
    };
    let archive = repository
        .archive_agent_context_v4(run_id, transcript, &checkpoint)
        .unwrap();
    assert_eq!(archive.size_bytes, transcript.len() as u64);
    assert_eq!(archive.sha256.len(), 64);
    assert_eq!(
        repository
            .agent_context_archive_v4(archive.archive_id)
            .unwrap()
            .unwrap(),
        (transcript.into(), checkpoint)
    );
}

#[test]
fn run_completion_atomically_persists_ordered_assistant_message() {
    let repository = Repository::open_in_memory().unwrap();
    let run = Uuid::new_v4();
    let project = Uuid::new_v4();
    let conversation = Uuid::new_v4();
    let user = Message::markdown(
        Uuid::new_v4(),
        project,
        conversation,
        7,
        MessageRole::User,
        "run analysis",
        Utc::now(),
    );
    repository.save_message(&user).unwrap();

    let first = AgentEventV4::first(
        run,
        project,
        conversation,
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
    repository.append_agent_event_v4(&first).unwrap();
    repository.append_agent_event_v4(&proposal).unwrap();
    repository.append_agent_event_v4(&completed).unwrap();

    let messages = repository.messages_for_conversation(conversation).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].sequence, 7);
    assert_eq!(messages[1].id, run);
    assert_eq!(messages[1].sequence, 8);
    assert_eq!(messages[1].role, MessageRole::Assistant);
    assert_eq!(messages[1].markdown, "## Verified result");

    // Replaying the terminal event neither duplicates the event nor the
    // assistant message, which is important during restart recovery.
    repository.append_agent_event_v4(&completed).unwrap();
    assert_eq!(repository.agent_events_v4(run).unwrap().len(), 3);
    assert_eq!(
        repository
            .messages_for_conversation(conversation)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn legacy_completion_without_answer_markdown_falls_back_to_summary() {
    let repository = Repository::open_in_memory().unwrap();
    let run = Uuid::new_v4();
    let project = Uuid::new_v4();
    let conversation = Uuid::new_v4();
    let first = AgentEventV4::first(
        run,
        project,
        conversation,
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
    repository.append_agent_event_v4(&first).unwrap();
    repository.append_agent_event_v4(&proposal).unwrap();
    repository.append_agent_event_v4(&completed).unwrap();

    let messages = repository.messages_for_conversation(conversation).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].markdown, "legacy summary");
}

#[test]
fn empty_completion_answer_rolls_back_run_completed_event() {
    let repository = Repository::open_in_memory().unwrap();
    let run = Uuid::new_v4();
    let project = Uuid::new_v4();
    let conversation = Uuid::new_v4();
    let first = AgentEventV4::first(
        run,
        project,
        conversation,
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
    repository.append_agent_event_v4(&first).unwrap();
    repository.append_agent_event_v4(&proposal).unwrap();
    assert!(repository.append_agent_event_v4(&completed).is_err());
    assert_eq!(repository.agent_events_v4(run).unwrap().len(), 2);
    assert!(
        repository
            .messages_for_conversation(conversation)
            .unwrap()
            .is_empty()
    );
}

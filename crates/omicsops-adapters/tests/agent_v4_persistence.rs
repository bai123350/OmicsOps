use chrono::Utc;
use omicsops_adapters::persistence::Repository;
use omicsops_protocol::{AgentEventKindV4, AgentEventV4, RunModeV4};
use serde_json::json;
use uuid::Uuid;

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

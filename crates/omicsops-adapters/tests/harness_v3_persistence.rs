use chrono::{TimeZone, Utc};
use omicsops_adapters::persistence::Repository;
use omicsops_agent::harness_v3::{
    AgentRunEventKindV3, AgentRunEventV3, AgentRunSpecV3, AgentRunStateV3, AgentSnapshotV3,
    CompletionCriterionV3,
};
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use uuid::Uuid;

fn fixture() -> (
    Repository,
    AgentRunSpecV3,
    Project,
    Conversation,
    chrono::DateTime<Utc>,
) {
    let repository = Repository::open_in_memory().unwrap();
    let at = Utc.with_ymd_and_hms(2026, 8, 16, 9, 0, 0).unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "Harness v3",
        "E:/Science/v3",
        ProjectTemplate::Blank,
        at,
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "PBMC dynamic analysis", at);
    repository.save_project(&project).unwrap();
    repository.save_conversation(&conversation).unwrap();
    let spec = AgentRunSpecV3::new(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        "Analyze",
        vec![CompletionCriterionV3::new("done", "finish")],
        vec![],
        vec![],
    );
    (repository, spec, project, conversation, at)
}

#[test]
fn append_only_events_round_trip_after_chain_validation() {
    let (repository, spec, _, _, at) = fixture();
    let first = AgentRunEventV3::first(&spec, at, AgentRunEventKindV3::RunStarted).unwrap();
    let second = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ModelStepStarted { step: 1 },
    )
    .unwrap();

    repository.append_agent_run_event_v3(&first).unwrap();
    repository.append_agent_run_event_v3(&second).unwrap();

    assert_eq!(
        repository.agent_run_events_v3(spec.run_id).unwrap(),
        vec![first, second]
    );
}

#[test]
fn append_rejects_gaps_wrong_previous_hash_and_tampering() {
    let (repository, spec, _, _, at) = fixture();
    let first = AgentRunEventV3::first(&spec, at, AgentRunEventKindV3::RunStarted).unwrap();
    repository.append_agent_run_event_v3(&first).unwrap();

    let mut gap = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ModelStepStarted { step: 1 },
    )
    .unwrap();
    gap.sequence = 3;
    assert!(repository.append_agent_run_event_v3(&gap).is_err());

    let mut tampered = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ModelStepStarted { step: 1 },
    )
    .unwrap();
    tampered.event = AgentRunEventKindV3::ModelStepStarted { step: 9 };
    assert!(repository.append_agent_run_event_v3(&tampered).is_err());
}

#[test]
fn snapshot_is_a_hash_bound_cache_and_may_lag_the_event_log() {
    let (repository, spec, _, _, at) = fixture();
    let first = AgentRunEventV3::first(&spec, at, AgentRunEventKindV3::RunStarted).unwrap();
    repository.append_agent_run_event_v3(&first).unwrap();
    let state = AgentRunStateV3::replay(&spec, std::slice::from_ref(&first)).unwrap();
    let snapshot = AgentSnapshotV3 {
        run_id: spec.run_id,
        project_id: spec.project_id,
        conversation_id: spec.conversation_id,
        last_sequence: first.sequence,
        last_event_hash: first.event_hash.clone(),
        state,
    };
    repository.save_agent_snapshot_v3(&snapshot).unwrap();

    let second = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ModelStepStarted { step: 1 },
    )
    .unwrap();
    repository.append_agent_run_event_v3(&second).unwrap();

    assert_eq!(
        repository.agent_snapshot_v3(spec.run_id).unwrap().unwrap(),
        snapshot
    );
    assert_eq!(
        repository.agent_run_events_v3(spec.run_id).unwrap().len(),
        2
    );

    let mut invalid = snapshot;
    invalid.last_event_hash = "0".repeat(64);
    assert!(repository.save_agent_snapshot_v3(&invalid).is_err());
}

#[test]
fn conversation_and_project_deletion_transactionally_clean_v3_run_state() {
    let (repository, spec, project, conversation, at) = fixture();
    let first = AgentRunEventV3::first(&spec, at, AgentRunEventKindV3::RunStarted).unwrap();
    repository.append_agent_run_event_v3(&first).unwrap();
    let snapshot = AgentSnapshotV3 {
        run_id: spec.run_id,
        project_id: spec.project_id,
        conversation_id: spec.conversation_id,
        last_sequence: first.sequence,
        last_event_hash: first.event_hash.clone(),
        state: AgentRunStateV3::replay(&spec, std::slice::from_ref(&first)).unwrap(),
    };
    repository.save_agent_snapshot_v3(&snapshot).unwrap();

    assert!(
        repository
            .delete_conversation(project.id, conversation.id)
            .unwrap()
    );
    assert!(
        repository
            .agent_run_events_v3(spec.run_id)
            .unwrap()
            .is_empty()
    );
    assert!(repository.agent_snapshot_v3(spec.run_id).unwrap().is_none());

    let conversation2 = Conversation::new(Uuid::new_v4(), project.id, "Second", at);
    repository.save_conversation(&conversation2).unwrap();
    let spec2 = AgentRunSpecV3::new(
        Uuid::new_v4(),
        project.id,
        conversation2.id,
        "Analyze again",
        vec![],
        vec![],
        vec![],
    );
    let event2 = AgentRunEventV3::first(&spec2, at, AgentRunEventKindV3::RunStarted).unwrap();
    repository.append_agent_run_event_v3(&event2).unwrap();

    assert!(repository.delete_project(project.id).unwrap());
    assert!(
        repository
            .agent_run_events_v3(spec2.run_id)
            .unwrap()
            .is_empty()
    );
}

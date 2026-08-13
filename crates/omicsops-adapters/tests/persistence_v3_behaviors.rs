use chrono::{TimeZone, Utc};
use omicsops_adapters::persistence::Repository;
use omicsops_agent::AgentEvent;
use omicsops_core::workspace::{
    AgentTurn, Artifact, Conversation, Message, MessageRole, ModelProfile, ModelProviderKind,
    NotebookEntry, NotebookEntryKind, Project, ProjectTemplate, SkillPackage, SyncDirection,
    SyncEntry, SyncState, TurnStatus,
};
use uuid::Uuid;

#[test]
fn schema_v3_round_trips_projects_and_conversations() {
    let repository = Repository::open_in_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "PBMC atlas",
        "E:/Science/pbmc-atlas",
        ProjectTemplate::SingleCellRnaSeq,
        now,
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "QC and clustering", now);

    repository.save_project(&project).unwrap();
    repository.save_conversation(&conversation).unwrap();

    assert_eq!(repository.schema_version().unwrap(), 3);
    assert_eq!(repository.list_projects().unwrap(), vec![project]);
    assert_eq!(
        repository
            .conversations_for_project(conversation.project_id)
            .unwrap(),
        vec![conversation]
    );
}

#[test]
fn conversation_messages_and_agent_events_are_replayed_in_sequence() {
    let repository = Repository::open_in_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "PBMC",
        "E:/PBMC",
        ProjectTemplate::SingleCellRnaSeq,
        now,
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "Analysis", now);
    repository.save_project(&project).unwrap();
    repository.save_conversation(&conversation).unwrap();
    let later = Message::markdown(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        2,
        MessageRole::Assistant,
        "计划就绪",
        now,
    );
    let earlier = Message::markdown(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        1,
        MessageRole::User,
        "检查 QC",
        now,
    );
    repository.save_message(&later).unwrap();
    repository.save_message(&earlier).unwrap();

    let turn_id = Uuid::new_v4();
    let mut second = AgentEvent::text_delta(project.id, conversation.id, turn_id, "B");
    second.sequence = 2;
    let mut first = AgentEvent::text_delta(project.id, conversation.id, turn_id, "A");
    first.sequence = 1;
    repository.append_agent_event(&second).unwrap();
    repository.append_agent_event(&first).unwrap();

    assert_eq!(
        repository
            .messages_for_conversation(conversation.id)
            .unwrap(),
        vec![earlier, later]
    );
    assert_eq!(
        repository.agent_events_for_turn(turn_id).unwrap(),
        vec![first, second]
    );
}

#[test]
fn opening_v2_database_creates_a_v2_backup_before_migration() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("omicsops.db");
    {
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 2; CREATE TABLE sentinel (id INTEGER);")
            .unwrap();
    }

    let repository = Repository::open(&database).unwrap();

    assert_eq!(repository.schema_version().unwrap(), 3);
    assert!(directory.path().join("omicsops.db.v2.bak").exists());
}

#[test]
fn project_research_records_round_trip_through_normalized_v3_tables() {
    let repository = Repository::open_in_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "PBMC",
        "E:/PBMC",
        ProjectTemplate::SingleCellRnaSeq,
        now,
    );
    repository.save_project(&project).unwrap();
    let notebook = NotebookEntry {
        id: Uuid::new_v4(),
        project_id: project.id,
        conversation_id: None,
        turn_id: None,
        kind: NotebookEntryKind::Decision,
        title: "QC threshold".into(),
        markdown: "Keep cells with >500 genes".into(),
        confidence: Some(0.8),
        evidence_ids: vec!["PMID:123".into()],
        artifact_ids: vec![],
        created_at: now,
        updated_at: now,
    };
    let artifact = Artifact {
        id: Uuid::new_v4(),
        project_id: project.id,
        run_id: None,
        relative_path: "results/umap.png".into(),
        remote_path: None,
        media_type: "image/png".into(),
        size_bytes: 42,
        sha256: "abc".into(),
        verified: true,
        created_at: now,
    };
    let sync = SyncEntry {
        id: Uuid::new_v4(),
        project_id: project.id,
        relative_path: "results/umap.png".into(),
        local_relative_path: Some("results/umap.png".into()),
        remote_path: Some("/srv/results/umap.png".into()),
        direction: SyncDirection::RemoteToLocal,
        size_bytes: 42,
        sha256: "abc".into(),
        state: SyncState::Synced,
        transferred_bytes: 42,
        retry_count: 0,
        error: None,
        updated_at: now,
    };
    let model = ModelProfile {
        id: Uuid::new_v4(),
        label: "Local".into(),
        provider: ModelProviderKind::Ollama,
        base_url: "http://127.0.0.1:11434".into(),
        model: "qwen3".into(),
        credential_reference: None,
        supports_tools: true,
        supports_vision: false,
    };
    let skill = SkillPackage {
        id: Uuid::new_v4(),
        name: "scRNA QC".into(),
        version: "1.0.0".into(),
        source_path: "skills/scrna-qc".into(),
        sha256: "def".into(),
        enabled: true,
        capabilities: vec!["read_local".into()],
    };
    repository.save_notebook_entry(&notebook).unwrap();
    repository.save_artifact_v3(&artifact).unwrap();
    repository.save_sync_entry(&sync).unwrap();
    repository.save_model_profile(&model).unwrap();
    repository.save_skill_package(&skill).unwrap();
    assert_eq!(
        repository.notebook_for_project(project.id).unwrap(),
        vec![notebook]
    );
    assert_eq!(
        repository.artifacts_for_project(project.id).unwrap(),
        vec![artifact]
    );
    assert_eq!(
        repository.sync_entries_for_project(project.id).unwrap(),
        vec![sync]
    );
    assert_eq!(repository.list_model_profiles().unwrap(), vec![model]);
    assert_eq!(repository.list_skill_packages().unwrap(), vec![skill]);
}

#[test]
fn agent_turn_state_is_persisted_for_restart_recovery() {
    let repository = Repository::open_in_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "PBMC",
        "E:/PBMC",
        ProjectTemplate::SingleCellRnaSeq,
        now,
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "QC", now);
    repository.save_project(&project).unwrap();
    repository.save_conversation(&conversation).unwrap();
    let turn = AgentTurn {
        id: Uuid::new_v4(),
        project_id: project.id,
        conversation_id: conversation.id,
        status: TurnStatus::Streaming,
        model_profile_id: Uuid::new_v4(),
        started_at: now,
        finished_at: None,
    };
    repository.save_agent_turn(&turn).unwrap();
    assert_eq!(
        repository
            .agent_turns_for_conversation(conversation.id)
            .unwrap(),
        vec![turn]
    );
}

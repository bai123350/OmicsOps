use chrono::{TimeZone, Utc};
use omicsops_adapters::persistence::Repository;
use omicsops_agent::AgentEvent;
use omicsops_core::domain::{RunEvent, RunState};
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
fn deleting_a_conversation_is_project_scoped_and_removes_chat_context() {
    let repository = Repository::open_in_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 14, 0, 0, 0).unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "PBMC",
        "E:/PBMC",
        ProjectTemplate::SingleCellRnaSeq,
        now,
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "QC", now);
    let turn = AgentTurn {
        id: Uuid::new_v4(),
        project_id: project.id,
        conversation_id: conversation.id,
        status: TurnStatus::Succeeded,
        model_profile_id: Uuid::new_v4(),
        started_at: now,
        finished_at: Some(now),
    };
    let message = Message::markdown(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        1,
        MessageRole::User,
        "检查 QC",
        now,
    );
    let event = AgentEvent::text_delta(project.id, conversation.id, turn.id, "完成");
    repository.save_project(&project).unwrap();
    repository.save_conversation(&conversation).unwrap();
    repository.save_message(&message).unwrap();
    repository.save_agent_turn(&turn).unwrap();
    repository.append_agent_event(&event).unwrap();

    assert!(
        !repository
            .delete_conversation(Uuid::new_v4(), conversation.id)
            .unwrap()
    );
    assert_eq!(
        repository
            .messages_for_conversation(conversation.id)
            .unwrap(),
        vec![message]
    );
    assert!(
        repository
            .delete_conversation(project.id, conversation.id)
            .unwrap()
    );
    assert!(
        repository
            .conversations_for_project(project.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .messages_for_conversation(conversation.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .agent_turns_for_conversation(conversation.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .agent_events_for_turn(turn.id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn deleting_a_project_cleans_its_records_but_preserves_other_projects_and_files() {
    let repository = Repository::open_in_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 14, 0, 0, 0).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let data_path = directory.path().join("matrix.h5ad");
    let metadata_dir = directory.path().join(".omicsops");
    let manifest_path = metadata_dir.join("project.json");
    std::fs::create_dir_all(&metadata_dir).unwrap();
    std::fs::write(&data_path, b"scientific data").unwrap();
    std::fs::write(&manifest_path, b"project metadata").unwrap();
    let target = Project::new(
        Uuid::new_v4(),
        "Delete me",
        directory.path().to_string_lossy(),
        ProjectTemplate::SingleCellRnaSeq,
        now,
    );
    let retained = Project::new(
        Uuid::new_v4(),
        "Keep me",
        "E:/Science/retained",
        ProjectTemplate::Blank,
        now,
    );
    repository.save_project(&target).unwrap();
    repository.save_project(&retained).unwrap();

    let target_conversation = Conversation::new(Uuid::new_v4(), target.id, "QC", now);
    let retained_conversation = Conversation::new(Uuid::new_v4(), retained.id, "Keep", now);
    repository.save_conversation(&target_conversation).unwrap();
    repository
        .save_conversation(&retained_conversation)
        .unwrap();
    let target_turn = AgentTurn {
        id: Uuid::new_v4(),
        project_id: target.id,
        conversation_id: target_conversation.id,
        status: TurnStatus::Succeeded,
        model_profile_id: Uuid::new_v4(),
        started_at: now,
        finished_at: Some(now),
    };
    repository.save_agent_turn(&target_turn).unwrap();
    repository
        .append_agent_event(&AgentEvent::text_delta(
            target.id,
            target_conversation.id,
            target_turn.id,
            "done",
        ))
        .unwrap();
    repository
        .save_message(&Message::markdown(
            Uuid::new_v4(),
            target.id,
            target_conversation.id,
            1,
            MessageRole::User,
            "run QC",
            now,
        ))
        .unwrap();

    let run_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    repository
        .put_json(
            "run_checkpoint",
            &run_id.to_string(),
            &serde_json::json!({
                "run_id": run_id,
                "project_id": target.id,
                "plan_id": plan_id
            }),
        )
        .unwrap();
    repository
        .put_json(
            "analysis_plan",
            &plan_id.to_string(),
            &serde_json::json!({"id": plan_id, "title": "QC"}),
        )
        .unwrap();
    repository
        .put_json(
            "artifact_v2",
            &format!("{run_id}:umap"),
            &serde_json::json!({"run_id": run_id, "remote_path": "/results/umap.png"}),
        )
        .unwrap();
    repository
        .put_json(
            "remote_agent_memory",
            "target-memory",
            &serde_json::json!({"project_id": target.id, "run_id": run_id}),
        )
        .unwrap();
    repository
        .put_json(
            "mcp_server",
            "global-mcp",
            &serde_json::json!({"name": "global"}),
        )
        .unwrap();
    repository
        .append_event(&RunEvent {
            sequence: 1,
            timestamp: now,
            run_id,
            stage_id: None,
            step_id: None,
            attempt: 0,
            action: "start".into(),
            state: RunState::Running,
            log_reference: None,
            reason: "test".into(),
        })
        .unwrap();

    let notebook = NotebookEntry {
        id: Uuid::new_v4(),
        project_id: target.id,
        conversation_id: Some(target_conversation.id),
        turn_id: Some(target_turn.id),
        kind: NotebookEntryKind::Observation,
        title: "QC".into(),
        markdown: "observed".into(),
        confidence: None,
        evidence_ids: vec![],
        artifact_ids: vec![],
        created_at: now,
        updated_at: now,
    };
    let artifact = Artifact {
        id: Uuid::new_v4(),
        project_id: target.id,
        run_id: Some(run_id),
        relative_path: "results/umap.png".into(),
        remote_path: Some("/results/umap.png".into()),
        media_type: "image/png".into(),
        size_bytes: 42,
        sha256: "abc".into(),
        verified: true,
        created_at: now,
    };
    let sync = SyncEntry {
        id: Uuid::new_v4(),
        project_id: target.id,
        relative_path: "results/umap.png".into(),
        local_relative_path: Some("results/umap.png".into()),
        remote_path: Some("/results/umap.png".into()),
        direction: SyncDirection::RemoteToLocal,
        size_bytes: 42,
        sha256: "abc".into(),
        state: SyncState::Synced,
        transferred_bytes: 42,
        retry_count: 0,
        error: None,
        updated_at: now,
    };
    repository.save_notebook_entry(&notebook).unwrap();
    repository.save_artifact_v3(&artifact).unwrap();
    repository.save_sync_entry(&sync).unwrap();

    assert_eq!(
        repository.run_ids_for_project(target.id).unwrap(),
        vec![run_id]
    );
    assert!(repository.delete_project(target.id).unwrap());
    assert!(!repository.delete_project(target.id).unwrap());
    assert!(repository.get_project(target.id).unwrap().is_none());
    assert!(repository.get_project(retained.id).unwrap().is_some());
    assert!(
        repository
            .conversations_for_project(target.id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        repository.conversations_for_project(retained.id).unwrap(),
        vec![retained_conversation]
    );
    assert!(repository.events_for_run(run_id).unwrap().is_empty());
    assert!(
        repository
            .notebook_for_project(target.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .artifacts_for_project(target.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .sync_entries_for_project(target.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .get_json::<serde_json::Value>("run_checkpoint", &run_id.to_string())
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .get_json::<serde_json::Value>("analysis_plan", &plan_id.to_string())
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .get_json::<serde_json::Value>("artifact_v2", &format!("{run_id}:umap"))
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .get_json::<serde_json::Value>("remote_agent_memory", "target-memory")
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .get_json::<serde_json::Value>("mcp_server", "global-mcp")
            .unwrap()
            .is_some()
    );
    assert_eq!(std::fs::read(&data_path).unwrap(), b"scientific data");
    assert_eq!(std::fs::read(&manifest_path).unwrap(), b"project metadata");
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
        context_window_tokens: None,
    };
    let skill = SkillPackage {
        id: Uuid::new_v4(),
        name: "scRNA QC".into(),
        version: "1.0.0".into(),
        source_path: "skills/scrna-qc".into(),
        sha256: "def".into(),
        enabled: true,
        capabilities: vec!["read_local".into()],
        category: Some("single_cell".into()),
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
    assert!(repository.has_active_agent_turns(project.id).unwrap());
    assert_eq!(
        repository
            .agent_turns_for_conversation(conversation.id)
            .unwrap(),
        vec![turn]
    );
}

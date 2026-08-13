use chrono::{TimeZone, Utc};
use omicsops_core::workspace::{
    Conversation, ConversationStatus, Message, MessageRole, NotebookEntry, NotebookEntryKind,
    Project, ProjectStatus, ProjectTemplate, SyncDirection, SyncEntry, SyncState,
    conflict_sibling_path,
};
use uuid::Uuid;

#[test]
fn workspace_entities_preserve_project_and_conversation_identity() {
    let now = Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let project = Project::new(
        project_id,
        "PBMC atlas",
        "E:/Science/pbmc-atlas",
        ProjectTemplate::SingleCellRnaSeq,
        now,
    );
    let conversation = Conversation::new(conversation_id, project.id, "QC and clustering", now);
    let message = Message::markdown(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        1,
        MessageRole::User,
        "Compare the PBMC batches",
        now,
    );

    assert_eq!(project.status, ProjectStatus::Ready);
    assert_eq!(project.local_root, "E:/Science/pbmc-atlas");
    assert_eq!(conversation.status, ConversationStatus::Idle);
    assert_eq!(message.project_id, project.id);
    assert_eq!(message.markdown, "Compare the PBMC batches");
}

#[test]
fn notebook_entries_link_evidence_and_artifacts_without_embedding_files() {
    let now = Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let entry = NotebookEntry {
        id: Uuid::nil(),
        project_id: Uuid::nil(),
        conversation_id: Some(Uuid::nil()),
        turn_id: None,
        kind: NotebookEntryKind::Decision,
        title: "Batch correction method".into(),
        markdown: "Use Harmony after QC diagnostics.".into(),
        confidence: Some(0.82),
        evidence_ids: vec!["pmid:123".into()],
        artifact_ids: vec![Uuid::nil()],
        created_at: now,
        updated_at: now,
    };

    assert_eq!(entry.evidence_ids, ["pmid:123"]);
    assert_eq!(entry.artifact_ids, [Uuid::nil()]);
}

#[test]
fn sync_conflicts_create_versioned_siblings_instead_of_overwriting() {
    let entry = SyncEntry {
        id: Uuid::nil(),
        project_id: Uuid::nil(),
        relative_path: "results/markers.csv".into(),
        local_relative_path: Some("results/markers.csv".into()),
        remote_path: Some("/srv/pbmc/results/markers.csv".into()),
        direction: SyncDirection::RemoteToLocal,
        size_bytes: 1024,
        sha256: "abc".into(),
        state: SyncState::Conflict,
        transferred_bytes: 512,
        retry_count: 1,
        error: None,
        updated_at: Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap(),
    };

    assert_eq!(
        conflict_sibling_path(&entry.relative_path, 2),
        "results/markers.conflict-2.csv"
    );
}

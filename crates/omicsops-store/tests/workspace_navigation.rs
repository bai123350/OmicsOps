use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::*;
use omicsops_store::Store;
use omicsops_store::workspace_navigation::{SaveLibraryRecord, SavePublicationRecord};
use sha2::{Digest, Sha256};
use uuid::Uuid;

async fn project(store: &Store, name: &str) -> (Project, Conversation) {
    let p = Project::new(
        Uuid::new_v4(),
        name,
        r"C:\data\navigation",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&p).await.unwrap();
    let c = Conversation::new(Uuid::new_v4(), p.id, "Source conversation", Utc::now());
    store.save_conversation(&c).await.unwrap();
    (p, c)
}

fn group_request(project_id: Uuid, name: &str) -> SaveGroupRequest {
    SaveGroupRequest {
        request_id: Uuid::new_v4(),
        project_id,
        group_id: None,
        name: name.into(),
    }
}

fn source(project_id: Uuid, conversation_id: Uuid) -> WorkspaceSourceRef {
    WorkspaceSourceRef {
        project_id,
        kind: SourceKind::Message,
        id: Uuid::new_v4().to_string(),
        conversation_id: Some(conversation_id),
        run_id: None,
        sequence: None,
        event_hash: None,
        content_sha256: None,
        start: None,
        end: None,
    }
}

fn snapshot(source: WorkspaceSourceRef, text: &str) -> WorkspaceSourceSnapshot {
    WorkspaceSourceSnapshot {
        source,
        title: "Saved source".into(),
        text: text.into(),
        sha256: "untrusted incoming hash".into(),
        status: "generated".into(),
        metadata: Default::default(),
        availability: SourceAvailability::Available,
    }
}

fn publication(project_id: Uuid, title: &str, markdown: &str) -> SavePublicationRecord {
    SavePublicationRecord {
        request: SavePublicationRequest {
            request_id: Uuid::new_v4(),
            project_id,
            publication_id: None,
            expected_revision: 0,
            title: title.into(),
            markdown: markdown.into(),
            sources: vec![],
        },
        references: vec![],
    }
}

fn library(p: &Project, c: &Conversation, title: &str, text: &str) -> SaveLibraryRecord {
    let source = source(p.id, c.id);
    SaveLibraryRecord {
        request: SaveLibraryItemRequest {
            request_id: Uuid::new_v4(),
            title: title.into(),
            kind: LibraryKind::Excerpt,
            source: source.clone(),
        },
        snapshot: snapshot(source, text),
        source_project_name: p.name.clone(),
        source_conversation_title: Some(c.title.clone()),
    }
}

fn library_query() -> ListLibraryRequest {
    ListLibraryRequest {
        query: String::new(),
        kind: None,
        project_id: None,
        offset: 0,
        limit: 20,
    }
}

#[tokio::test]
async fn journey_status_filter_applies_before_pagination() {
    use omicsops_protocol::{AgentEventKindV4, AgentEventV4, RunModeV4};
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Status project").await;
    let at = chrono::DateTime::parse_from_rfc3339("2026-09-21T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut failed = Uuid::nil();
    for index in 0..26 {
        let run = Uuid::new_v4();
        let status = if index == 0 {
            failed = run;
            "failed"
        } else {
            "running"
        };
        store
            .save_agent_run_v4(run, p.id, c.id, status, &serde_json::json!({}))
            .await
            .unwrap();
        let event = AgentEventV4::first(
            run,
            p.id,
            c.id,
            at + chrono::Duration::minutes(index),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        store.append_agent_event_v4(&event).await.unwrap();
    }
    let mut request = JourneyRequest {
        project_id: p.id,
        query: String::new(),
        kind: Some(SourceKind::Run),
        status: None,
        offset: 0,
        limit: 25,
    };
    let first = store.workspace_journey(&request).await.unwrap();
    assert_eq!(first.entries.len(), 25);
    assert!(first.entries.iter().all(|entry| entry.status == "running"));
    request.offset = 25;
    assert_eq!(
        store.workspace_journey(&request).await.unwrap().entries[0]
            .source
            .id,
        failed.to_string()
    );
    request.offset = 0;
    request.status = Some(" failed ".into());
    let filtered = store.workspace_journey(&request).await.unwrap();
    assert_eq!(
        filtered.entries.len(),
        1,
        "status must filter before applying the first-page limit"
    );
    assert_eq!(filtered.entries[0].source.id, failed.to_string());
    assert_eq!(filtered.next_offset, None);
}

#[tokio::test]
async fn library_source_projects_include_later_pages_and_ignore_current_filters() {
    let store = Store::open_in_memory().await.unwrap();
    let (old, old_conversation) = project(&store, "Old source").await;
    let record = library(&old, &old_conversation, "Old item", "Old text");
    let old_id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspace_library(id,kind,title,source_project_id,source_project_name,source_conversation_id,source_conversation_title,text_preview,snapshot_text,snapshot_json,created_at) VALUES(?,'artifact','Old item',?,?,?,?,'Old text','Old text',?,1)")
        .bind(old_id.to_string()).bind(old.id.to_string()).bind(&old.name).bind(old_conversation.id.to_string()).bind(&old_conversation.title).bind(serde_json::to_string(&record.snapshot).unwrap()).execute(store.pool()).await.unwrap();
    let (new, new_conversation) = project(&store, "New source").await;
    for index in 0..25 {
        store
            .workspace_save_library_item(&library(
                &new,
                &new_conversation,
                &format!("New item {index}"),
                "New text",
            ))
            .await
            .unwrap();
    }
    store.delete_project(old.id).await.unwrap();
    let mut request = ListLibraryRequest {
        limit: 25,
        ..library_query()
    };
    let first = store.workspace_list_library(&request).await.unwrap();
    assert!(
        first
            .items
            .iter()
            .all(|item| item.source_project_id == new.id)
    );
    assert_eq!(first.source_projects.len(), 2);
    assert!(
        first
            .source_projects
            .iter()
            .any(|p| p.id == old.id && p.name == "Old source")
    );
    request.offset = 25;
    assert_eq!(
        store.workspace_list_library(&request).await.unwrap().items[0].id,
        old_id
    );
    request.offset = 0;
    request.query = "New".into();
    request.kind = Some(LibraryKind::Excerpt);
    request.project_id = Some(new.id);
    let filtered = store.workspace_list_library(&request).await.unwrap();
    assert_eq!(
        filtered.source_projects, first.source_projects,
        "project choices must not depend on active query, kind, project, or page"
    );
}

#[tokio::test]
async fn library_source_project_overflow_returns_an_explicit_error() {
    let store = Store::open_in_memory().await.unwrap();
    sqlx::query("WITH RECURSIVE n(value) AS (SELECT 1 UNION ALL SELECT value+1 FROM n WHERE value<1001) INSERT INTO workspace_library(id,kind,title,source_project_id,source_project_name,text_preview,snapshot_text,snapshot_json,created_at) SELECT printf('00000000-0000-0000-0000-%012d',value),'excerpt','Saved item',printf('00000000-0000-0000-0000-%012d',value),'Saved source','','','{}',value FROM n")
        .execute(store.pool()).await.unwrap();
    let error = store
        .workspace_list_library(&library_query())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("1000 source projects"),
        "unexpected catalog limit error: {error}"
    );
}

#[tokio::test]
async fn initialization_adds_durable_navigation_tables() {
    let store = Store::open_in_memory().await.unwrap();
    for table in [
        "conversation_groups",
        "conversation_group_members",
        "workspace_request_ledger",
        "workspace_library",
    ] {
        assert!(
            store.has_table(table).await.unwrap(),
            "missing durable table: {table}"
        );
    }
}

#[tokio::test]
async fn legacy_restore_uses_host_redaction_before_new_revision_is_persisted() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, _) = project(&store, "Project").await;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO publications(id,project_id,title,status) VALUES(?,?,?,'draft')")
        .bind(id.to_string())
        .bind(p.id.to_string())
        .bind("Legacy")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO publication_revisions(id,publication_id,revision,content) VALUES(?,?,1,?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(id.to_string())
    .bind("password=restore-secret")
    .execute(store.pool())
    .await
    .unwrap();
    let request = RestorePublicationRequest {
        request_id: Uuid::new_v4(),
        project_id: p.id,
        publication_id: id,
        expected_revision: 1,
        revision: 1,
    };
    let restored = store
        .workspace_restore_publication_with_redaction(&request, |text| {
            text.replace("password=restore-secret", "password=[REDACTED]")
        })
        .await
        .unwrap();
    assert_eq!(restored.revisions[0].markdown, "password=[REDACTED]");
    assert!(!restored.revisions[0].legacy);
    let content: String = sqlx::query_scalar(
        "SELECT content FROM publication_revisions WHERE publication_id=? AND revision=2",
    )
    .bind(id.to_string())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(!content.contains("restore-secret"));
    let original: String = sqlx::query_scalar(
        "SELECT content FROM publication_revisions WHERE publication_id=? AND revision=1",
    )
    .bind(id.to_string())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(original, "password=restore-secret");
}

#[tokio::test]
async fn group_retries_are_stable_and_changed_payloads_conflict() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, _) = project(&store, "Project").await;
    let request = group_request(p.id, "  RNA 分析  ");
    let first = store.workspace_save_group(&request).await.unwrap();
    assert_eq!(first.name, "RNA 分析");
    assert_eq!(store.workspace_save_group(&request).await.unwrap(), first);
    let mut changed = request.clone();
    changed.name = "Changed".into();
    assert!(store.workspace_save_group(&changed).await.is_err());
    assert!(
        store
            .workspace_save_group(&group_request(p.id, "RNA 分析"))
            .await
            .is_err()
    );
    for name in ["".to_string(), " ".to_string(), "字".repeat(81)] {
        assert!(
            store
                .workspace_save_group(&group_request(p.id, &name))
                .await
                .is_err()
        );
    }
    let renamed = store
        .workspace_save_group(&SaveGroupRequest {
            request_id: Uuid::new_v4(),
            group_id: Some(first.id),
            name: "Renamed".into(),
            ..request.clone()
        })
        .await
        .unwrap();
    assert_eq!(renamed.id, first.id);
    assert_eq!(
        store.workspace_save_group(&request).await.unwrap(),
        first,
        "retry returns originally committed result"
    );
    assert_eq!(
        store.workspace_list_groups(p.id).await.unwrap().groups[0].name,
        "Renamed"
    );
}

#[tokio::test]
async fn cross_project_moves_are_atomic_and_group_deletion_preserves_conversations() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Project").await;
    let (other, other_c) = project(&store, "Other").await;
    let g = store
        .workspace_save_group(&group_request(p.id, "Group"))
        .await
        .unwrap();
    let move_request = MoveConversationsRequest {
        project_id: p.id,
        group_id: Some(g.id),
        conversation_ids: vec![c.id, other_c.id],
    };
    assert!(
        store
            .workspace_move_conversations(&move_request)
            .await
            .is_err()
    );
    assert!(
        store
            .workspace_list_groups(p.id)
            .await
            .unwrap()
            .memberships
            .is_empty()
    );
    assert!(
        store
            .workspace_move_conversations(&MoveConversationsRequest {
                project_id: other.id,
                conversation_ids: vec![other_c.id],
                ..move_request.clone()
            })
            .await
            .is_err()
    );
    assert!(
        store
            .workspace_move_conversations(&MoveConversationsRequest {
                conversation_ids: vec![c.id, Uuid::new_v4()],
                ..move_request.clone()
            })
            .await
            .is_err()
    );
    assert!(store.workspace_delete_group(other.id, g.id).await.is_err());
    store
        .workspace_move_conversations(&MoveConversationsRequest {
            conversation_ids: vec![c.id],
            ..move_request
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .workspace_list_groups(p.id)
            .await
            .unwrap()
            .memberships
            .len(),
        1
    );
    store.workspace_delete_group(p.id, g.id).await.unwrap();
    assert!(
        store
            .workspace_list_groups(p.id)
            .await
            .unwrap()
            .memberships
            .is_empty()
    );
    assert_eq!(
        store.conversations_for_project(p.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn membership_cascades_only_when_its_conversation_or_project_is_deleted() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Project").await;
    let g = store
        .workspace_save_group(&group_request(p.id, "Group"))
        .await
        .unwrap();
    store
        .workspace_move_conversations(&MoveConversationsRequest {
            project_id: p.id,
            group_id: Some(g.id),
            conversation_ids: vec![c.id],
        })
        .await
        .unwrap();
    store.delete_conversation(p.id, c.id).await.unwrap();
    let state = store.workspace_list_groups(p.id).await.unwrap();
    assert_eq!(state.groups.len(), 1);
    assert!(state.memberships.is_empty());
    store.delete_project(p.id).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversation_groups")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn publications_append_revisions_check_expected_head_and_replay_original_result() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, _) = project(&store, "Project").await;
    let first_record = publication(p.id, " First title ", "First text");
    let first = store
        .workspace_save_publication(&first_record)
        .await
        .unwrap();
    assert_eq!(first.publication.revision, 1);
    let mut second_record = publication(p.id, "Second title", "Second text");
    second_record.request.publication_id = Some(first.publication.id);
    second_record.request.expected_revision = 1;
    let second = store
        .workspace_save_publication(&second_record)
        .await
        .unwrap();
    assert_eq!(second.publication.revision, 2);
    assert_eq!(second.revisions[1], first.revisions[0]);
    assert_eq!(second.revisions[1].title, "First title");
    assert_eq!(
        store
            .workspace_save_publication(&first_record)
            .await
            .unwrap(),
        first
    );
    assert_eq!(
        store
            .workspace_replay_publication(&second_record.request)
            .await
            .unwrap(),
        Some(second.clone())
    );
    let mut stale = second_record.clone();
    stale.request.request_id = Uuid::new_v4();
    assert!(
        store
            .workspace_save_publication(&stale)
            .await
            .unwrap_err()
            .to_string()
            .contains("revision conflict")
    );
    let mut changed = first_record.clone();
    changed.request.markdown = "different".into();
    assert!(
        store
            .workspace_replay_publication(&changed.request)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .workspace_get_publication(p.id, first.publication.id)
            .await
            .unwrap(),
        second
    );
    assert_eq!(
        store.workspace_list_publications(p.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn publication_restore_appends_the_historical_title_and_preserves_old_bytes() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, _) = project(&store, "Project").await;
    let first = store
        .workspace_save_publication(&publication(p.id, "Historical", "Old body"))
        .await
        .unwrap();
    let mut edit = publication(p.id, "Current", "New body");
    edit.request.publication_id = Some(first.publication.id);
    edit.request.expected_revision = 1;
    store.workspace_save_publication(&edit).await.unwrap();
    let restore = RestorePublicationRequest {
        request_id: Uuid::new_v4(),
        project_id: p.id,
        publication_id: first.publication.id,
        expected_revision: 2,
        revision: 1,
    };
    let result = store.workspace_restore_publication(&restore).await.unwrap();
    assert_eq!(result.publication.title, "Historical");
    assert_eq!(result.publication.revision, 3);
    assert_eq!(result.revisions[0].markdown, "Old body");
    assert_eq!(result.revisions[2], first.revisions[0]);
    assert_eq!(
        store.workspace_restore_publication(&restore).await.unwrap(),
        result
    );
    let err = sqlx::query("UPDATE publication_revisions SET content='overwritten' WHERE id=?")
        .bind(first.revisions[0].id.to_string())
        .execute(store.pool())
        .await;
    assert!(
        err.is_err(),
        "revisions must remain append-only even through other SQL callers"
    );
}

#[tokio::test]
async fn legacy_and_unknown_publication_content_remain_readable_and_unmodified() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, _) = project(&store, "Project").await;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO publications(id,project_id,title,status) VALUES(?,?,?,'draft')")
        .bind(id.to_string())
        .bind(p.id.to_string())
        .bind("Legacy title")
        .execute(store.pool())
        .await
        .unwrap();
    for (revision, content) in [
        (1, "Legacy plain text"),
        (2, r#"{"schema_version":999,"future":"Keep all bytes"}"#),
    ] {
        sqlx::query(
            "INSERT INTO publication_revisions(id,publication_id,revision,content) VALUES(?,?,?,?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(id.to_string())
        .bind(revision)
        .bind(content)
        .execute(store.pool())
        .await
        .unwrap();
    }
    let detail = store.workspace_get_publication(p.id, id).await.unwrap();
    assert!(detail.revisions.iter().all(|r| r.legacy));
    assert_eq!(detail.revisions[1].markdown, "Legacy plain text");
    assert_eq!(
        detail.revisions[0].markdown,
        r#"{"schema_version":999,"future":"Keep all bytes"}"#
    );
    let mut new = publication(p.id, "New structured", "Editable separately");
    new.request.publication_id = Some(id);
    new.request.expected_revision = 2;
    let saved = store.workspace_save_publication(&new).await.unwrap();
    assert!(!saved.revisions[0].legacy);
    assert_eq!(saved.revisions[1].markdown, detail.revisions[0].markdown);
    assert_eq!(saved.revisions[1].title, "Legacy title");
    assert_eq!(saved.revisions[2].title, "Legacy title");
}

#[tokio::test]
async fn missing_publication_sources_can_only_reuse_the_exact_previous_snapshot() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Project").await;
    let mut first_record = publication(p.id, "Manuscript", "Before source deletion");
    let reference = snapshot(source(p.id, c.id), "Durable source");
    first_record.request.sources = vec![reference.source.clone()];
    first_record.references = vec![reference];
    let first = store
        .workspace_save_publication(&first_record)
        .await
        .unwrap();
    store.delete_conversation(p.id, c.id).await.unwrap();
    let mut edit = publication(p.id, "Manuscript", "After source deletion");
    edit.request.publication_id = Some(first.publication.id);
    edit.request.expected_revision = 1;
    edit.request.sources = first_record.request.sources;
    edit.references = first.revisions[0].references.clone();
    edit.references[0].availability = SourceAvailability::Missing;
    let mut forged = edit.clone();
    forged.references[0].text = "Forged replacement".into();
    assert!(store.workspace_save_publication(&forged).await.is_err());
    let saved = store.workspace_save_publication(&edit).await.unwrap();
    assert_eq!(saved.revisions[0].references[0].text, "Durable source");
    assert_eq!(
        saved.revisions[0].references[0].availability,
        SourceAvailability::Missing
    );
    let mut foreign_draft = edit.clone();
    foreign_draft.request.request_id = Uuid::new_v4();
    foreign_draft.request.publication_id = None;
    foreign_draft.request.expected_revision = 0;
    assert!(
        store
            .workspace_save_publication(&foreign_draft)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn concurrent_saves_allow_only_one_expected_head_and_one_retry_result() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("concurrent.sqlite"))
        .await
        .unwrap();
    let (p, c) = project(&store, "Project").await;
    let original = store
        .workspace_save_publication(&publication(p.id, "First", "Body"))
        .await
        .unwrap();
    let mut left = publication(p.id, "Left", "Left edit");
    left.request.publication_id = Some(original.publication.id);
    left.request.expected_revision = 1;
    let mut right = left.clone();
    right.request.request_id = Uuid::new_v4();
    right.request.title = "Right".into();
    let (left_result, right_result) = tokio::join!(
        store.workspace_save_publication(&left),
        store.workspace_save_publication(&right)
    );
    assert_ne!(left_result.is_ok(), right_result.is_ok());
    assert_eq!(
        store
            .workspace_get_publication(p.id, original.publication.id)
            .await
            .unwrap()
            .revisions
            .len(),
        2
    );
    let record = library(&p, &c, "Concurrent", "One snapshot");
    let (left_result, right_result) = tokio::join!(
        store.workspace_save_library_item(&record),
        store.workspace_save_library_item(&record)
    );
    assert_eq!(left_result.unwrap(), right_result.unwrap());
    assert_eq!(
        store
            .workspace_list_library(&library_query())
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    store.pool().close().await;
}

#[tokio::test]
async fn publication_references_reject_cross_project_and_redact_before_hashing() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Project").await;
    let (other, other_c) = project(&store, "Other").await;
    let mut record = publication(p.id, "api_key=TITLE", "api_key=BODY");
    let foreign = snapshot(source(other.id, other_c.id), "foreign");
    record.request.sources = vec![foreign.source.clone()];
    record.references = vec![foreign];
    assert!(store.workspace_save_publication(&record).await.is_err());
    let own = snapshot(source(p.id, c.id), "api_key=SOURCE");
    record.request.sources = vec![own.source.clone()];
    record.references = vec![own];
    let detail = store.workspace_save_publication(&record).await.unwrap();
    assert_eq!(detail.publication.title, "api_key=[REDACTED]");
    assert_eq!(detail.revisions[0].markdown, "api_key=[REDACTED]");
    let reference = &detail.revisions[0].references[0];
    assert_eq!(reference.text, "api_key=[REDACTED]");
    assert_eq!(
        reference.sha256,
        hex::encode(Sha256::digest(reference.text.as_bytes()))
    );
    assert!(
        store
            .workspace_get_publication(other.id, detail.publication.id)
            .await
            .is_err()
    );
    let content: String =
        sqlx::query_scalar("SELECT content FROM publication_revisions WHERE id=?")
            .bind(detail.revisions[0].id.to_string())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        detail.revisions[0].sha256,
        hex::encode(Sha256::digest(content.as_bytes()))
    );
    assert!(!content.contains("SOURCE"));
}

#[tokio::test]
async fn library_snapshots_and_source_names_survive_deletion_and_replay() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Source project").await;
    let record = library(&p, &c, " Snippet ", "api_key=SECRET\nresult <- 1");
    let saved = store.workspace_save_library_item(&record).await.unwrap();
    assert_eq!(saved.item.title, "Snippet");
    assert_eq!(saved.snapshot.text, "api_key=[REDACTED]\nresult <- 1");
    assert_eq!(
        saved.snapshot.sha256,
        hex::encode(Sha256::digest(saved.snapshot.text.as_bytes()))
    );
    let overwritten =
        sqlx::query("UPDATE workspace_library SET snapshot_text='changed' WHERE id=?")
            .bind(saved.item.id.to_string())
            .execute(store.pool())
            .await;
    assert!(overwritten.is_err());
    store.delete_project(p.id).await.unwrap();
    assert_eq!(
        store
            .workspace_get_library_item(saved.item.id)
            .await
            .unwrap(),
        saved
    );
    assert_eq!(
        store
            .workspace_replay_library_item(&record.request)
            .await
            .unwrap(),
        Some(saved.clone())
    );
    assert_eq!(
        store.workspace_save_library_item(&record).await.unwrap(),
        saved
    );
    assert_eq!(saved.item.source_project_name, "Source project");
    assert_eq!(
        saved.item.source_conversation_title.as_deref(),
        Some("Source conversation")
    );
    let mut changed = record.clone();
    changed.request.title = "Different".into();
    assert!(store.workspace_save_library_item(&changed).await.is_err());
}

#[tokio::test]
async fn library_search_is_literal_bounded_paginated_and_remove_preserves_source() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Project").await;
    let (other, other_c) = project(&store, "Other").await;
    let one = store
        .workspace_save_library_item(&library(&p, &c, "RNA", "gene%count"))
        .await
        .unwrap();
    store
        .workspace_save_library_item(&library(&p, &c, "RNA two", "different"))
        .await
        .unwrap();
    store
        .workspace_save_library_item(&library(&other, &other_c, "Other", "RNA"))
        .await
        .unwrap();
    let request = ListLibraryRequest {
        query: "RNA".into(),
        project_id: Some(p.id),
        limit: 1,
        ..library_query()
    };
    let first = store.workspace_list_library(&request).await.unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.next_offset, Some(1));
    let second = store
        .workspace_list_library(&ListLibraryRequest {
            offset: 1,
            ..request
        })
        .await
        .unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.next_offset, None);
    assert_ne!(first.items[0].id, second.items[0].id);
    let percent = store
        .workspace_list_library(&ListLibraryRequest {
            query: "%".into(),
            ..library_query()
        })
        .await
        .unwrap();
    assert_eq!(percent.items.len(), 1);
    assert_eq!(percent.items[0].id, one.item.id);
    assert!(
        store
            .workspace_list_library(&ListLibraryRequest {
                limit: 101,
                ..library_query()
            })
            .await
            .is_err()
    );
    assert!(
        store
            .workspace_list_library(&ListLibraryRequest {
                query: "x".repeat(513),
                ..library_query()
            })
            .await
            .is_err()
    );
    store
        .workspace_delete_library_item(one.item.id)
        .await
        .unwrap();
    assert!(store.workspace_get_library_item(one.item.id).await.is_err());
    assert_eq!(
        store.conversations_for_project(p.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn writes_reject_unbounded_text_and_snapshot_identity_mismatch() {
    let store = Store::open_in_memory().await.unwrap();
    let (p, c) = project(&store, "Project").await;
    let mut record = library(&p, &c, "Large", &"x".repeat(1_048_577));
    assert!(store.workspace_save_library_item(&record).await.is_err());
    record.snapshot.text = "small".into();
    record.snapshot.source.id = Uuid::new_v4().to_string();
    assert!(store.workspace_save_library_item(&record).await.is_err());
    assert!(
        store
            .workspace_save_publication(&publication(p.id, "Large", &"x".repeat(1_048_577)))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn navigation_persists_across_reopen_with_idempotent_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.sqlite");
    let store = Store::open(&path).await.unwrap();
    let (p, c) = project(&store, "Project").await;
    let request = group_request(p.id, "Persistent");
    let group = store.workspace_save_group(&request).await.unwrap();
    store
        .workspace_move_conversations(&MoveConversationsRequest {
            project_id: p.id,
            group_id: Some(group.id),
            conversation_ids: vec![c.id],
        })
        .await
        .unwrap();
    let published = store
        .workspace_save_publication(&publication(p.id, "Draft", "Persistent body"))
        .await
        .unwrap();
    let collected = store
        .workspace_save_library_item(&library(&p, &c, "Collection", "Persistent text"))
        .await
        .unwrap();
    store.pool().close().await;
    for _ in 0..2 {
        let reopened = Store::open(&path).await.unwrap();
        assert_eq!(
            reopened.workspace_save_group(&request).await.unwrap(),
            group
        );
        assert_eq!(
            reopened
                .workspace_list_groups(p.id)
                .await
                .unwrap()
                .memberships
                .len(),
            1
        );
        assert_eq!(
            reopened
                .workspace_get_publication(p.id, published.publication.id)
                .await
                .unwrap(),
            published
        );
        assert_eq!(
            reopened
                .workspace_get_library_item(collected.item.id)
                .await
                .unwrap(),
            collected
        );
        reopened.pool().close().await;
    }
}

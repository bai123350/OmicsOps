use chrono::Utc;
use omicsops_core::workspace::{Artifact, Conversation, Project, ProjectTemplate};
use omicsops_dto::{
    SavePublicationRequest, SourceAvailability, SourceKind, WorkspaceSourceRef,
    WorkspaceSourceSnapshot,
};
use omicsops_protocol::{AgentEventKindV4, AgentEventV4, RunModeV4, ToolCallV4};
use omicsops_store::{SavePublicationRecord, Store};
use serde_json::json;
use uuid::Uuid;

async fn project(store: &Store) -> Uuid {
    let id = Uuid::new_v4();
    store
        .save_project(&Project::new(
            id,
            "Publication evidence",
            r"C:\synthetic",
            ProjectTemplate::Blank,
            Utc::now(),
        ))
        .await
        .unwrap();
    id
}

async fn conversation(store: &Store, project_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    store
        .save_conversation(&Conversation::new(id, project_id, "Synthetic", Utc::now()))
        .await
        .unwrap();
    id
}

fn snapshot(project_id: Uuid, kind: SourceKind, id: String) -> WorkspaceSourceSnapshot {
    WorkspaceSourceSnapshot {
        source: WorkspaceSourceRef {
            project_id,
            kind,
            id,
            conversation_id: None,
            run_id: None,
            sequence: None,
            event_hash: None,
            content_sha256: None,
            start: None,
            end: None,
        },
        title: "Saved".into(),
        text: "saved source".into(),
        sha256: String::new(),
        status: "valid".into(),
        metadata: Default::default(),
        availability: SourceAvailability::Available,
    }
}

async fn save(
    store: &Store,
    project_id: Uuid,
    publication_id: Option<Uuid>,
    expected_revision: u64,
    reference: WorkspaceSourceSnapshot,
) -> Uuid {
    store
        .workspace_save_publication(&SavePublicationRecord {
            request: SavePublicationRequest {
                request_id: Uuid::new_v4(),
                project_id,
                publication_id,
                expected_revision,
                title: format!("Revision {}", expected_revision + 1),
                markdown: format!("body {}", expected_revision + 1),
                sources: vec![reference.source.clone()],
            },
            references: vec![reference],
        })
        .await
        .unwrap()
        .publication
        .id
}

#[tokio::test]
async fn context_keeps_missing_snapshot() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let reference = snapshot(p, SourceKind::Evidence, Uuid::new_v4().to_string());
    let publication = save(&store, p, None, 0, reference).await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert_eq!(context.revision.revision, 1);
    assert_eq!(context.revision.references[0].text, "saved source");
    assert_eq!(
        context.current_sources[0].availability,
        Some(SourceAvailability::Missing)
    );
}

#[tokio::test]
async fn context_selects_exact_revision() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let reference = snapshot(p, SourceKind::Evidence, Uuid::new_v4().to_string());
    let publication = save(&store, p, None, 0, reference.clone()).await;
    save(&store, p, Some(publication), 1, reference).await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert_eq!(context.revision.revision, 1);
    assert_eq!(context.revision.markdown, "body 1");
    assert_eq!(context.revision.references[0].text, "saved source");
    assert!(
        store
            .workspace_publication_read_context(p, publication, 3)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn context_rejects_foreign_owner() {
    let store = Store::open_in_memory().await.unwrap();
    let a = project(&store).await;
    let b = project(&store).await;
    let publication = save(
        &store,
        a,
        None,
        0,
        snapshot(a, SourceKind::Evidence, Uuid::new_v4().to_string()),
    )
    .await;
    assert!(
        store
            .workspace_publication_read_context(b, publication, 1)
            .await
            .is_err()
    );
    let foreign = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO scientific_evidence_v4(id,project_id,valid,value_json) VALUES(?,?,1,?)",
    )
    .bind(foreign.to_string())
    .bind(b.to_string())
    .bind(json!({"id":foreign,"project_id":b,"sources":[]}).to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let second = save(
        &store,
        a,
        None,
        0,
        snapshot(a, SourceKind::Evidence, foreign.to_string()),
    )
    .await;
    let context = store
        .workspace_publication_read_context(a, second, 1)
        .await
        .unwrap();
    assert!(context.related_records.is_empty());
    assert_eq!(
        context.current_sources[0].availability,
        Some(SourceAvailability::Missing)
    );
}

#[tokio::test]
async fn context_bounds_related_records() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let evidence_id = Uuid::new_v4();
    let ids: Vec<_> = (0..70).map(|_| Uuid::new_v4()).collect();
    sqlx::query("INSERT INTO scientific_evidence_v4(id,project_id,valid,value_json) VALUES(?,?,1,?)")
        .bind(evidence_id.to_string()).bind(p.to_string())
        .bind(json!({"id":evidence_id,"project_id":p,"sources":ids.iter().map(|id| json!({"kind":"artifact","artifact_id":id})).collect::<Vec<_>>()}).to_string())
        .execute(store.pool()).await.unwrap();
    for id in &ids {
        sqlx::query("INSERT INTO scientific_artifacts_v4(id,project_id,producer_analysis_id,valid,value_json) VALUES(?,?,?,1,?)")
            .bind(id.to_string()).bind(p.to_string()).bind(Uuid::new_v4().to_string())
            .bind(json!({"id":id,"project_id":p,"producer_analysis_id":Uuid::new_v4()}).to_string())
            .execute(store.pool()).await.unwrap();
    }
    let publication = save(
        &store,
        p,
        None,
        0,
        snapshot(p, SourceKind::Evidence, evidence_id.to_string()),
    )
    .await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert!(context.truncated);
    assert!(context.related_records.len() <= 64);
}

#[tokio::test]
async fn context_does_not_read_whole_state() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    sqlx::query("INSERT INTO scientific_states_v4(project_id,revision,state_sha256,value_json) VALUES(?,7,?,?)")
        .bind(p.to_string()).bind("a".repeat(64)).bind("not json ".repeat(700_000))
        .execute(store.pool()).await.unwrap();
    let publication = save(
        &store,
        p,
        None,
        0,
        snapshot(p, SourceKind::Evidence, Uuid::new_v4().to_string()),
    )
    .await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert_eq!(context.scientific_revision, Some(7));
    assert_eq!(context.scientific_state_sha256, Some("a".repeat(64)));
}

#[tokio::test]
async fn context_marks_oversized_record_unavailable_without_parsing_it() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let evidence = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO scientific_evidence_v4(id,project_id,valid,value_json) VALUES(?,?,1,?)",
    )
    .bind(evidence.to_string())
    .bind(p.to_string())
    .bind(json!({"id":evidence,"project_id":p,"padding":"x".repeat(262_145)}).to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let publication = save(
        &store,
        p,
        None,
        0,
        snapshot(p, SourceKind::Evidence, evidence.to_string()),
    )
    .await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert!(context.truncated);
    assert_eq!(
        context.current_sources[0].availability,
        Some(SourceAvailability::Unavailable)
    );
    assert!(context.related_records.is_empty());
}

#[tokio::test]
async fn context_bounds_total_scientific_json_bytes() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let mut references = Vec::new();
    for _ in 0..24 {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO scientific_evidence_v4(id,project_id,valid,value_json) VALUES(?,?,1,?)",
        )
        .bind(id.to_string())
        .bind(p.to_string())
        .bind(
            json!({"id":id,"project_id":p,"sources":[],"padding":"x".repeat(200_000)}).to_string(),
        )
        .execute(store.pool())
        .await
        .unwrap();
        references.push(snapshot(p, SourceKind::Evidence, id.to_string()));
    }
    let publication = store
        .workspace_save_publication(&SavePublicationRecord {
            request: SavePublicationRequest {
                request_id: Uuid::new_v4(),
                project_id: p,
                publication_id: None,
                expected_revision: 0,
                title: "Budget".into(),
                markdown: "Body".into(),
                sources: references.iter().map(|item| item.source.clone()).collect(),
            },
            references,
        })
        .await
        .unwrap()
        .publication
        .id;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert!(context.truncated);
    assert!(context.related_records.len() < 24);
    assert!(
        context
            .current_sources
            .iter()
            .any(|item| item.availability == Some(SourceAvailability::Unavailable))
    );
}

#[tokio::test]
async fn context_traverses_only_explicit_scientific_lineage() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let evidence = Uuid::new_v4();
    let artifact = Uuid::new_v4();
    let analysis = Uuid::new_v4();
    let dataset = Uuid::new_v4();
    let unrelated = Uuid::new_v4();
    let provenance = Uuid::new_v4();
    let run = Uuid::new_v4();
    sqlx::query("INSERT INTO scientific_evidence_v4(id,project_id,valid,value_json) VALUES(?,?,1,?)")
        .bind(evidence.to_string()).bind(p.to_string())
        .bind(json!({"id":evidence,"project_id":p,"sources":[{"kind":"artifact","artifact_id":artifact}],"source_call_id":"shared-call"}).to_string())
        .execute(store.pool()).await.unwrap();
    for id in [artifact, unrelated] {
        sqlx::query("INSERT INTO scientific_artifacts_v4(id,project_id,producer_analysis_id,valid,value_json) VALUES(?,?,?,1,?)")
            .bind(id.to_string()).bind(p.to_string()).bind(analysis.to_string())
            .bind(json!({"id":id,"project_id":p,"producer_analysis_id":analysis,"source_call_id":"shared-call"}).to_string())
            .execute(store.pool()).await.unwrap();
    }
    sqlx::query("INSERT INTO scientific_analyses_v4(id,project_id,status,value_json) VALUES(?,?,'completed',?)")
        .bind(analysis.to_string()).bind(p.to_string())
        .bind(json!({"id":analysis,"project_id":p,"run_id":run,"input_dataset_ids":[dataset]}).to_string())
        .execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO scientific_provenance_v4(id,project_id,run_id,analysis_id,complete,value_json) VALUES(?,?,?,?,0,?)")
        .bind(provenance.to_string()).bind(p.to_string()).bind(run.to_string()).bind(analysis.to_string())
        .bind(json!({"id":provenance,"project_id":p,"analysis_id":analysis,"run_id":run,"complete":false}).to_string())
        .execute(store.pool()).await.unwrap();
    sqlx::query(
        "INSERT INTO scientific_datasets_v4(id,project_id,active,value_json) VALUES(?,?,1,?)",
    )
    .bind(dataset.to_string())
    .bind(p.to_string())
    .bind(json!({"id":dataset,"project_id":p}).to_string())
    .execute(store.pool())
    .await
    .unwrap();
    let publication = save(
        &store,
        p,
        None,
        0,
        snapshot(p, SourceKind::Evidence, evidence.to_string()),
    )
    .await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert!(!context.truncated);
    assert_eq!(context.related_records.len(), 5);
    assert!(
        context
            .related_records
            .iter()
            .any(|item| item.id == provenance)
    );
    assert!(
        !context
            .related_records
            .iter()
            .any(|item| item.id == unrelated)
    );
    assert_eq!(
        context
            .related_records
            .iter()
            .find(|item| item.id == analysis)
            .unwrap()
            .run_owner_verified,
        Some(false)
    );
}

#[tokio::test]
async fn context_classifies_saved_tool_hash_change() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let c = conversation(&store, p).await;
    let run = Uuid::new_v4();
    store
        .save_agent_run_v4(run, p, c, "running", &json!({}))
        .await
        .unwrap();
    let call_id = "source-call".to_string();
    let event = AgentEventV4::first(
        run,
        p,
        c,
        Utc::now(),
        AgentEventKindV4::ToolRequested {
            call: ToolCallV4 {
                call_id: call_id.clone(),
                tool_id: "synthetic_tool".into(),
                arguments: json!({}),
            },
        },
    );
    store.append_agent_event_v4(&event).await.unwrap();
    let mut saved = snapshot(p, SourceKind::Tool, call_id);
    saved.source.run_id = Some(run);
    saved.source.conversation_id = Some(c);
    saved.source.sequence = Some(event.sequence);
    saved.source.event_hash = Some("a".repeat(64));
    assert_ne!(
        saved.source.event_hash.as_deref(),
        Some(event.event_hash.as_str())
    );
    let publication = save(&store, p, None, 0, saved).await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert_eq!(
        context.current_sources[0].availability,
        Some(SourceAvailability::Changed)
    );
}

#[tokio::test]
async fn context_run_projection_matches_existing_source_reader() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let c = conversation(&store, p).await;
    let run = Uuid::new_v4();
    store
        .save_agent_run_v4(run, p, c, "completed", &json!({}))
        .await
        .unwrap();
    let first = AgentEventV4::first(
        run,
        p,
        c,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    let last = AgentEventV4::next(
        &first,
        first.occurred_at + chrono::Duration::minutes(2),
        AgentEventKindV4::RunNeedsAttention {
            message: "Synthetic stop".into(),
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    store.append_agent_event_v4(&last).await.unwrap();
    let mut saved = snapshot(p, SourceKind::Run, run.to_string());
    saved.source.run_id = Some(run);
    saved.source.conversation_id = Some(c);
    let publication = save(&store, p, None, 0, saved).await;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    let existing = store
        .workspace_source_record(p, SourceKind::Run, &run.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(context.current_sources[0].availability, None);
    assert_eq!(context.current_sources[0].value.as_ref(), Some(&existing));
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(existing["started_at"].as_str().unwrap()).unwrap(),
        first.occurred_at
    );
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(existing["last_event_at"].as_str().unwrap()).unwrap(),
        last.occurred_at
    );
}

#[tokio::test]
async fn context_shared_artifact_producer_counts_scientific_bytes_once() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let c = conversation(&store, p).await;
    let run = Uuid::new_v4();
    store
        .save_agent_run_v4(run, p, c, "completed", &json!({}))
        .await
        .unwrap();
    let analysis = Uuid::new_v4();
    sqlx::query("INSERT INTO scientific_analyses_v4(id,project_id,status,value_json) VALUES(?,?,'completed',?)")
        .bind(analysis.to_string()).bind(p.to_string())
        .bind(json!({"id":analysis,"project_id":p,"run_id":run,"input_dataset_ids":[],"padding":"x".repeat(200_000)}).to_string())
        .execute(store.pool()).await.unwrap();
    let mut references = Vec::new();
    for _ in 0..16 {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO scientific_artifacts_v4(id,project_id,producer_analysis_id,valid,value_json) VALUES(?,?,?,1,?)")
            .bind(id.to_string()).bind(p.to_string()).bind(analysis.to_string())
            .bind(json!({"id":id,"project_id":p,"producer_analysis_id":analysis,"padding":"x".repeat(200_000)}).to_string())
            .execute(store.pool()).await.unwrap();
        let mut reference = snapshot(p, SourceKind::Artifact, id.to_string());
        reference.source.run_id = Some(run);
        reference.source.conversation_id = Some(c);
        references.push(reference);
    }
    let publication = store
        .workspace_save_publication(&SavePublicationRecord {
            request: SavePublicationRequest {
                request_id: Uuid::new_v4(),
                project_id: p,
                publication_id: None,
                expected_revision: 0,
                title: "Shared producer".into(),
                markdown: "Body".into(),
                sources: references.iter().map(|item| item.source.clone()).collect(),
            },
            references,
        })
        .await
        .unwrap()
        .publication
        .id;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    assert!(!context.truncated);
    assert_eq!(context.related_records.len(), 17);
    assert!(
        context
            .current_sources
            .iter()
            .all(|item| item.availability.is_none() && item.value.is_some())
    );
}

#[tokio::test]
async fn context_distinguishes_existing_run_bad_association_from_missing_source() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let c = conversation(&store, p).await;
    let foreign_project = project(&store).await;
    let foreign_conversation = conversation(&store, foreign_project).await;
    let run = Uuid::new_v4();
    store
        .save_agent_run_v4(run, p, c, "running", &json!({}))
        .await
        .unwrap();
    let publication = store
        .workspace_save_publication(&SavePublicationRecord {
            request: SavePublicationRequest {
                request_id: Uuid::new_v4(),
                project_id: p,
                publication_id: None,
                expected_revision: 0,
                title: "Association".into(),
                markdown: "Body".into(),
                sources: vec![],
            },
            references: vec![],
        })
        .await
        .unwrap()
        .publication
        .id;
    let mut wrong_run = snapshot(p, SourceKind::Run, run.to_string());
    wrong_run.source.run_id = Some(Uuid::new_v4());
    wrong_run.source.conversation_id = Some(c);
    let mut foreign_conversation_ref = snapshot(p, SourceKind::Run, run.to_string());
    foreign_conversation_ref.source.run_id = Some(run);
    foreign_conversation_ref.source.conversation_id = Some(foreign_conversation);
    let mut nonexistent_conversation_ref = snapshot(p, SourceKind::Run, run.to_string());
    nonexistent_conversation_ref.source.run_id = Some(run);
    nonexistent_conversation_ref.source.conversation_id = Some(Uuid::new_v4());
    let missing = snapshot(p, SourceKind::Run, Uuid::new_v4().to_string());
    let references = vec![
        wrong_run,
        foreign_conversation_ref,
        nonexistent_conversation_ref,
        missing,
    ];
    let content =
        json!({"schema_version":1,"title":"Association","markdown":"Body","references":references})
            .to_string();
    sqlx::query(
        "INSERT INTO publication_revisions(id,publication_id,revision,content) VALUES(?,?,2,?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(publication.to_string())
    .bind(content)
    .execute(store.pool())
    .await
    .unwrap();
    let context = store
        .workspace_publication_read_context(p, publication, 2)
        .await
        .unwrap();
    assert_eq!(context.revision.references.len(), 4);
    assert_eq!(
        context.current_sources[0].availability,
        Some(SourceAvailability::Unavailable)
    );
    assert_eq!(
        context.current_sources[1].availability,
        Some(SourceAvailability::Unavailable)
    );
    assert_eq!(
        context.current_sources[2].availability,
        Some(SourceAvailability::Unavailable)
    );
    assert_eq!(
        context.current_sources[3].availability,
        Some(SourceAvailability::Missing)
    );
}

#[tokio::test]
async fn context_preserves_accepted_legacy_artifact_associations() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let c = conversation(&store, p).await;
    let run = Uuid::new_v4();
    store
        .save_agent_run_v4(run, p, c, "running", &json!({}))
        .await
        .unwrap();

    let mut references = Vec::new();
    for (saved_run, saved_conversation) in
        [(Some(run), None), (None, Some(c)), (Some(run), Some(c))]
    {
        let artifact = Artifact {
            id: Uuid::new_v4(),
            project_id: p,
            run_id: None,
            relative_path: "synthetic.txt".into(),
            remote_path: None,
            media_type: "text/plain".into(),
            size_bytes: 12,
            sha256: "a".repeat(64),
            verified: true,
            created_at: Utc::now(),
        };
        store.save_artifact_v3(&artifact).await.unwrap();
        let mut reference = snapshot(p, SourceKind::LegacyArtifact, artifact.id.to_string());
        reference.source.run_id = saved_run;
        reference.source.conversation_id = saved_conversation;
        references.push(reference);
    }
    let mut absent = snapshot(p, SourceKind::LegacyArtifact, Uuid::new_v4().to_string());
    absent.source.run_id = Some(run);
    absent.source.conversation_id = Some(c);
    references.push(absent);

    // Exercise the existing save contract, including its run/conversation checks.
    let publication = store
        .workspace_save_publication(&SavePublicationRecord {
            request: SavePublicationRequest {
                request_id: Uuid::new_v4(),
                project_id: p,
                publication_id: None,
                expected_revision: 0,
                title: "Legacy artifact associations".into(),
                markdown: "Body".into(),
                sources: references.iter().map(|item| item.source.clone()).collect(),
            },
            references,
        })
        .await
        .unwrap()
        .publication
        .id;
    let context = store
        .workspace_publication_read_context(p, publication, 1)
        .await
        .unwrap();
    for item in &context.current_sources[..3] {
        assert_eq!(item.availability, None);
        assert_eq!(
            item.value,
            store
                .workspace_source_record(p, SourceKind::LegacyArtifact, &item.source.id)
                .await
                .unwrap()
        );
    }
    assert_eq!(
        context.current_sources[3].availability,
        Some(SourceAvailability::Missing)
    );

    let foreign_project = project(&store).await;
    let foreign_conversation = conversation(&store, foreign_project).await;
    let foreign_run = Uuid::new_v4();
    store
        .save_agent_run_v4(
            foreign_run,
            foreign_project,
            foreign_conversation,
            "running",
            &json!({}),
        )
        .await
        .unwrap();
    let existing_id = context.current_sources[0].source.id.clone();
    let mut wrong_run = snapshot(p, SourceKind::LegacyArtifact, existing_id.clone());
    wrong_run.source.run_id = Some(foreign_run);
    let mut wrong_conversation = snapshot(p, SourceKind::LegacyArtifact, existing_id);
    wrong_conversation.source.conversation_id = Some(foreign_conversation);
    let mut missing_with_wrong_selector =
        snapshot(p, SourceKind::LegacyArtifact, Uuid::new_v4().to_string());
    missing_with_wrong_selector.source.run_id = Some(foreign_run);
    let content = json!({
        "schema_version": 1,
        "title": "Historical associations",
        "markdown": "Body",
        "references": [wrong_run, wrong_conversation, missing_with_wrong_selector]
    })
    .to_string();
    sqlx::query(
        "INSERT INTO publication_revisions(id,publication_id,revision,content) VALUES(?,?,2,?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(publication.to_string())
    .bind(content)
    .execute(store.pool())
    .await
    .unwrap();
    let historical = store
        .workspace_publication_read_context(p, publication, 2)
        .await
        .unwrap();
    assert_eq!(
        historical.current_sources[0].availability,
        Some(SourceAvailability::Unavailable)
    );
    assert_eq!(
        historical.current_sources[1].availability,
        Some(SourceAvailability::Unavailable)
    );
    assert_eq!(
        historical.current_sources[2].availability,
        Some(SourceAvailability::Missing)
    );
}

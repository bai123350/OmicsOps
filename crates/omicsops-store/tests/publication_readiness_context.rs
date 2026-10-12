use chrono::Utc;
use omicsops_core::workspace::{Project, ProjectTemplate};
use omicsops_dto::{
    SavePublicationRequest, SourceAvailability, SourceKind, WorkspaceSourceRef,
    WorkspaceSourceSnapshot,
};
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

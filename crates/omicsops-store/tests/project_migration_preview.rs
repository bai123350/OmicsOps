use chrono::Utc;
use omicsops_core::workspace::{Conversation, ConversationStatus, Project, ProjectTemplate};
use omicsops_dto::{MigrationMetadataCategoryV1 as Category, MigrationRebindKindV1 as Rebind};
use omicsops_store::Store;
use uuid::Uuid;

async fn project(store: &Store) -> Project {
    let p = Project::new(Uuid::new_v4(), "Synthetic project", r"C:\private-root", ProjectTemplate::Blank, Utc::now());
    store.save_project(&p).await.unwrap();
    p
}

async fn exec(store: &Store, sql: &str, id: Uuid) {
    sqlx::query(sql).bind(id.to_string()).execute(store.pool()).await.unwrap();
}

// Snapshot the actual data and all JSON/body/credential boundaries that preview must not mutate.
async fn snapshot(store: &Store) -> Vec<Vec<String>> {
    let queries = [
        "SELECT quote(id)||quote(name)||quote(description)||quote(updated_at) FROM projects ORDER BY id",
        "SELECT quote(frame_id)||quote(title)||quote(status)||quote(updated_at) FROM conversation_records ORDER BY frame_id",
        "SELECT quote(conversation_id)||quote(visibility)||quote(revision)||quote(changed_at) FROM conversation_visibility_v4 ORDER BY conversation_id",
        "SELECT quote(run_id)||quote(status)||quote(value_json)||quote(updated_at) FROM agent_runs_v4 ORDER BY run_id",
        "SELECT quote(job_id)||quote(value_json) FROM runtime_jobs_v4 ORDER BY job_id",
        "SELECT quote(id)||quote(profile_json) FROM connections ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM model_profiles ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM skill_packages ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM app_objects ORDER BY id",
        "SELECT quote(project_id)||quote(revision)||quote(value_json) FROM scientific_states_v4 ORDER BY project_id",
        "SELECT quote(id)||quote(content)||quote(content_sha256) FROM publication_revisions ORDER BY id",
        "SELECT quote(id)||quote(content) FROM messages ORDER BY id",
        "SELECT quote(project_id)||quote(local_root)||quote(remote_root)||quote(connection_id) FROM project_omicsops ORDER BY project_id",
        "SELECT quote(id)||quote(config_json) FROM execution_contexts ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM scientific_datasets_v4 ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM scientific_analyses_v4 ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM scientific_artifacts_v4 ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM scientific_evidence_v4 ORDER BY id",
        "SELECT quote(id)||quote(value_json) FROM scientific_provenance_v4 ORDER BY id",
        "SELECT quote(request_id)||quote(result_json) FROM workspace_request_ledger ORDER BY request_id",
    ];
    let mut result = Vec::new();
    for query in queries { result.push(sqlx::query_scalar(query).fetch_all(store.pool()).await.unwrap()); }
    result
}

#[tokio::test]
async fn migration_metadata_empty_selection() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let c = Conversation::new(Uuid::new_v4(),p.id,"New conversation",Utc::now());
    store.save_conversation(&c).await.unwrap();
    let before = snapshot(&store).await;
    let empty = store.workspace_project_migration_metadata(p.id,&[]).await.unwrap();
    assert_eq!(empty.categories.len(),3);
    assert!(empty.categories.iter().all(|c| !c.selected && c.examples.is_empty() && !c.examples_truncated));
    assert_eq!(empty.categories[0].record_count,1);
    assert_eq!(empty.public_project.source_project_id,p.id);
    assert_eq!(empty.scientific_revision,None);
    assert_eq!(empty.categories[2].kind_counts.iter().map(|c| (c.kind.as_str(),c.record_count)).collect::<Vec<_>>(),vec![("dataset",0),("analysis",0),("artifact",0),("evidence",0),("provenance",0)]);
    assert!(empty.categories[..2].iter().all(|c|c.kind_counts.is_empty()));
    assert!(empty.exclusions.iter().any(|e|e.contains("include 0 shelved")));
    assert_eq!(snapshot(&store).await,before);
}

#[tokio::test]
async fn migration_metadata_selected_categories_bounded() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    for i in 0..25 {
        let c=Conversation::new(Uuid::new_v4(),p.id,format!("Conversation {i}"),Utc::now()); store.save_conversation(&c).await.unwrap();
        sqlx::query("INSERT INTO publications(id,project_id,title,status) VALUES(?,?,'password=title-secret','draft')").bind(format!("publication-{i:02}")).bind(p.id.to_string()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO publication_revisions(id,publication_id,revision,content,content_sha256) VALUES(?,?,1,'PRIVATE BODY','not-a-hash')").bind(format!("revision-{i}")).bind(format!("publication-{i:02}")).execute(store.pool()).await.unwrap();
        for table in ["scientific_datasets_v4","scientific_analyses_v4","scientific_artifacts_v4","scientific_evidence_v4","scientific_provenance_v4"] {
            let cols=match table {"scientific_datasets_v4"=>"active,value_json", "scientific_analyses_v4"=>"status,value_json", "scientific_artifacts_v4"=>"producer_analysis_id,valid,value_json", "scientific_evidence_v4"=>"valid,value_json", _=>"run_id,analysis_id,complete,value_json"};
            let vals=match table {"scientific_datasets_v4"|"scientific_evidence_v4"=>"1,'MALFORMED PRIVATE JSON'", "scientific_analyses_v4"=>"'succeeded','MALFORMED PRIVATE JSON'", "scientific_artifacts_v4"=>"'private-analysis',1,'MALFORMED PRIVATE JSON'", _=>"'private-run','private-analysis',1,'MALFORMED PRIVATE JSON'"};
            sqlx::query(&format!("INSERT INTO {table}(id,project_id,{cols}) VALUES(?,?,{vals})")).bind(format!("item-{i:02}")).bind(p.id.to_string()).execute(store.pool()).await.unwrap();
        }
    }
    exec(&store,"INSERT INTO scientific_states_v4(project_id,revision,state_sha256,value_json) VALUES(?,7,'header-hash','NOT JSON')",p.id).await;
    sqlx::query("INSERT INTO publication_revisions(id,publication_id,revision,content,content_sha256) VALUES('latest','publication-00',2,'NEW PRIVATE BODY',?)").bind("a".repeat(64)).execute(store.pool()).await.unwrap();
    let selected=store.workspace_project_migration_metadata(p.id,&[Category::Conversations,Category::Publications,Category::ScientificMetadata,Category::Conversations]).await.unwrap();
    assert_eq!(selected.categories.len(),3);
    assert_eq!(selected.categories.iter().map(|c|c.record_count).collect::<Vec<_>>(),vec![25,25,125]);
    assert!(selected.categories.iter().all(|c|c.selected && c.examples.len()==20 && c.examples_truncated));
    assert_eq!(selected.scientific_revision,Some(7));
    assert_eq!(selected.categories[2].kind_counts.iter().map(|c|c.record_count).collect::<Vec<_>>(),vec![25;5]);
    let science=&selected.categories[2].examples;
    for kind in ["dataset","analysis","artifact","evidence","provenance"] { assert!(science.iter().any(|e|e.kind==kind),"missing {kind}"); }
    let public=serde_json::to_string(&selected.categories).unwrap();
    for secret in ["title-secret","PRIVATE BODY","PRIVATE JSON","private-run","private-analysis"] {assert!(!public.contains(secret));}
    let current=selected.categories[1].examples.iter().find(|e|e.source_id=="publication-00").unwrap();
    assert_eq!(current.revision,Some(2));
    assert!(current.label.ends_with(&format!(" [sha256: {}]","a".repeat(64))));
    assert!(!public.contains("not-a-hash"));
    assert!(selected.categories[1].examples.iter().filter(|e|e.source_id!="publication-00").all(|e|e.revision==Some(1)));
}

#[tokio::test]
async fn migration_metadata_all_sessions_and_safe_bindings() {
    let store = Store::open_in_memory().await.unwrap();
    let mut p=project(&store).await; let other=project(&store).await;
    let connection=Uuid::new_v4(); let profile=Uuid::new_v4();
    sqlx::query("INSERT INTO connections(id,label,profile_json) VALUES(?,'private-host','NOT JSON password=connection-secret')").bind(connection.to_string()).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO model_profiles(id,value_json) VALUES(?,'NOT JSON api_key=model-secret')").bind(profile.to_string()).execute(store.pool()).await.unwrap();
    p.connection_id=Some(connection); p.remote_root=Some("/private/remote".into()); store.save_project(&p).await.unwrap();
    for i in 0..3 {
        let mut c=Conversation::new(Uuid::new_v4(),p.id,"password=conversation-secret",Utc::now());
        c.model_profile_id=Some(profile); if i==2 { c.status=ConversationStatus::Archived; } store.save_conversation(&c).await.unwrap();
        if i==1 {sqlx::query("INSERT INTO conversation_visibility_v4 VALUES(?,?,'shelved',1,1)").bind(c.id.to_string()).bind(p.id.to_string()).execute(store.pool()).await.unwrap();}
        if i==0 {
            sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES('private-run',?,?,'running','NOT JSON token=run-secret')").bind(p.id.to_string()).bind(c.id.to_string()).execute(store.pool()).await.unwrap();
            sqlx::query("INSERT INTO runtime_jobs_v4 VALUES('private-job','private-run','private-call','NOT JSON recovery-secret')").execute(store.pool()).await.unwrap();
        }
    }
    exec(&store,"INSERT INTO execution_contexts(id,project_id,kind,label,location,status,config_json) VALUES('private-context',?,'ssh','private-label','private-location','ready','NOT JSON')",p.id).await;
    sqlx::query("INSERT INTO app_objects(kind,id,value_json,updated_at) VALUES('mcp_server','global-server','NOT JSON token=mcp-secret',0)").execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO skill_packages VALUES('global-package','NOT JSON password=skill-secret')").execute(store.pool()).await.unwrap();
    let before=snapshot(&store).await;
    let selected=store.workspace_project_migration_metadata(p.id,&[Category::Conversations]).await.unwrap();
    assert_eq!(selected.categories[0].record_count,3); assert_eq!(selected.shelved_conversation_count,1);
    assert!(selected.exclusions.iter().any(|e|e.contains("include 1 shelved") && e.contains("Legacy archived")));
    assert!(selected.categories[0].examples.iter().any(|e|e.status=="archived"));
    let count=|kind| selected.rebind.iter().find(|r|r.kind==kind).unwrap().count;
    assert_eq!(count(Rebind::ProjectPaths),2); assert_eq!(count(Rebind::SshConnection),1); assert_eq!(count(Rebind::ModelProfile),1); assert_eq!(count(Rebind::KernelEnvironment),1); assert_eq!(count(Rebind::RuntimeJobs),1);
    let unrelated=store.workspace_project_migration_metadata(other.id,&[]).await.unwrap();
    for kind in [Rebind::McpServer,Rebind::SkillGrant] {
        let binding=selected.rebind.iter().find(|r|r.kind==kind).unwrap(); assert_eq!(binding.count,1); assert!(binding.message.contains("application-global registered")); assert!(binding.message.contains("not project grants"));
        assert_eq!(unrelated.rebind.iter().find(|r|r.kind==kind).unwrap().count,1);
    }
    assert_eq!(unrelated.rebind.iter().find(|r|r.kind==Rebind::RuntimeJobs).unwrap().count,0);
    let public=serde_json::json!({"project":selected.public_project,"categories":selected.categories,"rebind":selected.rebind,"exclusions":selected.exclusions}).to_string();
    for secret in ["credential_reference","private-root","private/remote","private-host","private-run","private-job","private-context","-secret","private-location"] { assert!(!public.contains(secret),"leaked {secret}"); }
    assert_eq!(snapshot(&store).await,before);
}

#[tokio::test]
async fn migration_metadata_owner_and_readonly() {
    let store=Store::open_in_memory().await.unwrap(); let mut p=project(&store).await; let other=project(&store).await;
    p.name="界".repeat(300); p.description=format!("password=description-secret\n{}", "界".repeat(2000)); store.save_project(&p).await.unwrap();
    store.save_conversation(&Conversation::new(Uuid::new_v4(),other.id,"OTHER PROJECT",Utc::now())).await.unwrap();
    let before=snapshot(&store).await;
    // SQLite itself rejects writes on every connection used by the real Store path.
    sqlx::query("PRAGMA query_only=ON").execute(store.pool()).await.unwrap();
    assert!(sqlx::query("UPDATE projects SET name=name WHERE id=?").bind(p.id.to_string()).execute(store.pool()).await.is_err());
    let result=store.workspace_project_migration_metadata(p.id,&[Category::Conversations]).await.unwrap();
    assert!(result.public_project.name.len()<=800 && result.public_project.description.len()<=4096);
    assert!(result.exclusions.iter().any(|e| e.contains("truncated")));
    assert_eq!(result.categories[0].record_count,0);
    assert!(store.workspace_project_migration_metadata(Uuid::new_v4(),&[]).await.is_err());
    assert_eq!(snapshot(&store).await,before);
}

#[tokio::test]
async fn migration_metadata_unselected_summary_read_boundary() {
    let store=Store::open_in_memory().await.unwrap(); let p=project(&store).await;
    store.save_conversation(&Conversation::new(Uuid::new_v4(),p.id,"Synthetic",Utc::now())).await.unwrap();
    sqlx::query("ALTER TABLE conversation_records RENAME TO fixture_conversations").execute(store.pool()).await.unwrap();
    sqlx::query("CREATE VIEW conversation_records AS SELECT frame_id,project_id,json_extract('MALFORMED','$') AS title,status,model_profile_id,created_at,updated_at FROM fixture_conversations").execute(store.pool()).await.unwrap();
    // An actual selected-summary query reaches the failing expression. COUNT and
    // safe binding queries for the unselected category must never evaluate it.
    assert!(store.workspace_project_migration_metadata(p.id,&[Category::Conversations]).await.is_err());
    let result=store.workspace_project_migration_metadata(p.id,&[]).await.unwrap();
    assert_eq!(result.categories[0].record_count,1);
    assert!(result.categories[0].examples.is_empty());
}

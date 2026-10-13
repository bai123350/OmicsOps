use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::*;
use omicsops_store::{SaveFileBaselineRecord, Store};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn hash(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

async fn project(store: &Store) -> Project {
    let project = Project::new(
        Uuid::new_v4(),
        "synthetic baseline",
        r"C:\synthetic",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    project
}

fn record(project_id: Uuid, branch: Option<Uuid>, text_bytes: u64) -> SaveFileBaselineRecord {
    let entries: Vec<FileBaselineEntryV1> = if text_bytes == 0 {
        vec![serde_json::from_value(serde_json::json!({"backend_id":"local", "relative_path":null,"content_mode":"reference_only","size_bytes":null,"hash":null,"omission":"sensitive_content","captured_at":"2026-10-13T00:00:00Z"})).unwrap()]
    } else {
        (0..8).map(|i| serde_json::from_value(serde_json::json!({"backend_id":"local","relative_path":format!("scripts/{i}.py"),"content_mode":"text_snapshot","size_bytes":text_bytes/8,"hash":{"sha256":hash(b"synthetic bytes"),"scope":"full_file"},"omission":null,"captured_at":"2026-10-13T00:00:00Z"})).unwrap()).collect()
    };
    let id = Uuid::new_v4();
    let manifest = serde_json::json!({"schema_version":1,"id":id,"project_id":project_id,"branch_conversation_id":branch,"title":"Synthetic baseline","entries":entries.iter().map(|entry| serde_json::json!({"entry":entry,"blob_sha256":entry.hash.as_ref().map(|h| &h.sha256)})).collect::<Vec<_>>(),"captured_text_bytes":text_bytes,"created_at":"2026-10-13T00:00:00Z","local_root_binding_sha256":hash(b"synthetic root binding")}).to_string();
    let baseline = FileBaselineV1 {
        schema_version: 1,
        id,
        project_id,
        branch_conversation_id: branch,
        title: "Synthetic baseline".into(),
        entries,
        manifest_sha256: hash(manifest.as_bytes()),
        captured_text_bytes: text_bytes,
        created_at: "2026-10-13T00:00:00Z".into(),
    };
    SaveFileBaselineRecord {
        request_id: Uuid::new_v4(),
        request_sha256: hash(b"canonical request bytes"),
        baseline,
        internal_manifest_json: manifest,
    }
}

#[tokio::test]
async fn baseline_request_replay() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let original = record(p.id, None, 8);
    let receipt = store
        .workspace_save_file_baseline(original.clone())
        .await
        .unwrap();
    let mut retry = original.clone();
    retry.baseline.id = Uuid::new_v4(); // Replay precedes validating a recapture.
    let replay = store.workspace_save_file_baseline(retry).await.unwrap();
    assert_eq!(replay, receipt);
    assert_eq!(
        store
            .workspace_find_file_baseline_request(original.request_id, &original.request_sha256)
            .await
            .unwrap(),
        Some(receipt)
    );
    let stored_digest: String = sqlx::query_scalar(
        "SELECT request_sha256 FROM workspace_request_ledger WHERE request_id=?",
    )
    .bind(original.request_id.to_string())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(stored_digest, original.request_sha256);
}

#[tokio::test]
async fn baseline_request_conflict() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let original = record(p.id, None, 0);
    store
        .workspace_save_file_baseline(original.clone())
        .await
        .unwrap();
    assert!(
        store
            .workspace_find_file_baseline_request(original.request_id, &hash(b"different request"))
            .await
            .is_err()
    );
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspace_request_ledger(request_id,operation,request_sha256,result_json,created_at) VALUES(?,'save_group',?,'{}',0)").bind(id.to_string()).bind(&original.request_sha256).execute(store.pool()).await.unwrap();
    assert!(
        store
            .workspace_find_file_baseline_request(id, &original.request_sha256)
            .await
            .is_err()
    );
}

async fn branch(store: &Store, p: &Project) -> Uuid {
    let source = Conversation::new(Uuid::new_v4(), p.id, "source", Utc::now());
    let child = Conversation::new(Uuid::new_v4(), p.id, "branch", Utc::now());
    store.save_conversation(&source).await.unwrap();
    store.save_conversation(&child).await.unwrap();
    sqlx::query("INSERT INTO conversation_branches_v4(request_id,branch_conversation_id,project_id,source_conversation_id,source_message_id,checkpoint_kind,source_sequence,source_head_sequence,boundary_hash,request_hash,state,created_at,updated_at) VALUES(?,?,?,?,?,'before_user',0,0,?,?,'active',0,0)").bind(Uuid::new_v4().to_string()).bind(child.id.to_string()).bind(p.id.to_string()).bind(source.id.to_string()).bind(Uuid::new_v4().to_string()).bind(hash(b"boundary")).bind(hash(b"request")).execute(store.pool()).await.unwrap();
    child.id
}

#[tokio::test]
async fn baseline_foreign_branch_rejected() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let foreign = project(&store).await;
    let b = branch(&store, &foreign).await;
    assert!(
        store
            .workspace_save_file_baseline(record(p.id, Some(b), 0))
            .await
            .is_err()
    );
    let plain = Conversation::new(Uuid::new_v4(), p.id, "plain", Utc::now());
    store.save_conversation(&plain).await.unwrap();
    assert!(
        store
            .workspace_save_file_baseline(record(p.id, Some(plain.id), 0))
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM exploration_baselines_v4")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn baseline_branch_unlink() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let b = branch(&store, &p).await;
    let original = record(p.id, Some(b), 8);
    let receipt = store
        .workspace_save_file_baseline(original.clone())
        .await
        .unwrap();
    for state in ["archived", "merged"] {
        sqlx::query("UPDATE conversation_branches_v4 SET state=? WHERE branch_conversation_id=?")
            .bind(state)
            .bind(b.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            store
                .workspace_get_file_baseline(p.id, receipt.id)
                .await
                .unwrap()
                .unwrap()
                .baseline
                .branch_conversation_id,
            Some(b)
        );
    }
    store.delete_conversation(p.id, b).await.unwrap();
    let after = store
        .workspace_get_file_baseline(p.id, receipt.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.baseline.branch_conversation_id, None);
    assert_eq!(after.baseline.manifest_sha256, receipt.manifest_sha256);
    let manifest: String =
        sqlx::query_scalar("SELECT manifest_json FROM exploration_baselines_v4 WHERE id=?")
            .bind(receipt.id.to_string())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(manifest, original.internal_manifest_json);
    assert_eq!(
        store
            .workspace_find_file_baseline_request(original.request_id, &original.request_sha256)
            .await
            .unwrap()
            .unwrap(),
        after.baseline
    );
    let foreign = project(&store).await;
    assert!(
        store
            .workspace_get_file_baseline(foreign.id, receipt.id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn baseline_quotas() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    assert!(
        store
            .workspace_save_file_baseline(record(p.id, None, 4 * 1024 * 1024 + 8))
            .await
            .is_err()
    );
    for _ in 0..64 {
        store
            .workspace_save_file_baseline(record(p.id, None, 4 * 1024 * 1024))
            .await
            .unwrap();
    }
    assert!(
        store
            .workspace_save_file_baseline(record(p.id, None, 0))
            .await
            .is_err()
    );
    let page = store
        .workspace_list_file_baselines(&ListFileBaselinesRequestV1 {
            project_id: p.id,
            branch_conversation_id: None,
            offset: 0,
            limit: 7,
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 7);
    assert_eq!(page.next_offset, Some(7));
    let other = project(&store).await;
    store
        .workspace_save_file_baseline(record(other.id, None, 0))
        .await
        .unwrap();
}

#[tokio::test]
async fn baseline_migration_twice_keeps_legacy() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("synthetic.sqlite");
    let store = Store::open(&path).await.unwrap();
    let p = project(&store).await;
    sqlx::query("INSERT INTO workspace_snapshots(id,project_id,workspace_dir,digest,manifest_json,created_at) VALUES(?,?,'synthetic','legacy','{\"legacy\":true}',0)").bind(Uuid::new_v4().to_string()).bind(p.id.to_string()).execute(store.pool()).await.unwrap();
    sqlx::query("DROP TABLE exploration_baselines_v4")
        .execute(store.pool())
        .await
        .unwrap();
    store.pool().close().await;
    let store = Store::open(&path).await.unwrap();
    let receipt = store
        .workspace_save_file_baseline(record(p.id, None, 8))
        .await
        .unwrap();
    store.pool().close().await;
    for _ in 0..2 {
        let reopened = Store::open(&path).await.unwrap();
        assert_eq!(
            reopened
                .workspace_get_file_baseline(p.id, receipt.id)
                .await
                .unwrap()
                .unwrap()
                .baseline,
            receipt
        );
        let legacy: Vec<String> =
            sqlx::query_scalar("SELECT manifest_json FROM workspace_snapshots")
                .fetch_all(reopened.pool())
                .await
                .unwrap();
        assert_eq!(legacy, vec!["{\"legacy\":true}"]);
        reopened.pool().close().await;
    }
}

#[tokio::test]
async fn baseline_invalid_manifests_never_write_metadata_or_ledger() {
    let store = Store::open_in_memory().await.unwrap();
    let p = project(&store).await;
    let original = record(p.id, None, 8);
    for field in ["blob", "scope", "path", "size", "unknown", "binding"] {
        let mut bad = original.clone();
        bad.request_id = Uuid::new_v4();
        let mut json: serde_json::Value =
            serde_json::from_str(&bad.internal_manifest_json).unwrap();
        match field {
            "blob" => json["entries"][0]["blob_sha256"] = serde_json::Value::Null,
            "scope" => {
                json["entries"][0]["entry"]["hash"]["scope"] = serde_json::json!("recorded_file");
                bad.baseline.entries[0].hash.as_mut().unwrap().scope =
                    FileReferenceHashScopeV1::RecordedFile;
            }
            "path" => {
                json["entries"][0]["entry"]["relative_path"] = serde_json::Value::Null;
                bad.baseline.entries[0].relative_path = None;
            }
            "size" => {
                json["captured_text_bytes"] = serde_json::json!(9);
                bad.baseline.captured_text_bytes = 9;
            }
            "unknown" => json["source_body"] = serde_json::json!("synthetic forbidden body"),
            "binding" => json["local_root_binding_sha256"] = serde_json::json!("C:\\synthetic"),
            _ => unreachable!(),
        }
        bad.internal_manifest_json = json.to_string();
        bad.baseline.manifest_sha256 = hash(bad.internal_manifest_json.as_bytes());
        assert!(
            store
                .workspace_save_file_baseline(bad.clone())
                .await
                .is_err(),
            "{field}"
        );
        assert_eq!(
            store
                .workspace_find_file_baseline_request(bad.request_id, &bad.request_sha256)
                .await
                .unwrap(),
            None
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM exploration_baselines_v4")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn baseline_migration_rejects_foreign_current_association() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("synthetic.sqlite");
    let store = Store::open(&path).await.unwrap();
    let p = project(&store).await;
    let other = project(&store).await;
    let own_branch = branch(&store, &p).await;
    let foreign = branch(&store, &other).await;
    let receipt = store
        .workspace_save_file_baseline(record(p.id, Some(own_branch), 0))
        .await
        .unwrap();
    let manifest: String =
        sqlx::query_scalar("SELECT manifest_json FROM exploration_baselines_v4 WHERE id=?")
            .bind(receipt.id.to_string())
            .fetch_one(store.pool())
            .await
            .unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    manifest["branch_conversation_id"] = serde_json::json!(foreign);
    let manifest = manifest.to_string();
    sqlx::query("UPDATE exploration_baselines_v4 SET branch_conversation_id=?,manifest_json=?,manifest_sha256=? WHERE id=?")
        .bind(foreign.to_string()).bind(&manifest).bind(hash(manifest.as_bytes()))
        .bind(receipt.id.to_string()).execute(store.pool()).await.unwrap();
    store.pool().close().await;
    let error = match Store::open(&path).await {
        Ok(_) => panic!("foreign branch accepted"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("file baseline branch ownership mismatch")
    );
}

#[tokio::test]
async fn baseline_migration_rejects_malformed_request_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("synthetic.sqlite");
    let store = Store::open(&path).await.unwrap();
    let p = project(&store).await;
    let receipt = store
        .workspace_save_file_baseline(record(p.id, None, 0))
        .await
        .unwrap();
    sqlx::query("UPDATE exploration_baselines_v4 SET request_id='invalid-uuid' WHERE id=?")
        .bind(receipt.id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    store.pool().close().await;
    assert!(Store::open(&path).await.is_err());
}

#[tokio::test]
async fn baseline_recorded_reference_is_bounded_and_checks_exact_provenance() {
    let store = Store::open_in_memory().await.unwrap();
    let mut p = project(&store).await;
    let connection = Uuid::from_u128(9);
    store
        .save_connection(&omicsops_core::domain::ConnectionProfile {
            id: connection,
            label: "fixture".into(),
            host: "example.invalid".into(),
            port: 22,
            username: "fixture".into(),
            authentication: omicsops_core::domain::AuthenticationMethod::Password,
            authentication_reference: "fixture-ref".into(),
            host_key_fingerprint: None,
        })
        .await
        .unwrap();
    p.connection_id = Some(connection);
    p.remote_root = Some("/project".into());
    store.save_project(&p).await.unwrap();
    let backend = format!("ssh:{connection}");
    let analysis = Uuid::new_v4();
    let artifact = Uuid::new_v4();
    let a=serde_json::json!({"id":artifact,"project_id":p.id,"relative_path":"a.txt","producer_analysis_id":analysis,"size_bytes":7,"sha256":"a".repeat(64)}).to_string();
    let n = serde_json::json!({"id":analysis,"project_id":p.id,"runtime":{"backend_id":backend}})
        .to_string();
    sqlx::query(
        "INSERT INTO scientific_analyses_v4(id,project_id,status,value_json) VALUES(?,?,?,?)",
    )
    .bind(analysis.to_string())
    .bind(p.id.to_string())
    .bind("succeeded")
    .bind(&n)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO scientific_artifacts_v4(id,project_id,producer_analysis_id,valid,value_json) VALUES(?,?,?,1,?)").bind(artifact.to_string()).bind(p.id.to_string()).bind(analysis.to_string()).bind(&a).execute(store.pool()).await.unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (Some(7), Some("a".repeat(64)))
    );
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, "ssh:other", "a.txt")
            .await
            .unwrap(),
        (None, None)
    );
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "other.txt")
            .await
            .unwrap(),
        (None, None)
    );
    let mut value: serde_json::Value = serde_json::from_str(&a).unwrap();
    value["project_id"] = serde_json::json!(Uuid::new_v4());
    sqlx::query("UPDATE scientific_artifacts_v4 SET value_json=? WHERE id=?")
        .bind(value.to_string())
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (None, None)
    );
    sqlx::query("UPDATE scientific_artifacts_v4 SET value_json=? WHERE id=?")
        .bind("{malformed")
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (None, None)
    );
    sqlx::query("UPDATE scientific_artifacts_v4 SET value_json=? WHERE id=?")
        .bind(&a)
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let mut unknown_hash: serde_json::Value = serde_json::from_str(&a).unwrap();
    unknown_hash["sha256"] = serde_json::json!("not-a-hash");
    sqlx::query("UPDATE scientific_artifacts_v4 SET value_json=? WHERE id=?")
        .bind(unknown_hash.to_string())
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (Some(7), None)
    );
    let mut unknown_size: serde_json::Value = serde_json::from_str(&a).unwrap();
    unknown_size["size_bytes"] = serde_json::json!("unknown");
    sqlx::query("UPDATE scientific_artifacts_v4 SET value_json=? WHERE id=?")
        .bind(unknown_size.to_string())
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (None, Some("a".repeat(64)))
    );
    sqlx::query("UPDATE scientific_artifacts_v4 SET value_json=? WHERE id=?")
        .bind(format!("{}{}", a, " ".repeat(65536)))
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (None, None)
    );
    sqlx::query("UPDATE scientific_artifacts_v4 SET value_json=? WHERE id=?")
        .bind(&a)
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let second = Uuid::new_v4();
    let mut conflicting: serde_json::Value = serde_json::from_str(&a).unwrap();
    conflicting["id"] = serde_json::json!(second);
    conflicting["size_bytes"] = serde_json::json!(8);
    sqlx::query("INSERT INTO scientific_artifacts_v4(id,project_id,producer_analysis_id,valid,value_json) VALUES(?,?,?,1,?)").bind(second.to_string()).bind(p.id.to_string()).bind(analysis.to_string()).bind(conflicting.to_string()).execute(store.pool()).await.unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (None, None)
    );
    sqlx::query("DELETE FROM scientific_artifacts_v4 WHERE id=?")
        .bind(second.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let mut bad_identity: serde_json::Value = serde_json::from_str(&a).unwrap();
    bad_identity["id"] = serde_json::json!("malformed-row-id");
    sqlx::query("UPDATE scientific_artifacts_v4 SET id='malformed-row-id',value_json=? WHERE id=?")
        .bind(bad_identity.to_string())
        .bind(artifact.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (None, None)
    );
    sqlx::query("UPDATE scientific_artifacts_v4 SET id=?,value_json=? WHERE id='malformed-row-id'")
        .bind(artifact.to_string())
        .bind(&a)
        .execute(store.pool())
        .await
        .unwrap();
    p.connection_id = None;
    p.remote_root = None;
    store.save_project(&p).await.unwrap();
    assert_eq!(
        store
            .workspace_file_recorded_reference(p.id, &backend, "a.txt")
            .await
            .unwrap(),
        (None, None)
    );
}

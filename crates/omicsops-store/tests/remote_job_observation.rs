use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_protocol::*;
use omicsops_store::{Store, StoreError};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

async fn fixture(store: &Store, project_id: Option<Uuid>) -> RunSpecV4 {
    let project_id = match project_id {
        Some(id) => id,
        None => {
            let project = Project::new(
                Uuid::new_v4(),
                "jobs",
                "synthetic",
                ProjectTemplate::Blank,
                Utc::now(),
            );
            store.save_project(&project).await.unwrap();
            project.id
        }
    };
    let conversation = Conversation::new(Uuid::new_v4(), project_id, "jobs", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "test".into(),
        steps: vec!["compute".into()],
        completion_criteria: vec!["result".into()],
        requested_capabilities: Default::default(),
    };
    let selection = ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "ssh:old-binding".into(),
        backend_kind: ComputeBackendKindV4::Ssh,
        autonomy_mode: AutonomyModeV4::Supervised,
        approval_policy: ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: NetworkPolicyV4::HostInherited,
        container_image: None,
    };
    let run_id = Uuid::new_v4();
    let model_id = Uuid::new_v4();
    let hash = RunSpecV4::approval_hash_for(
        run_id,
        project_id,
        conversation.id,
        model_id,
        &plan,
        &selection,
    )
    .unwrap();
    let spec = RunSpecV4::freeze_with_compute(
        run_id,
        project_id,
        conversation.id,
        model_id,
        plan,
        selection,
        &hash,
        Utc::now(),
    )
    .unwrap();
    store
        .save_agent_run_v4_if_unlocked(
            run_id,
            project_id,
            conversation.id,
            "running",
            &json!({"spec":spec}),
        )
        .await
        .unwrap();
    store
        .append_agent_event_v4(&AgentEventV4::first(
            run_id,
            project_id,
            conversation.id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        ))
        .await
        .unwrap();
    spec
}

async fn append(store: &Store, run_id: Uuid, kind: AgentEventKindV4) {
    let previous = store.agent_events_v4(run_id).await.unwrap().pop().unwrap();
    store
        .append_agent_event_v4(&AgentEventV4::next(&previous, Utc::now(), kind))
        .await
        .unwrap();
}

async fn reserve(
    store: &Store,
    spec: &RunSpecV4,
    name: &str,
    detached: bool,
) -> (RuntimeJobV4, ToolCallV4) {
    let call = ToolCallV4 {
        call_id: name.into(),
        tool_id: "runtime.execute".into(),
        arguments: json!({"language":"python","environment":"system","background":detached,"code":"synthetic code"}),
    };
    append(
        store,
        spec.run_id,
        AgentEventKindV4::ToolRequested { call: call.clone() },
    )
    .await;
    append(
        store,
        spec.run_id,
        AgentEventKindV4::ToolDispatchStarted {
            call_id: name.into(),
            tool_id: call.tool_id.clone(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: name.into(),
        },
    )
    .await;
    let context = ExecutionContextKeyV4 {
        project_id: spec.project_id,
        run_id: spec.run_id,
        backend_id: "ssh:old-binding".into(),
        language: KernelLanguageV4::Python,
        environment: "system".into(),
    };
    let (job, acquired) = store
        .reserve_runtime_job_with_remote_root_v4(
            &context,
            name,
            &call.canonical_hash().unwrap(),
            detached.then_some("/synthetic/old-root"),
            detached.then_some("SHA256:synthetic"),
        )
        .await
        .unwrap();
    assert!(acquired);
    (job, call)
}

async fn terminal(store: &Store, job: &RuntimeJobV4) -> RuntimeJobV4 {
    let running = store
        .advance_runtime_job_v4(job, RuntimeJobStateV4::Running, Some(job.job_id), None)
        .await
        .unwrap();
    let result = RuntimeResultV4 {
        request_id: job.job_id,
        session_id: job.job_id,
        process_identity: format!("ssh-detached:{}", job.job_id),
        stdout: "synthetic output".into(),
        stderr: String::new(),
        stdout_capture: None,
        stderr_capture: None,
        succeeded: true,
        artifacts: vec![],
        software_versions: Default::default(),
    };
    store
        .advance_runtime_job_v4(&running, RuntimeJobStateV4::Succeeded, None, Some(&result))
        .await
        .unwrap()
}

async fn set_job(store: &Store, id: Uuid, value: &Value) {
    sqlx::query("UPDATE runtime_jobs_v4 SET value_json=?1 WHERE job_id=?2")
        .bind(value.to_string())
        .bind(id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
}

async fn snapshot(store: &Store) -> Vec<(String, String)> {
    sqlx::query_as("SELECT 'job',value_json FROM runtime_jobs_v4 UNION ALL SELECT 'run',value_json FROM agent_runs_v4 UNION ALL SELECT 'event',value_json FROM agent_events_v4 UNION ALL SELECT 'receipt',result_json FROM runtime_job_results_v4 ORDER BY 1,2").fetch_all(store.pool()).await.unwrap()
}

#[tokio::test]
async fn metadata_filters_project_and_conversation_before_limit_and_keeps_old_bindings() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("jobs.sqlite")).await.unwrap();
    let a = fixture(&store, None).await;
    let (original, _) = reserve(&store, &a, "original", true).await;
    reserve(&store, &a, "interactive", false).await;
    let b = fixture(&store, Some(a.project_id)).await;
    for index in 0..101 {
        reserve(&store, &b, &format!("other-{index}"), true).await;
    }
    let other = fixture(&store, None).await;
    reserve(&store, &other, "foreign", true).await;
    let before = snapshot(&store).await;
    let metadata = store
        .remote_job_metadata_v4(a.project_id, Some(a.conversation_id))
        .await
        .unwrap();
    assert_eq!(metadata.len(), 1);
    assert_eq!(
        metadata
            .iter()
            .filter(|item| item.conversation_id == a.conversation_id)
            .count(),
        metadata.len()
    );
    assert_eq!(metadata[0].job, original);
    assert_eq!(
        store
            .remote_job_metadata_v4(a.project_id, None)
            .await
            .unwrap()
            .len(),
        100
    );
    assert!(
        store
            .remote_job_metadata_v4(other.project_id, Some(a.conversation_id))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .remote_job_record_v4(other.project_id, original.job_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .remote_job_record_v4(a.project_id, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(snapshot(&store).await, before);
}

#[tokio::test]
async fn selected_record_loads_original_spec_request_and_optional_receipt_without_mutation() {
    let store = Store::open_in_memory().await.unwrap();
    let spec = fixture(&store, None).await;
    let (job, call) = reserve(&store, &spec, "original", true).await;
    let before = snapshot(&store).await;
    let loaded = store
        .remote_job_record_v4(spec.project_id, job.job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.job.request_sha256, call.canonical_hash().unwrap());
    assert_eq!(loaded.frozen_spec, spec);
    assert_eq!(loaded.conversation_id, spec.conversation_id);
    assert!(loaded.result.is_none());
    assert_eq!(snapshot(&store).await, before);
    terminal(&store, &job).await;
    let before = snapshot(&store).await;
    assert!(
        store
            .remote_job_record_v4(spec.project_id, job.job_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .unwrap()
            .succeeded
    );
    assert_eq!(snapshot(&store).await, before);
    append(
        &store,
        spec.run_id,
        AgentEventKindV4::ToolFinished {
            outcome: ToolOutcomeV4 {
                call_id: job.call_id.clone(),
                tool_id: "runtime.execute".into(),
                succeeded: true,
                model_content: "done".into(),
                data: json!({}),
                provenance: vec![],
            },
        },
    )
    .await;
    assert!(
        store
            .remote_job_record_v4(spec.project_id, job.job_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .is_none()
    );
}

#[tokio::test]
async fn selected_record_rejects_tampered_scope_spec_event_chain_context_and_original_hash() {
    for field in [
        "project",
        "run",
        "job",
        "call",
        "environment",
        "backend",
        "language",
        "request",
        "spec",
        "event",
        "conversation",
    ] {
        let store = Store::open_in_memory().await.unwrap();
        let spec = fixture(&store, None).await;
        let (job, _) = reserve(&store, &spec, "original", true).await;
        let mut value = serde_json::to_value(&job).unwrap();
        match field {
            "project" => value["context"]["project_id"] = json!(Uuid::new_v4()),
            "run" => value["context"]["run_id"] = json!(Uuid::new_v4()),
            "job" => value["job_id"] = json!(Uuid::new_v4()),
            "call" => value["call_id"] = json!("tampered"),
            "environment" => value["context"]["environment"] = json!("tampered"),
            "backend" => value["context"]["backend_id"] = json!("ssh:tampered"),
            "language" => value["context"]["language"] = json!("r"),
            "request" => value["request_sha256"] = json!("b".repeat(64)),
            "spec" => {
                sqlx::query("UPDATE agent_runs_v4 SET value_json=json_set(value_json,'$.spec.plan.objective','tampered-sensitive-marker') WHERE run_id=?1").bind(spec.run_id.to_string()).execute(store.pool()).await.unwrap();
            }
            "event" => {
                sqlx::query("UPDATE agent_events_v4 SET value_json=json_set(value_json,'$.event.call.arguments.code','tampered-sensitive-marker') WHERE run_id=?1 AND sequence=1").bind(spec.run_id.to_string()).execute(store.pool()).await.unwrap();
            }
            "conversation" => {
                let other = fixture(&store, Some(spec.project_id)).await;
                sqlx::query("UPDATE agent_runs_v4 SET conversation_id=?1 WHERE run_id=?2")
                    .bind(other.conversation_id.to_string())
                    .bind(spec.run_id.to_string())
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        set_job(&store, job.job_id, &value).await;
        let before = snapshot(&store).await;
        let error = store
            .remote_job_record_v4(spec.project_id, job.job_id)
            .await
            .unwrap_err();
        assert!(
            matches!(error, StoreError::InvalidInput(_)),
            "{field}: {error}"
        );
        assert_eq!(
            error.to_string(),
            "invalid input: remote job record integrity mismatch",
            "{field}"
        );
        assert_eq!(snapshot(&store).await, before);
    }
}

#[tokio::test]
async fn receipt_hash_uses_stored_bytes_and_rejects_present_corruption_or_identity_mismatch() {
    for field in [
        "whitespace",
        "request",
        "session",
        "success",
        "nonterminal",
        "oversize",
        "invalid_json",
    ] {
        let store = Store::open_in_memory().await.unwrap();
        let spec = fixture(&store, None).await;
        let (job, _) = reserve(&store, &spec, "original", true).await;
        let job = terminal(&store, &job).await;
        let original: String =
            sqlx::query_scalar("SELECT result_json FROM runtime_job_results_v4 WHERE job_id=?1")
                .bind(job.job_id.to_string())
                .fetch_one(store.pool())
                .await
                .unwrap();
        let mut receipt: Value = serde_json::from_str(&original).unwrap();
        let serialized = match field {
            "whitespace" => format!(" {original}"),
            "request" => {
                receipt["request_id"] = json!(Uuid::new_v4());
                receipt.to_string()
            }
            "session" => {
                receipt["session_id"] = json!(Uuid::new_v4());
                receipt.to_string()
            }
            "success" => {
                receipt["succeeded"] = json!(false);
                receipt.to_string()
            }
            "oversize" => {
                receipt["stdout"] = json!("x".repeat(1024 * 1024));
                receipt.to_string()
            }
            "invalid_json" => "sensitive-invalid-json-marker".into(),
            _ => original.clone(),
        };
        sqlx::query("UPDATE runtime_job_results_v4 SET result_json=?1 WHERE job_id=?2")
            .bind(&serialized)
            .bind(job.job_id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let mut value = serde_json::to_value(&job).unwrap();
        if field != "whitespace" {
            value["result_sha256"] = json!(hex::encode(Sha256::digest(serialized.as_bytes())));
        }
        if field == "nonterminal" {
            value["state"] = json!("running");
        }
        set_job(&store, job.job_id, &value).await;
        let before = snapshot(&store).await;
        assert_eq!(
            store
                .remote_job_record_v4(spec.project_id, job.job_id)
                .await
                .unwrap_err()
                .to_string(),
            "invalid input: remote job record integrity mismatch",
            "{field}"
        );
        assert_eq!(snapshot(&store).await, before);
        if field == "whitespace" {
            value["result_sha256"] = json!(hex::encode(Sha256::digest(serialized.as_bytes())));
            set_job(&store, job.job_id, &value).await;
            assert!(
                store
                    .remote_job_record_v4(spec.project_id, job.job_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .result
                    .is_some()
            );
        }
    }
}

#[tokio::test]
async fn retired_terminal_receipt_preserves_required_request_identity_and_digest() {
    for succeeded in [true, false] {
        let store = Store::open_in_memory().await.unwrap();
        let spec = fixture(&store, None).await;
        let (job, _) = reserve(&store, &spec, "original", true).await;
        let running = store
            .advance_runtime_job_v4(&job, RuntimeJobStateV4::Running, Some(job.job_id), None)
            .await
            .unwrap();
        let result = RuntimeResultV4 {
            request_id: job.job_id,
            session_id: job.job_id,
            process_identity: format!("ssh-detached:{}", job.job_id),
            stdout: String::new(),
            stderr: String::new(),
            stdout_capture: None,
            stderr_capture: None,
            succeeded,
            artifacts: vec![],
            software_versions: Default::default(),
        };
        let state = if succeeded {
            RuntimeJobStateV4::Succeeded
        } else {
            RuntimeJobStateV4::Failed
        };
        let terminal = store
            .advance_runtime_job_v4(&running, state, None, Some(&result))
            .await
            .unwrap();
        append(
            &store,
            spec.run_id,
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: job.call_id.clone(),
                    tool_id: "runtime.execute".into(),
                    succeeded,
                    model_content: "recorded".into(),
                    data: json!({}),
                    provenance: vec![],
                },
            },
        )
        .await;
        let before = snapshot(&store).await;
        let loaded = store
            .remote_job_record_v4(spec.project_id, job.job_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.job, terminal);
        assert!(loaded.result.is_none());
        assert_eq!(snapshot(&store).await, before);
        let original = serde_json::to_value(&terminal).unwrap();
        for (field, replacement) in [
            ("result_request_id", Value::Null),
            ("result_sha256", Value::Null),
            ("result_sha256", json!("a".repeat(63))),
            ("result_sha256", json!("g".repeat(64))),
        ] {
            let mut invalid = original.clone();
            invalid[field] = replacement;
            set_job(&store, job.job_id, &invalid).await;
            let before = snapshot(&store).await;
            assert_eq!(
                store
                    .remote_job_record_v4(spec.project_id, job.job_id)
                    .await
                    .unwrap_err()
                    .to_string(),
                "invalid input: remote job record integrity mismatch",
                "{state:?} {field}"
            );
            assert_eq!(snapshot(&store).await, before);
        }
        set_job(&store, job.job_id, &original).await;
        let before = snapshot(&store).await;
        assert!(
            store
                .remote_job_record_v4(spec.project_id, job.job_id)
                .await
                .unwrap()
                .unwrap()
                .result
                .is_none()
        );
        assert_eq!(snapshot(&store).await, before);
    }
}

#[tokio::test]
async fn selected_record_checks_detached_session_identity_even_without_a_receipt() {
    let store = Store::open_in_memory().await.unwrap();
    let spec = fixture(&store, None).await;
    let (job, _) = reserve(&store, &spec, "original", true).await;
    let running = store
        .advance_runtime_job_v4(&job, RuntimeJobStateV4::Running, Some(job.job_id), None)
        .await
        .unwrap();
    let original = serde_json::to_value(&running).unwrap();
    for session in [json!(Uuid::new_v4()), Value::Null] {
        let mut invalid = original.clone();
        invalid["session_id"] = session;
        set_job(&store, job.job_id, &invalid).await;
        assert_eq!(
            store
                .remote_job_record_v4(spec.project_id, job.job_id)
                .await
                .unwrap_err()
                .to_string(),
            "invalid input: remote job record integrity mismatch"
        );
    }
    // CAS compares the stored serialization, so restore the exact typed bytes
    // rather than the key-sorted Value encoding used for corruption fixtures.
    sqlx::query("UPDATE runtime_jobs_v4 SET value_json=?1 WHERE job_id=?2")
        .bind(serde_json::to_string(&running).unwrap())
        .bind(job.job_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let terminal = store
        .advance_runtime_job_v4(&running, RuntimeJobStateV4::Unknown, None, None)
        .await
        .unwrap();
    assert_eq!(
        store
            .remote_job_record_v4(spec.project_id, job.job_id)
            .await
            .unwrap()
            .unwrap()
            .job,
        terminal
    );
}

use std::collections::BTreeMap;

use chrono::Utc;
use omicsops_adapters::persistence::Repository;
use omicsops_core::{
    audit::{RunEventKindV2, RunEventV2},
    domain::{ResourceLimits, StepRisk},
    plan_v2::{ApprovedPlan, PolicyEnvelope},
};
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn opening_a_legacy_database_creates_a_backup_and_migrates_transactionally() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("omicsops.db");
    {
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE app_objects (kind TEXT NOT NULL, id TEXT NOT NULL, value_json TEXT NOT NULL, PRIMARY KEY(kind, id));
                 INSERT INTO app_objects VALUES ('fixture', 'one', '{\"preserved\":true}');
                 PRAGMA user_version = 0;",
            )
            .unwrap();
    }

    let repository = Repository::open(&database).unwrap();

    assert_eq!(repository.schema_version().unwrap(), 3);
    assert_eq!(
        repository
            .get_json::<serde_json::Value>("fixture", "one")
            .unwrap()
            .unwrap()["preserved"],
        true
    );
    assert!(directory.path().join("omicsops.db.v1.bak").exists());
}

#[test]
fn approved_plan_records_round_trip_by_id() {
    let repository = Repository::open_in_memory().unwrap();
    let approved = ApprovedPlan {
        id: Uuid::new_v4(),
        plan_id: Uuid::new_v4(),
        plan_hash: "abc123".into(),
        policy: PolicyEnvelope {
            allowed_tools: vec!["bio.fastqc".into()],
            allowed_domains: vec![],
            max_risk: StepRisk::Low,
            allow_legacy_shell: false,
        },
        approved_at: Utc::now(),
    };

    repository.save_approved_plan(&approved).unwrap();

    assert_eq!(
        repository.get_approved_plan(approved.id).unwrap(),
        Some(approved)
    );
}

#[test]
fn audit_events_are_hash_chained_and_cannot_be_overwritten() {
    let repository = Repository::open_in_memory().unwrap();
    let run_id = Uuid::new_v4();
    let first = RunEventV2::new(
        1,
        run_id,
        RunEventKindV2::RunStarted,
        "started",
        None,
        BTreeMap::new(),
    )
    .unwrap();
    let second = RunEventV2::new(
        2,
        run_id,
        RunEventKindV2::StepSucceeded,
        "verified",
        Some(first.event_hash.clone()),
        BTreeMap::from([("verification".into(), "sha256".into())]),
    )
    .unwrap();

    repository.append_audit_event(&first).unwrap();
    repository.append_audit_event(&second).unwrap();

    assert!(repository.append_audit_event(&first).is_err());
    let events = repository.audit_events_for_run(run_id).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[1].prev_hash.as_deref(),
        Some(events[0].event_hash.as_str())
    );
    assert!(events.iter().all(RunEventV2::verify_hash));
}

#[test]
fn default_resource_limits_remain_serializable_in_migrated_storage() {
    let repository = Repository::open_in_memory().unwrap();
    repository
        .put_json("fixture", "limits", &ResourceLimits::default())
        .unwrap();

    assert_eq!(
        repository
            .get_json::<ResourceLimits>("fixture", "limits")
            .unwrap(),
        Some(ResourceLimits::default())
    );
}

#[test]
fn migration_preserves_v1_shell_commands_as_unapproved_v2_legacy_actions() {
    use omicsops_core::{
        domain::{AnalysisPlan, StageSpec, StepSpec},
        plan_v2::{AnalysisPlanV2, StepAction},
    };
    let directory = tempdir().unwrap();
    let database = directory.path().join("legacy.db");
    let plan = AnalysisPlan {
        id: Uuid::new_v4(),
        title: "legacy".into(),
        summary: "legacy".into(),
        stages: vec![StageSpec {
            id: "s".into(),
            goal: "s".into(),
            dependencies: vec![],
            steps: vec![StepSpec {
                id: "x".into(),
                title: "x".into(),
                rationale: "x".into(),
                command: "echo exact".into(),
                working_directory: "work".into(),
                timeout_seconds: 10,
                risk: StepRisk::Low,
                completion_conditions: vec![],
                expected_artifacts: vec![],
            }],
            expected_artifacts: vec![],
            completion_conditions: vec![],
        }],
        resource_budget: ResourceLimits::default(),
        highest_risk: StepRisk::Low,
        approved: true,
    };
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TABLE app_objects (kind TEXT NOT NULL, id TEXT NOT NULL, value_json TEXT NOT NULL, PRIMARY KEY(kind,id));").unwrap();
    connection
        .execute(
            "INSERT INTO app_objects VALUES ('analysis_plan', ?1, ?2)",
            rusqlite::params![plan.id.to_string(), serde_json::to_string(&plan).unwrap()],
        )
        .unwrap();
    drop(connection);

    let repository = Repository::open(database).unwrap();
    let migrated: AnalysisPlanV2 = repository
        .get_json("analysis_plan_v2", &plan.id.to_string())
        .unwrap()
        .unwrap();
    assert!(
        matches!(&migrated.stages[0].steps[0].action, StepAction::LegacyShell { command } if command == "echo exact")
    );
    assert_eq!(migrated.metadata["approval_status"], "reapproval_required");
}

#[test]
fn step_attempts_and_environment_locks_use_versioned_tables() {
    use omicsops_core::plan_v2::{EnvironmentLock, StepAttempt};
    let repository = Repository::open_in_memory().unwrap();
    let run_id = Uuid::new_v4();
    let attempt = StepAttempt {
        run_id,
        step_id: "qc".into(),
        attempt: 0,
        action_hash: "hash".into(),
        process_group_id: Some(42),
        started_at: Utc::now(),
        finished_at: Some(Utc::now()),
        exit_code: Some(0),
        log_path: "logs/qc.0.log".into(),
        manifest_path: ".omicsops/state/qc.0.manifest.json".into(),
        verifications: vec![],
    };
    let lock = EnvironmentLock {
        run_id,
        backend: "micromamba".into(),
        remote_path: ".omicsops/environment.lock".into(),
        sha256: "abc".into(),
        declared_dependencies: vec!["salmon".into()],
    };
    repository.save_step_attempt(&attempt).unwrap();
    repository.save_environment_lock(&lock).unwrap();
    assert_eq!(
        repository.step_attempts_for_run(run_id).unwrap(),
        vec![attempt]
    );
    assert_eq!(
        repository.environment_lock_for_run(run_id).unwrap(),
        Some(lock)
    );
}

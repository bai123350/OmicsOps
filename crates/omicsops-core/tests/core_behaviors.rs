use std::path::Path;

use omicsops_core::{
    domain::{
        AnalysisPlan, ResourceLimits, RunCheckpoint, RunState, StageSpec, StepRisk, StepSpec,
    },
    policy::{CommandPolicy, PolicyDecision},
    project::{RemoteProjectLayout, require_remote_descendant, validate_relative_remote_path},
    redaction::redact_secrets,
    state::RunStateMachine,
};
use uuid::Uuid;

#[test]
fn state_machine_enforces_approval_before_running() {
    let mut machine = RunStateMachine::new();
    machine.transition(RunState::Inspecting).unwrap();
    machine.transition(RunState::AwaitingPlanApproval).unwrap();

    assert!(machine.transition(RunState::Running).is_err());
    machine.transition(RunState::Preparing).unwrap();
    machine.transition(RunState::Running).unwrap();
    assert_eq!(machine.current(), RunState::Running);
}

#[test]
fn project_layout_has_resumable_control_directories() {
    let layout = RemoteProjectLayout::new("/srv/omicsops/pbmc");

    assert_eq!(layout.control_dir(), "/srv/omicsops/pbmc/.omicsops");
    assert_eq!(layout.state_dir(), "/srv/omicsops/pbmc/.omicsops/state");
    assert_eq!(layout.results_dir(), "/srv/omicsops/pbmc/results");
    assert_eq!(layout.required_directories().len(), 6);
}

#[test]
fn relative_remote_paths_cannot_escape_the_project() {
    assert!(validate_relative_remote_path(Path::new("results/report.html")).is_ok());
    assert!(validate_relative_remote_path(Path::new("../outside")).is_err());
    assert!(validate_relative_remote_path(Path::new("/etc/passwd")).is_err());
}

#[test]
fn artifact_paths_must_be_real_descendants_not_prefix_or_parent_traversals() {
    assert!(
        require_remote_descendant(
            "/srv/omicsops/pbmc/results",
            "/srv/omicsops/pbmc/results/report.html"
        )
        .is_ok()
    );
    assert!(
        require_remote_descendant(
            "/srv/omicsops/pbmc/results",
            "/srv/omicsops/pbmc/results-old/secret.txt"
        )
        .is_err()
    );
    assert!(
        require_remote_descendant(
            "/srv/omicsops/pbmc/results",
            "/srv/omicsops/pbmc/results/../state/run.json"
        )
        .is_err()
    );
    assert!(require_remote_descendant("/srv/omicsops/pbmc/results", "/etc/passwd").is_err());
}

#[test]
fn command_policy_rejects_privilege_and_external_writes() {
    let policy = CommandPolicy::new("/srv/omicsops/pbmc", &["ftp.ncbi.nlm.nih.gov"]);

    assert!(matches!(
        policy.evaluate("sudo apt-get update", StepRisk::Low),
        PolicyDecision::Denied { .. }
    ));
    assert!(matches!(
        policy.evaluate("rm -rf /srv/shared", StepRisk::High),
        PolicyDecision::Denied { .. }
    ));
}

#[test]
fn command_policy_requires_approval_for_successful_artifact_overwrite() {
    let policy = CommandPolicy::new("/srv/omicsops/pbmc", &[]);
    let decision = policy.evaluate(
        "python scripts/export.py --overwrite results/pbmc.h5ad",
        StepRisk::High,
    );

    assert!(matches!(decision, PolicyDecision::RequiresApproval { .. }));
}

#[test]
fn audit_text_redacts_registered_secrets() {
    let output = redact_secrets(
        "Authorization: Bearer sk-live-123 password=hunter2",
        &["sk-live-123", "hunter2"],
    );

    assert!(!output.contains("sk-live-123"));
    assert!(!output.contains("hunter2"));
    assert_eq!(
        output,
        "Authorization: Bearer [REDACTED] password=[REDACTED]"
    );
}

#[test]
fn run_checkpoint_skips_empty_stages_and_advances_without_rerunning_completed_steps() {
    let plan = AnalysisPlan {
        id: Uuid::new_v4(),
        title: "PBMC".into(),
        summary: "test plan".into(),
        stages: vec![
            stage("prepare", vec![]),
            stage("analysis", vec![step("qc"), step("cluster")]),
            stage("empty-report", vec![]),
            stage("report", vec![step("render")]),
        ],
        resource_budget: ResourceLimits::default(),
        highest_risk: StepRisk::Low,
        approved: true,
    };
    let mut checkpoint =
        RunCheckpoint::new(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), plan.id);

    assert_eq!(
        checkpoint
            .current_step(&plan)
            .map(|(stage, step)| (stage.id.as_str(), step.id.as_str())),
        Some(("analysis", "qc"))
    );

    assert!(!checkpoint.advance_after_success(&plan));
    assert_eq!(
        checkpoint
            .current_step(&plan)
            .map(|(stage, step)| (stage.id.as_str(), step.id.as_str())),
        Some(("analysis", "cluster"))
    );

    assert!(!checkpoint.advance_after_success(&plan));
    assert_eq!(
        checkpoint
            .current_step(&plan)
            .map(|(stage, step)| (stage.id.as_str(), step.id.as_str())),
        Some(("report", "render"))
    );

    assert!(checkpoint.advance_after_success(&plan));
    assert!(checkpoint.current_step(&plan).is_none());
}

#[test]
fn run_checkpoint_resumes_only_the_exact_pending_approval() {
    let mut checkpoint = RunCheckpoint::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    checkpoint.pause_for_approval(
        "successful artifact would be overwritten",
        "python export.py --overwrite results/pbmc.h5ad",
    );
    let request_id = checkpoint.pending_approval.as_ref().unwrap().id;

    assert_eq!(checkpoint.state, RunState::PausedForApproval);
    assert!(checkpoint.approve(Uuid::new_v4()).is_err());
    assert!(checkpoint.pending_approval.is_some());

    let approved_command = checkpoint.approve(request_id).unwrap();

    assert_eq!(
        approved_command,
        "python export.py --overwrite results/pbmc.h5ad"
    );
    assert_eq!(checkpoint.state, RunState::Preparing);
    assert!(checkpoint.pending_approval.is_none());
}

fn stage(id: &str, steps: Vec<StepSpec>) -> StageSpec {
    StageSpec {
        id: id.into(),
        goal: id.into(),
        dependencies: Vec::new(),
        steps,
        expected_artifacts: Vec::new(),
        completion_conditions: Vec::new(),
    }
}

fn step(id: &str) -> StepSpec {
    StepSpec {
        id: id.into(),
        title: id.into(),
        rationale: id.into(),
        command: "true".into(),
        working_directory: "work".into(),
        timeout_seconds: 60,
        risk: StepRisk::Low,
        completion_conditions: Vec::new(),
        expected_artifacts: Vec::new(),
    }
}

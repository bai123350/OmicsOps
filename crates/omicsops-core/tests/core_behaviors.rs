use std::path::Path;

use omicsops_core::{
    domain::{RunState, StepRisk},
    policy::{CommandPolicy, PolicyDecision},
    project::{RemoteProjectLayout, validate_relative_remote_path},
    redaction::redact_secrets,
    state::RunStateMachine,
};

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

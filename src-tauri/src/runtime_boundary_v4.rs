use omicsops_dto::{
    DetachedJobLifecycleV4, InteractiveLifecycleV4, RuntimeBoundaryLimitV4,
    RuntimeBoundarySourceV4, RuntimeBoundaryVerificationV4, RuntimeBoundaryViewV4,
    RuntimeExecutionLocationV4,
};
use omicsops_protocol::{ComputeBackendKindV4, ComputeSelectionV4, IsolationStrengthV4};
use uuid::Uuid;

pub(crate) fn describe_runtime_boundary(
    project_id: Uuid,
    source: RuntimeBoundarySourceV4,
    selection: &ComputeSelectionV4,
) -> Result<RuntimeBoundaryViewV4, String> {
    selection
        .validate()
        .map_err(|_| "runtime_boundary_invalid_selection".to_string())?;
    use RuntimeBoundaryLimitV4::*;
    let (execution_location, isolation, limits, detached_job_lifecycle) =
        match selection.backend_kind {
            ComputeBackendKindV4::Local => (
                RuntimeExecutionLocationV4::LocalHost,
                IsolationStrengthV4::Process,
                vec![SameUserPermissions, ProjectCwdNotAccessControl],
                DetachedJobLifecycleV4::Unsupported,
            ),
            ComputeBackendKindV4::Ssh => (
                RuntimeExecutionLocationV4::SshHost,
                IsolationStrengthV4::Process,
                vec![SameUserPermissions, ProjectCwdNotAccessControl],
                DetachedJobLifecycleV4::SshLinuxOnly,
            ),
            ComputeBackendKindV4::Docker | ComputeBackendKindV4::Podman => (
                RuntimeExecutionLocationV4::LocalContainer,
                IsolationStrengthV4::Container,
                vec![
                    ProjectMountReadWrite,
                    ReadOnlyRootfs,
                    CapabilitiesDropped,
                    NoNewPrivileges,
                    PidsLimit256,
                    SharedKernelNotVm,
                ],
                DetachedJobLifecycleV4::Unsupported,
            ),
        };
    Ok(RuntimeBoundaryViewV4 {
        project_id,
        source,
        compute_selection: selection.clone(),
        execution_location,
        isolation,
        limits,
        interactive_lifecycle: InteractiveLifecycleV4::RunScopedNoRestartReconnect,
        detached_job_lifecycle,
        verification: RuntimeBoundaryVerificationV4::NotCheckedByThisView,
    })
}

/// A bounded Host statement for model context. IDs and image identities stay
/// in the frozen compute selection, where they can be represented in full.
pub(crate) fn format_runtime_boundary(view: &RuntimeBoundaryViewV4) -> String {
    use omicsops_protocol::{ApprovalPolicyV4, AutonomyModeV4, NetworkPolicyV4};

    let location = match view.execution_location {
        RuntimeExecutionLocationV4::LocalHost => "local_host",
        RuntimeExecutionLocationV4::SshHost => "ssh_host",
        RuntimeExecutionLocationV4::LocalContainer => "local_container",
    };
    let isolation = match view.isolation {
        IsolationStrengthV4::Process => "process",
        IsolationStrengthV4::Container => "container",
    };
    let backend = match view.compute_selection.backend_kind {
        ComputeBackendKindV4::Local => "local",
        ComputeBackendKindV4::Ssh => "ssh",
        ComputeBackendKindV4::Docker => "docker",
        ComputeBackendKindV4::Podman => "podman",
    };
    let autonomy = match view.compute_selection.autonomy_mode {
        AutonomyModeV4::Supervised => "supervised",
        AutonomyModeV4::FullAuto => "full_auto",
    };
    let approval = match view.compute_selection.approval_policy {
        ApprovalPolicyV4::RequestApproval => "request_approval",
        ApprovalPolicyV4::RiskBased => "risk_based",
        ApprovalPolicyV4::AutoApproveExceptLocalDeletion => "auto_approve_except_local_deletion",
        ApprovalPolicyV4::FullAccess => "full_access",
    };
    let network = match view.compute_selection.network_policy {
        NetworkPolicyV4::HostInherited => "host_inherited",
        NetworkPolicyV4::None => "none",
    };
    let environment = if view.compute_selection.validate().is_ok() {
        view.compute_selection.environment.as_str()
    } else {
        "invalid_selection"
    };
    let execution_limits = match view.execution_location {
        RuntimeExecutionLocationV4::LocalHost => {
            "Current-user host process; system PATH interpreters; project cwd is not access control; host network inherited."
        }
        RuntimeExecutionLocationV4::SshHost => {
            "Remote SSH-user process; system or frozen project Micromamba environment; project cwd is not access control; remote network inherited."
        }
        RuntimeExecutionLocationV4::LocalContainer => {
            "Compute container has read-only rootfs, dropped capabilities, no-new-privileges, PID limit 256, and project mount read-write; shared kernel, not a VM. network none applies only to the compute container; model/MCP may use network. No CPU or memory quota is asserted."
        }
    };
    let detached = match view.detached_job_lifecycle {
        DetachedJobLifecycleV4::Unsupported => "Detached jobs unsupported on this backend.",
        DetachedJobLifecycleV4::SshLinuxOnly => {
            "Detached jobs: Linux only; not checked here. Query by original identity; no automatic polling or cancellation; uncertain dispatch must not be retried. Stop does not confirm remote termination."
        }
    };
    format!(
        "HOST RUNTIME BOUNDARY\nbackend={backend}; location={location}; isolation={isolation}; environment={environment}; autonomy={autonomy}; approval={approval}; network={network}.\n{execution_limits}\nInteractive kernel is run-scoped and cannot reconnect after app restart; stopping local wait does not establish remote termination.\n{detached}\nThis view is configuration only: interpreters, scientific dependencies not verified; remote liveness and computation success not checked. Full backend/image identity remains in frozen compute_selection."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_dto::{
        DetachedJobLifecycleV4, RuntimeBoundaryLimitV4, RuntimeExecutionLocationV4,
    };
    use serde_json::json;

    fn selection(kind: &str, backend_id: &str, environment: &str) -> ComputeSelectionV4 {
        let container = kind == "docker" || kind == "podman";
        serde_json::from_value(json!({
            "schema_version": 4,
            "backend_id": backend_id,
            "backend_kind": kind,
            "autonomy_mode": "supervised",
            "environment": environment,
            "network_policy": if container { "none" } else { "host_inherited" },
            "container_image": if container { json!({"reference":"python:3.12","image_id":"sha256:fixed"}) } else { serde_json::Value::Null }
        })).unwrap()
    }

    #[test]
    fn runtime_boundary_projects_each_backend_without_probe() {
        use RuntimeBoundaryLimitV4::*;
        let project_id = Uuid::new_v4();
        for (kind, id, environment, location, detached, limits) in [
            (
                "local",
                "local",
                "system",
                RuntimeExecutionLocationV4::LocalHost,
                DetachedJobLifecycleV4::Unsupported,
                vec![SameUserPermissions, ProjectCwdNotAccessControl],
            ),
            (
                "ssh",
                "ssh:offline",
                "rnaseq",
                RuntimeExecutionLocationV4::SshHost,
                DetachedJobLifecycleV4::SshLinuxOnly,
                vec![SameUserPermissions, ProjectCwdNotAccessControl],
            ),
            (
                "docker",
                "docker",
                "system",
                RuntimeExecutionLocationV4::LocalContainer,
                DetachedJobLifecycleV4::Unsupported,
                vec![
                    ProjectMountReadWrite,
                    ReadOnlyRootfs,
                    CapabilitiesDropped,
                    NoNewPrivileges,
                    PidsLimit256,
                    SharedKernelNotVm,
                ],
            ),
            (
                "podman",
                "podman",
                "system",
                RuntimeExecutionLocationV4::LocalContainer,
                DetachedJobLifecycleV4::Unsupported,
                vec![
                    ProjectMountReadWrite,
                    ReadOnlyRootfs,
                    CapabilitiesDropped,
                    NoNewPrivileges,
                    PidsLimit256,
                    SharedKernelNotVm,
                ],
            ),
        ] {
            let selected = selection(kind, id, environment);
            let view = describe_runtime_boundary(
                project_id,
                RuntimeBoundarySourceV4::DraftSelection,
                &selected,
            )
            .unwrap();
            assert_eq!(view.execution_location, location, "{kind}");
            assert_eq!(view.detached_job_lifecycle, detached, "{kind}");
            assert_eq!(view.limits, limits, "{kind}");
            assert_eq!(view.compute_selection, selected, "{kind}");
        }
    }

    #[test]
    fn runtime_boundary_rejects_invalid_process_policy_and_unpinned_image() {
        let mut local = selection("local", "local", "system");
        local.approval_policy = omicsops_protocol::ApprovalPolicyV4::FullAccess;
        assert!(
            describe_runtime_boundary(
                Uuid::new_v4(),
                RuntimeBoundarySourceV4::DraftSelection,
                &local
            )
            .is_err()
        );
        let mut docker = selection("docker", "docker", "system");
        docker.container_image.as_mut().unwrap().image_id.clear();
        assert!(
            describe_runtime_boundary(
                Uuid::new_v4(),
                RuntimeBoundarySourceV4::DraftSelection,
                &docker
            )
            .is_err()
        );
    }

    #[test]
    fn runtime_boundary_summary_is_bounded_and_states_backend_limits() {
        for (kind, backend, environment, expected) in [
            ("local", "local", "system", "local_host"),
            ("ssh", "ssh:offline", "rnaseq", "ssh_host"),
            ("docker", "docker", "system", "local_container"),
            ("podman", "podman", "system", "local_container"),
        ] {
            let view = describe_runtime_boundary(
                Uuid::new_v4(),
                RuntimeBoundarySourceV4::FrozenRun {
                    run_id: Uuid::new_v4(),
                },
                &selection(kind, backend, environment),
            )
            .unwrap();
            let summary = format_runtime_boundary(&view);
            assert!(summary.starts_with("HOST RUNTIME BOUNDARY\n"));
            assert!(summary.contains(expected), "{kind}: {summary}");
            assert!(summary.contains("environment="), "{kind}");
            assert!(summary.contains("dependencies not verified"), "{kind}");
            assert!(!summary.contains("ready"), "{kind}");
            if kind == "ssh" {
                assert!(!summary.contains(&view.compute_selection.backend_id));
            }
            assert!(!summary.contains("sha256:fixed"));
            assert!(summary.len() <= 4096, "{kind}: {}", summary.len());
            if kind == "ssh" {
                assert!(summary.contains("Stop does not confirm remote termination"));
                assert!(summary.contains("Linux only; not checked"));
            }
            if kind == "docker" || kind == "podman" {
                assert!(summary.contains("network none applies only to the compute container"));
                assert!(summary.contains("model/MCP may use network"));
                assert!(summary.contains("project mount read-write"));
            }
        }
    }
}

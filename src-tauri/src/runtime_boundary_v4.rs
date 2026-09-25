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
}

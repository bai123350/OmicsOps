use omicsops_protocol::{ComputeSelectionV4, IsolationStrengthV4};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeBoundaryRequestV4 {
    DraftSelection {
        project_id: Uuid,
        compute_selection: ComputeSelectionV4,
    },
    FrozenRun {
        project_id: Uuid,
        run_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeBoundarySourceV4 {
    DraftSelection,
    FrozenRun { run_id: Uuid },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionLocationV4 {
    LocalHost,
    SshHost,
    LocalContainer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveLifecycleV4 {
    RunScopedNoRestartReconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetachedJobLifecycleV4 {
    Unsupported,
    SshLinuxOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBoundaryVerificationV4 {
    NotCheckedByThisView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBoundaryLimitV4 {
    SameUserPermissions,
    ProjectCwdNotAccessControl,
    ProjectMountReadWrite,
    ReadOnlyRootfs,
    CapabilitiesDropped,
    NoNewPrivileges,
    PidsLimit256,
    SharedKernelNotVm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBoundaryViewV4 {
    pub project_id: Uuid,
    pub source: RuntimeBoundarySourceV4,
    pub compute_selection: ComputeSelectionV4,
    pub execution_location: RuntimeExecutionLocationV4,
    pub isolation: IsolationStrengthV4,
    pub limits: Vec<RuntimeBoundaryLimitV4>,
    pub interactive_lifecycle: InteractiveLifecycleV4,
    pub detached_job_lifecycle: DetachedJobLifecycleV4,
    pub verification: RuntimeBoundaryVerificationV4,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn runtime_boundary_request_cannot_override_frozen_run() {
        let value = json!({
            "source": "frozen_run", "project_id": Uuid::new_v4(),
            "run_id": Uuid::new_v4(), "compute_selection": null
        });
        assert!(serde_json::from_value::<RuntimeBoundaryRequestV4>(value).is_err());
    }

    #[test]
    fn runtime_boundary_view_serializes_source_limits_and_unchecked_status() {
        let project_id = Uuid::from_u128(1);
        let run_id = Uuid::from_u128(2);
        let selection = serde_json::from_value(json!({
            "schema_version":4,"backend_id":"local","backend_kind":"local",
            "autonomy_mode":"supervised","environment":"system",
            "network_policy":"host_inherited"
        }))
        .unwrap();
        let view = RuntimeBoundaryViewV4 {
            project_id,
            source: RuntimeBoundarySourceV4::FrozenRun { run_id },
            compute_selection: selection,
            execution_location: RuntimeExecutionLocationV4::LocalHost,
            isolation: omicsops_protocol::IsolationStrengthV4::Process,
            limits: vec![RuntimeBoundaryLimitV4::SameUserPermissions],
            interactive_lifecycle: InteractiveLifecycleV4::RunScopedNoRestartReconnect,
            detached_job_lifecycle: DetachedJobLifecycleV4::Unsupported,
            verification: RuntimeBoundaryVerificationV4::NotCheckedByThisView,
        };
        let value = serde_json::to_value(view).unwrap();
        assert_eq!(
            value["source"],
            json!({"kind":"frozen_run","run_id":run_id})
        );
        assert_eq!(value["limits"], json!(["same_user_permissions"]));
        assert_eq!(value["verification"], "not_checked_by_this_view");
    }
}

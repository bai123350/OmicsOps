use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

mod runtime_job;
pub use runtime_job::*;

pub const AGENT_RUNTIME_V4: &str = "omicsops.agent-runtime@4.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunModeV4 {
    Plan,
    Execute,
}

/// Stable identity used to bind a one-time Plan approval to the exact
/// generating revision that requested it. The revision UUID is intentionally
/// included in addition to the human-visible number so an old decision cannot
/// be replayed after a revision is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanApprovalScopeV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub revision_id: Uuid,
    pub revision: u64,
}

impl PlanApprovalScopeV4 {
    pub fn hash(self) -> String {
        let value = serde_json::json!({
            "mode": "plan",
            "project_id": self.project_id,
            "conversation_id": self.conversation_id,
            "run_id": self.run_id,
            "revision_id": self.revision_id,
            "revision": self.revision,
        });
        hex::encode(Sha256::digest(
            serde_json::to_vec(&value).expect("plan approval scope is serializable"),
        ))
    }
}

pub fn plan_approval_scope_hash(scope: PlanApprovalScopeV4) -> String {
    scope.hash()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusV4 {
    Planning,
    AwaitingApproval,
    Running,
    WaitingForInput,
    WaitingForApproval,
    Completed,
    Failed,
    Cancelled,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelErrorClassV4 {
    ContextOverflow,
    RateLimited,
    Server,
    Timeout,
    Transport,
    Authentication,
    InvalidRequest,
    InvalidResponse,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModelFailureV4 {
    pub class: ModelErrorClassV4,
    pub message: String,
    pub retryable: bool,
}

impl ModelFailureV4 {
    pub fn transient(class: ModelErrorClassV4, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
            retryable: true,
        }
    }

    pub fn permanent(class: ModelErrorClassV4, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
            retryable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionPlanV4 {
    pub schema_version: u8,
    pub objective: String,
    pub steps: Vec<String>,
    pub completion_criteria: Vec<String>,
    pub requested_capabilities: BTreeSet<String>,
}

impl ExecutionPlanV4 {
    pub fn validate(&self) -> Result<(), ProtocolErrorV4> {
        if self.schema_version != 4
            || self.objective.trim().is_empty()
            || self.steps.is_empty()
            || self.completion_criteria.is_empty()
        {
            return Err(ProtocolErrorV4::InvalidPlan);
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> Result<String, ProtocolErrorV4> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| ProtocolErrorV4::InvalidPlan)?;
        Ok(hex::encode(Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunExecutionKindV4 {
    #[default]
    ApprovedPlan,
    OrdinaryAgent,
}

fn is_approved_plan_execution(value: &RunExecutionKindV4) -> bool {
    *value == RunExecutionKindV4::ApprovedPlan
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DelegatedModelBindingV4 {
    pub profile_id: Uuid,
    pub configuration_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunSpecV4 {
    pub schema_version: u8,
    pub runtime_id: String,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_model: Option<DelegatedModelBindingV4>,
    pub plan: ExecutionPlanV4,
    pub approved_plan_hash: String,
    #[serde(default, skip_serializing_if = "is_approved_plan_execution")]
    pub execution_kind: RunExecutionKindV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_selection: Option<ComputeSelectionV4>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl RunSpecV4 {
    pub fn freeze(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        model_profile_id: Uuid,
        plan: ExecutionPlanV4,
        approved_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, ProtocolErrorV4> {
        let actual = plan.canonical_hash()?;
        if actual != approved_hash {
            return Err(ProtocolErrorV4::PlanHashMismatch);
        }
        Ok(Self {
            schema_version: 4,
            runtime_id: AGENT_RUNTIME_V4.into(),
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            plan,
            delegated_model: None,
            approved_plan_hash: actual,
            execution_kind: RunExecutionKindV4::ApprovedPlan,
            compute_selection: None,
            approval_hash: None,
            spec_hash: None,
            created_at: now,
        })
    }

    pub fn approval_hash_for(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        model_profile_id: Uuid,
        plan: &ExecutionPlanV4,
        selection: &ComputeSelectionV4,
    ) -> Result<String, ProtocolErrorV4> {
        selection.validate()?;
        let plan_hash = plan.canonical_hash()?;
        let value = serde_json::json!({
            "schema_version": 4,
            "runtime_id": AGENT_RUNTIME_V4,
            "run_id": run_id,
            "project_id": project_id,
            "conversation_id": conversation_id,
            "model_profile_id": model_profile_id,
            "plan_hash": plan_hash,
            "compute_selection": selection,
        });
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(&value).map_err(|_| ProtocolErrorV4::InvalidComputeSelection)?,
        )))
    }

    pub fn freeze_with_compute(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        model_profile_id: Uuid,
        plan: ExecutionPlanV4,
        selection: ComputeSelectionV4,
        approved_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, ProtocolErrorV4> {
        let expected = Self::approval_hash_for(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            &plan,
            &selection,
        )?;
        if expected != approved_hash {
            return Err(ProtocolErrorV4::ApprovalHashMismatch);
        }
        let plan_hash = plan.canonical_hash()?;
        let mut spec = Self {
            schema_version: 4,
            runtime_id: AGENT_RUNTIME_V4.into(),
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            plan,
            approved_plan_hash: plan_hash,
            delegated_model: None,
            execution_kind: RunExecutionKindV4::ApprovedPlan,
            compute_selection: Some(selection),
            approval_hash: Some(expected),
            spec_hash: None,
            created_at: now,
        };
        spec.spec_hash = Some(spec.calculate_spec_hash()?);
        Ok(spec)
    }

    pub fn freeze_ordinary_agent_with_compute(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        model_profile_id: Uuid,
        plan: ExecutionPlanV4,
        selection: ComputeSelectionV4,
        approved_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, ProtocolErrorV4> {
        let mut spec = Self::freeze_with_compute(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            plan,
            selection,
            approved_hash,
            now,
        )?;
        spec.execution_kind = RunExecutionKindV4::OrdinaryAgent;
        spec.spec_hash = Some(spec.calculate_spec_hash()?);
        Ok(spec)
    }

    pub fn calculate_spec_hash(&self) -> Result<String, ProtocolErrorV4> {
        let mut value = serde_json::json!({
            "schema_version": self.schema_version,
            "runtime_id": self.runtime_id,
            "run_id": self.run_id,
            "project_id": self.project_id,
            "conversation_id": self.conversation_id,
            "model_profile_id": self.model_profile_id,
            "plan": self.plan,
            "approved_plan_hash": self.approved_plan_hash,
            "compute_selection": self.compute_selection,
            "approval_hash": self.approval_hash,
            "created_at": self.created_at,
        });
        if self.execution_kind == RunExecutionKindV4::OrdinaryAgent {
            value["execution_kind"] = serde_json::json!(self.execution_kind);
        }
        if let Some(binding) = &self.delegated_model {
            value["delegated_model"] = serde_json::json!(binding);
        }
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(&value).map_err(|_| ProtocolErrorV4::InvalidComputeSelection)?,
        )))
    }

    pub fn validate_integrity(&self) -> Result<(), ProtocolErrorV4> {
        if let Some(binding) = &self.delegated_model {
            if self.execution_kind != RunExecutionKindV4::OrdinaryAgent
                || self.compute_selection.is_none()
                || binding.configuration_hash.len() != 64
                || !binding
                    .configuration_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(ProtocolErrorV4::SpecHashMismatch);
            }
        }
        if self.plan.canonical_hash()? != self.approved_plan_hash {
            return Err(ProtocolErrorV4::PlanHashMismatch);
        }
        if let Some(selection) = &self.compute_selection {
            selection.validate()?;
            let expected = Self::approval_hash_for(
                self.run_id,
                self.project_id,
                self.conversation_id,
                self.model_profile_id,
                &self.plan,
                selection,
            )?;
            if self.approval_hash.as_deref() != Some(expected.as_str()) {
                return Err(ProtocolErrorV4::ApprovalHashMismatch);
            }
            let spec_hash = self.calculate_spec_hash()?;
            if self.spec_hash.as_deref() != Some(spec_hash.as_str()) {
                return Err(ProtocolErrorV4::SpecHashMismatch);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffectV4 {
    ReadOnly,
    Mutating,
    Runtime,
    Network,
    Delegation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentRequestRouteV4 {
    ResearchRetrieval,
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskShapeV4 {
    Fast,
    MultiStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskShapeSourceV4 {
    Model,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhaseV4 {
    Routing,
    Discovery,
    Clarification,
    Organizing,
    Executing,
    Verifying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskStatusV4 {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentTaskV4 {
    pub id: String,
    pub title: String,
    pub status: AgentTaskStatusV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentTaskListUpdateV4 {
    pub schema_version: u8,
    pub expected_revision: u64,
    pub change_summary: String,
    pub tasks: Vec<AgentTaskV4>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentInputReasonV4 {
    Scope,
    #[default]
    Decision,
    MissingData,
    Blocker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionKindV4 {
    Shared,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowserApprovalScopeV4 {
    Once,
    Conversation,
    Project,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BrowserApprovalBindingV4 {
    pub capability: String,
    pub target_host: String,
    pub session: BrowserSessionKindV4,
    pub protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BrowserAuthorizationV4 {
    pub id: String,
    pub scope: BrowserApprovalScopeV4,
    pub binding: BrowserApprovalBindingV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BrowserTabSummaryV4 {
    pub session: BrowserSessionKindV4,
    pub tab_id: u64,
    pub run_id: Option<Uuid>,
    pub title: String,
    pub origin: String,
    pub created_by_run: bool,
}

impl BrowserAuthorizationV4 {
    pub fn validate(&self) -> Result<(), ProtocolErrorV4> {
        let context_valid = match self.scope {
            BrowserApprovalScopeV4::Once => {
                self.project_id.is_some() && self.conversation_id.is_some()
            }
            BrowserApprovalScopeV4::Conversation => {
                self.project_id.is_some() && self.conversation_id.is_some()
            }
            BrowserApprovalScopeV4::Project => {
                self.project_id.is_some() && self.conversation_id.is_none()
            }
            BrowserApprovalScopeV4::Global => {
                self.project_id.is_none() && self.conversation_id.is_none()
            }
        };
        if self.id.trim().is_empty()
            || self.binding.capability.trim().is_empty()
            || self.binding.target_host.trim().is_empty()
            || self.binding.protocol_version == 0
            || !context_valid
        {
            return Err(ProtocolErrorV4::InvalidToolApproval);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UncertainResolutionV4 {
    SideEffectObserved,
    SideEffectNotObserved,
    Compensated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolDescriptorV4 {
    pub id: String,
    pub description: String,
    pub input_schema: Value,
    pub effect: ToolEffectV4,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolCallV4 {
    pub call_id: String,
    pub tool_id: String,
    pub arguments: Value,
}

impl ToolCallV4 {
    pub fn canonical_hash(&self) -> Result<String, ProtocolErrorV4> {
        if self.call_id.trim().is_empty() || self.tool_id.trim().is_empty() {
            return Err(ProtocolErrorV4::InvalidToolApproval);
        }
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(self).map_err(|_| ProtocolErrorV4::InvalidToolApproval)?,
        )))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalDecisionV4 {
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolApprovalRequestV4 {
    pub approval_id: String,
    pub call: ToolCallV4,
    pub effect: ToolEffectV4,
    pub reason: String,
    pub call_hash: String,
    /// Execute approvals retain the historical `None` scope for wire
    /// compatibility. Plan approvals set this to a stable revision-bound
    /// hash so an execute decision cannot be replayed during planning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_hash: Option<String>,
    #[serde(
        default = "default_tool_approval_mode",
        skip_serializing_if = "is_execute_mode"
    )]
    pub mode: RunModeV4,
}

impl ToolApprovalRequestV4 {
    pub fn new(
        run_id: Uuid,
        spec_hash: &str,
        call: ToolCallV4,
        effect: ToolEffectV4,
        reason: impl Into<String>,
    ) -> Result<Self, ProtocolErrorV4> {
        Self::new_with_scope_and_mode(
            run_id,
            spec_hash,
            call,
            effect,
            reason,
            None,
            RunModeV4::Execute,
        )
    }

    pub fn new_with_scope(
        run_id: Uuid,
        scope_hash: &str,
        call: ToolCallV4,
        effect: ToolEffectV4,
        reason: impl Into<String>,
    ) -> Result<Self, ProtocolErrorV4> {
        Self::new_with_scope_and_mode(
            run_id,
            scope_hash,
            call,
            effect,
            reason,
            Some(scope_hash.to_owned()),
            RunModeV4::Plan,
        )
    }

    fn new_with_scope_and_mode(
        run_id: Uuid,
        binding_hash: &str,
        call: ToolCallV4,
        effect: ToolEffectV4,
        reason: impl Into<String>,
        scope_hash: Option<String>,
        mode: RunModeV4,
    ) -> Result<Self, ProtocolErrorV4> {
        let call_hash = call.canonical_hash()?;
        let mut binding = serde_json::json!({
            "run_id": run_id,
            "spec_hash": binding_hash,
            "call_hash": call_hash,
            "effect": effect,
        });
        if mode == RunModeV4::Plan {
            binding["mode"] = serde_json::json!("plan");
            binding["scope_hash"] = serde_json::json!(scope_hash.as_deref().unwrap_or_default());
        }
        let approval_id = hex::encode(Sha256::digest(
            serde_json::to_vec(&binding).map_err(|_| ProtocolErrorV4::InvalidToolApproval)?,
        ));
        Ok(Self {
            approval_id,
            call,
            effect,
            reason: reason.into(),
            call_hash,
            scope_hash,
            mode,
        })
    }

    pub fn validate(&self, run_id: Uuid, spec_hash: &str) -> Result<(), ProtocolErrorV4> {
        let expected = Self::new_with_scope_and_mode(
            run_id,
            spec_hash,
            self.call.clone(),
            self.effect,
            self.reason.clone(),
            self.scope_hash.clone(),
            self.mode,
        )?;
        if self.call_hash != expected.call_hash || self.approval_id != expected.approval_id {
            return Err(ProtocolErrorV4::InvalidToolApproval);
        }
        if self.scope_hash != expected.scope_hash || self.mode != expected.mode {
            return Err(ProtocolErrorV4::InvalidToolApproval);
        }
        Ok(())
    }

    pub fn validate_with_scope(
        &self,
        run_id: Uuid,
        scope_hash: &str,
        expected_mode: RunModeV4,
    ) -> Result<(), ProtocolErrorV4> {
        if self.scope_hash.as_deref() != Some(scope_hash) || self.mode != expected_mode {
            return Err(ProtocolErrorV4::InvalidToolApproval);
        }
        self.validate(run_id, scope_hash)
    }
}

fn default_tool_approval_mode() -> RunModeV4 {
    RunModeV4::Execute
}

fn is_execute_mode(mode: &RunModeV4) -> bool {
    *mode == RunModeV4::Execute
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolOutcomeV4 {
    pub call_id: String,
    pub tool_id: String,
    pub succeeded: bool,
    pub model_content: String,
    pub data: Value,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionContextKeyV4 {
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub backend_id: String,
    pub language: KernelLanguageV4,
    pub environment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KernelLanguageV4 {
    Python,
    R,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComputeBackendKindV4 {
    Ssh,
    Local,
    Docker,
    Podman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IsolationStrengthV4 {
    Process,
    Container,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyModeV4 {
    Supervised,
    FullAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicyV4 {
    RequestApproval,
    #[default]
    RiskBased,
    FullAccess,
}

impl ApprovalPolicyV4 {
    fn is_default(value: &Self) -> bool {
        *value == Self::RiskBased
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicyV4 {
    HostInherited,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainerImageSelectionV4 {
    pub reference: String,
    pub image_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ComputeSelectionV4 {
    pub schema_version: u8,
    pub backend_id: String,
    pub backend_kind: ComputeBackendKindV4,
    pub autonomy_mode: AutonomyModeV4,
    #[serde(default, skip_serializing_if = "ApprovalPolicyV4::is_default")]
    pub approval_policy: ApprovalPolicyV4,
    pub environment: String,
    pub network_policy: NetworkPolicyV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_image: Option<ContainerImageSelectionV4>,
}

impl ComputeSelectionV4 {
    pub fn validate(&self) -> Result<(), ProtocolErrorV4> {
        if self.schema_version != 4
            || self.backend_id.trim().is_empty()
            || self.environment.trim().is_empty()
            || self.environment.len() > 128
            || !self
                .environment
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        {
            return Err(ProtocolErrorV4::InvalidComputeSelection);
        }
        let container = matches!(
            self.backend_kind,
            ComputeBackendKindV4::Docker | ComputeBackendKindV4::Podman
        );
        let backend_matches = match self.backend_kind {
            ComputeBackendKindV4::Local => self.backend_id == "local",
            ComputeBackendKindV4::Docker => self.backend_id == "docker",
            ComputeBackendKindV4::Podman => self.backend_id == "podman",
            ComputeBackendKindV4::Ssh => {
                self.backend_id.starts_with("ssh:") && self.backend_id.len() > "ssh:".len()
            }
        };
        if !backend_matches
            || (container && self.environment != "system")
            || (self.backend_kind == ComputeBackendKindV4::Local && self.environment != "system")
            || (container && self.network_policy != NetworkPolicyV4::None)
            || (!container && self.network_policy != NetworkPolicyV4::HostInherited)
            || (self.autonomy_mode == AutonomyModeV4::FullAuto && !container)
            || (self.approval_policy == ApprovalPolicyV4::FullAccess
                && self.autonomy_mode != AutonomyModeV4::FullAuto)
            || (self.approval_policy != ApprovalPolicyV4::FullAccess
                && self.autonomy_mode != AutonomyModeV4::Supervised)
        {
            return Err(ProtocolErrorV4::InvalidComputeSelection);
        }
        match (&self.container_image, container) {
            (Some(image), true)
                if !image.reference.trim().is_empty()
                    && !image.image_id.trim().is_empty()
                    && !image.reference.chars().any(char::is_whitespace)
                    && !image.image_id.chars().any(char::is_whitespace) => {}
            (None, false) => {}
            _ => return Err(ProtocolErrorV4::InvalidComputeSelection),
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> Result<String, ProtocolErrorV4> {
        self.validate()?;
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(self).map_err(|_| ProtocolErrorV4::InvalidComputeSelection)?,
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ComputeBackendDescriptorV4 {
    pub schema_version: u8,
    pub backend_id: String,
    pub kind: ComputeBackendKindV4,
    pub isolation: IsolationStrengthV4,
    pub available: bool,
    pub supports_python: bool,
    pub supports_r: bool,
    pub supports_network_policy: bool,
}

impl ComputeBackendDescriptorV4 {
    pub fn permits(&self, mode: AutonomyModeV4) -> bool {
        self.available
            && (mode != AutonomyModeV4::FullAuto
                || self.isolation == IsolationStrengthV4::Container)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExternalExecutorKindV4 {
    AcpCodex,
    AcpClaudeCode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScientificBridgeV4 {
    pub schema_version: u8,
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub scientific_state_sha256: String,
    pub scientific_state: Value,
    pub allowed_artifact_paths: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalExecutorTaskV4 {
    pub schema_version: u8,
    pub executor: ExternalExecutorKindV4,
    pub objective: String,
    pub capabilities: BTreeSet<String>,
    pub output_schema: Value,
    pub bridge: ScientificBridgeV4,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalExecutorOutcomeV4 {
    pub schema_version: u8,
    pub succeeded: bool,
    pub output: Value,
    pub proposed_artifacts: Vec<String>,
    pub audit: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OutputCaptureV4 {
    pub excerpt: String,
    pub total_bytes: u64,
    pub sha256: String,
    pub archive_path: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeArtifactV4 {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeResultV4 {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub process_identity: String,
    pub stdout: String,
    pub stderr: String,
    pub stdout_capture: Option<OutputCaptureV4>,
    pub stderr_capture: Option<OutputCaptureV4>,
    pub succeeded: bool,
    pub artifacts: Vec<RuntimeArtifactV4>,
    #[serde(default)]
    pub software_versions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextCheckpointV4 {
    pub schema_version: u8,
    pub through_sequence: u64,
    pub completion_criteria: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub recent_steps: Vec<String>,
    pub scientific_state: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_shape: Option<AgentTaskShapeV4>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<AgentPhaseV4>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<AgentTaskV4>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextArchiveV4 {
    pub archive_id: Uuid,
    pub through_sequence: u64,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletionEvidenceRefV4 {
    Event { sequence: u64 },
    Artifact { artifact_id: Uuid },
    Evidence { evidence_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompletionCriterionEvidenceV4 {
    pub criterion: String,
    pub evidence: Vec<CompletionEvidenceRefV4>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompletionProposalV4 {
    pub schema_version: u8,
    pub summary: String,
    /// User-visible final response in Markdown. Kept serde-defaulted so older
    /// persisted proposals remain readable during the protocol migration.
    #[serde(default)]
    pub answer_markdown: String,
    pub criteria: Vec<CompletionCriterionEvidenceV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerificationSeverityV4 {
    Error,
    Warn,
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VerificationFindingV4 {
    pub severity: VerificationSeverityV4,
    pub code: String,
    pub message: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeterministicVerificationV4 {
    pub schema_version: u8,
    pub passed: bool,
    pub findings: Vec<VerificationFindingV4>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewerReportV4 {
    pub schema_version: u8,
    pub summary: String,
    pub findings: Vec<VerificationFindingV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DelegationIsolationV4 {
    ReadOnlyProject,
    EvidenceOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DelegationBudgetV4 {
    pub max_turns: u8,
    pub max_tool_calls: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelegatedTaskNodeV4 {
    pub id: String,
    pub objective: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub budget: DelegationBudgetV4,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
    pub output_schema: Value,
    pub isolation: DelegationIsolationV4,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelegationGraphV4 {
    pub schema_version: u8,
    pub nodes: Vec<DelegatedTaskNodeV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DelegationNodeStatusV4 {
    Succeeded,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelegationNodeOutcomeV4 {
    pub node_id: String,
    pub status: DelegationNodeStatusV4,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub tool_outcomes: Vec<ToolOutcomeV4>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelegationGraphOutcomeV4 {
    pub schema_version: u8,
    pub nodes: BTreeMap<String, DelegationNodeOutcomeV4>,
}

impl ReviewerReportV4 {
    pub fn validate(&self) -> Result<(), ProtocolErrorV4> {
        if self.schema_version != 4
            || self.summary.trim().is_empty()
            || self.findings.len() > 8
            || self.findings.iter().any(|finding| {
                finding.code.trim().is_empty()
                    || finding.message.trim().is_empty()
                    || finding.evidence.is_empty()
            })
        {
            return Err(ProtocolErrorV4::InvalidReview);
        }
        Ok(())
    }

    pub fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.severity == VerificationSeverityV4::Error)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEventKindV4 {
    RuntimeRecoveryAvailable {
        call_ids: Vec<String>,
    },
    GuidanceConsumed {
        message_id: Uuid,
        markdown: String,
    },
    RunCreated {
        mode: RunModeV4,
    },
    ModelText {
        text: String,
    },
    ModelRetrying {
        attempt: u8,
        class: ModelErrorClassV4,
        message: String,
    },
    ToolRequested {
        call: ToolCallV4,
    },
    ToolApprovalRequested {
        request: ToolApprovalRequestV4,
    },
    ToolApprovalDecided {
        approval_id: String,
        call_hash: String,
        decision: ToolApprovalDecisionV4,
    },
    RequestRouted {
        route: AgentRequestRouteV4,
    },
    TaskShapeSelected {
        task_shape: AgentTaskShapeV4,
        source: AgentTaskShapeSourceV4,
        reason: String,
    },
    PhaseChanged {
        phase: AgentPhaseV4,
    },
    CycleStarted {
        cycle_id: u64,
    },
    CycleFinished {
        cycle_id: u64,
    },
    TaskListUpdated {
        revision: u64,
        change_summary: String,
        tasks: Vec<AgentTaskV4>,
    },
    ToolBatchStarted {
        batch_id: u64,
        cycle_id: u64,
        phase: AgentPhaseV4,
        tool_names: Vec<String>,
        call_ids: Vec<String>,
    },
    ToolBatchFinished {
        batch_id: u64,
        cycle_id: u64,
        phase: AgentPhaseV4,
        tool_names: Vec<String>,
        call_ids: Vec<String>,
        duration_ms: u64,
        succeeded: u32,
        failed: u32,
    },
    BrowserConnectionRequired {
        session: BrowserSessionKindV4,
        protocol_version: u16,
        message: String,
    },
    BrowserHumanInterventionRequired {
        session: BrowserSessionKindV4,
        reason: String,
        message: String,
    },
    BrowserTabCleanupRequired {
        sessions: Vec<BrowserSessionKindV4>,
        tabs: Vec<BrowserTabSummaryV4>,
        message: String,
    },
    ToolDispatchStarted {
        call_id: String,
        tool_id: String,
        effect: ToolEffectV4,
        idempotency_key: String,
    },
    ToolFinished {
        outcome: ToolOutcomeV4,
    },
    ToolOutcomeReused {
        idempotency_key: String,
        outcome: ToolOutcomeV4,
    },
    ToolDispatchUncertain {
        call_id: String,
        tool_id: String,
    },
    ToolDispatchResolved {
        call_id: String,
        resolution: UncertainResolutionV4,
        evidence: String,
    },
    PlanProposed {
        plan: ExecutionPlanV4,
        plan_hash: String,
    },
    PlanApproved {
        plan_hash: String,
    },
    /// A user requested changes to the currently pending plan. This additive
    /// event keeps older event JSON readable while making feedback replayable.
    #[serde(alias = "plan_revision_request")]
    PlanRevisionRequested {
        plan_hash: String,
        feedback: String,
    },
    RunSpecFrozen {
        approval_hash: String,
        spec_hash: String,
    },
    ModeChanged {
        mode: RunModeV4,
    },
    InputRequested {
        question_id: String,
        question: String,
        #[serde(default)]
        reason: AgentInputReasonV4,
    },
    UserInputAnswered {
        question_id: String,
        answer: String,
    },
    ContextArchived {
        archive: ContextArchiveV4,
    },
    ContextCheckpointed {
        checkpoint: ContextCheckpointV4,
    },
    ScientificStateChanged {
        revision: u64,
        state_sha256: String,
        changes: Vec<String>,
    },
    CompletionProposed,
    CompletionProposalSubmitted {
        proposal: CompletionProposalV4,
    },
    DeterministicVerificationFinished {
        report: DeterministicVerificationV4,
    },
    ReviewerFinished {
        report: ReviewerReportV4,
    },
    ReviewerCorrectionRequested {
        correction: u8,
        findings: Vec<VerificationFindingV4>,
    },
    DelegationGraphStarted {
        call_id: String,
        graph: DelegationGraphV4,
    },
    DelegationNodeFinished {
        call_id: String,
        outcome: DelegationNodeOutcomeV4,
    },
    DelegationGraphFinished {
        call_id: String,
        outcome: DelegationGraphOutcomeV4,
    },
    RunCompleted,
    RunFailed {
        message: String,
    },
    RunNeedsAttention {
        message: String,
    },
    RunCancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AgentEventV4 {
    pub schema_version: u8,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub sequence: u64,
    pub occurred_at: DateTime<Utc>,
    pub previous_hash: String,
    pub event_hash: String,
    pub event: AgentEventKindV4,
}

impl AgentEventV4 {
    pub fn first(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        at: DateTime<Utc>,
        event: AgentEventKindV4,
    ) -> Self {
        Self::build(
            run_id,
            project_id,
            conversation_id,
            1,
            String::new(),
            at,
            event,
        )
    }
    pub fn next(previous: &Self, at: DateTime<Utc>, event: AgentEventKindV4) -> Self {
        Self::build(
            previous.run_id,
            previous.project_id,
            previous.conversation_id,
            previous.sequence + 1,
            previous.event_hash.clone(),
            at,
            event,
        )
    }
    fn build(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        sequence: u64,
        previous_hash: String,
        occurred_at: DateTime<Utc>,
        event: AgentEventKindV4,
    ) -> Self {
        let mut value = Self {
            schema_version: 4,
            run_id,
            project_id,
            conversation_id,
            sequence,
            occurred_at,
            previous_hash,
            event_hash: String::new(),
            event,
        };
        value.event_hash = value.calculate_hash();
        value
    }
    fn calculate_hash(&self) -> String {
        self.calculate_hash_for_event(
            &serde_json::to_value(&self.event).expect("serializable V4 event"),
        )
    }
    fn calculate_hash_for_event(&self, event: &serde_json::Value) -> String {
        let envelope = serde_json::json!({"schema_version":self.schema_version,"run_id":self.run_id,"project_id":self.project_id,"conversation_id":self.conversation_id,"sequence":self.sequence,"occurred_at":self.occurred_at,"previous_hash":self.previous_hash,"event":event});
        hex::encode(Sha256::digest(
            serde_json::to_vec(&envelope).expect("serializable V4 event"),
        ))
    }
    pub fn verify(&self) -> Result<(), ProtocolErrorV4> {
        let legacy_revision_request_hash =
            matches!(&self.event, AgentEventKindV4::PlanRevisionRequested { .. }).then(|| {
                let mut event = serde_json::to_value(&self.event).expect("serializable V4 event");
                event["kind"] = serde_json::Value::String("plan_revision_request".into());
                self.calculate_hash_for_event(&event)
            });
        if self.event_hash == self.calculate_hash()
            || legacy_revision_request_hash.as_deref() == Some(self.event_hash.as_str())
        {
            Ok(())
        } else {
            Err(ProtocolErrorV4::EventHashMismatch)
        }
    }
}

pub fn validate_event_chain_v4(events: &[AgentEventV4]) -> Result<(), ProtocolErrorV4> {
    for (index, event) in events.iter().enumerate() {
        event.verify()?;
        if event.sequence != index as u64 + 1
            || (index == 0 && !event.previous_hash.is_empty())
            || (index > 0 && event.previous_hash != events[index - 1].event_hash)
        {
            return Err(ProtocolErrorV4::BrokenEventChain);
        }
    }
    validate_terminal_position_v4(events)?;
    Ok(())
}

fn validate_terminal_position_v4(events: &[AgentEventV4]) -> Result<(), ProtocolErrorV4> {
    let mut terminal_seen = false;
    let mut cleanup_seen = false;
    for event in events {
        if is_terminal_event_kind_v4(&event.event) {
            if terminal_seen {
                return Err(ProtocolErrorV4::BrokenEventChain);
            }
            terminal_seen = true;
        } else if terminal_seen {
            if cleanup_seen
                || !matches!(
                    event.event,
                    AgentEventKindV4::BrowserTabCleanupRequired { .. }
                )
            {
                return Err(ProtocolErrorV4::BrokenEventChain);
            }
            cleanup_seen = true;
        }
    }
    Ok(())
}

fn is_terminal_event_kind_v4(event: &AgentEventKindV4) -> bool {
    matches!(
        event,
        AgentEventKindV4::RunCompleted
            | AgentEventKindV4::RunFailed { .. }
            | AgentEventKindV4::RunNeedsAttention { .. }
            | AgentEventKindV4::RunCancelled
    )
}

/// Validate durable events against their original JSON representation before
/// deserializing them into the current schema. This preserves the exact JSON
/// number spelling used by the writer and permits additive fields with serde
/// defaults without weakening the hash chain.
pub fn deserialize_event_chain_v4(
    serialized: &[String],
) -> Result<Vec<AgentEventV4>, ProtocolErrorV4> {
    let mut events: Vec<AgentEventV4> = Vec::with_capacity(serialized.len());
    for (index, serialized_event) in serialized.iter().enumerate() {
        let value: serde_json::Value = serde_json::from_str(serialized_event)
            .map_err(|_| ProtocolErrorV4::InvalidEventEncoding)?;
        let stored_hash = value
            .get("event_hash")
            .and_then(serde_json::Value::as_str)
            .ok_or(ProtocolErrorV4::InvalidEventEncoding)?;
        let field = |name: &str| {
            value
                .get(name)
                .cloned()
                .ok_or(ProtocolErrorV4::InvalidEventEncoding)
        };
        let envelope = serde_json::json!({
            "schema_version": field("schema_version")?,
            "run_id": field("run_id")?,
            "project_id": field("project_id")?,
            "conversation_id": field("conversation_id")?,
            "sequence": field("sequence")?,
            "occurred_at": field("occurred_at")?,
            "previous_hash": field("previous_hash")?,
            "event": field("event")?,
        });
        let calculated = hex::encode(Sha256::digest(
            serde_json::to_vec(&envelope).map_err(|_| ProtocolErrorV4::InvalidEventEncoding)?,
        ));
        if stored_hash != calculated {
            return Err(ProtocolErrorV4::EventHashMismatch);
        }
        let event: AgentEventV4 =
            serde_json::from_value(value).map_err(|_| ProtocolErrorV4::InvalidEventEncoding)?;
        if event.sequence != index as u64 + 1
            || (index == 0 && !event.previous_hash.is_empty())
            || (index > 0 && event.previous_hash != events[index - 1].event_hash)
        {
            return Err(ProtocolErrorV4::BrokenEventChain);
        }
        events.push(event);
    }
    validate_terminal_position_v4(&events)?;
    Ok(events)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolErrorV4 {
    #[error("invalid V4 execution plan")]
    InvalidPlan,
    #[error("approved plan hash does not match the plan")]
    PlanHashMismatch,
    #[error("invalid V4 compute selection")]
    InvalidComputeSelection,
    #[error("approved V4 run hash does not match the plan and compute selection")]
    ApprovalHashMismatch,
    #[error("V4 run specification hash mismatch")]
    SpecHashMismatch,
    #[error("V4 event hash mismatch")]
    EventHashMismatch,
    #[error("invalid V4 event encoding")]
    InvalidEventEncoding,
    #[error("broken V4 event chain")]
    BrokenEventChain,
    #[error("invalid V4 reviewer report")]
    InvalidReview,
    #[error("invalid or tampered V4 tool approval")]
    InvalidToolApproval,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_proposal_old_json_defaults_answer_markdown() {
        let proposal: CompletionProposalV4 = serde_json::from_value(serde_json::json!({
            "schema_version": 4,
            "summary": "legacy completion",
            "criteria": []
        }))
        .expect("legacy completion proposal remains readable");
        assert!(proposal.answer_markdown.is_empty());
    }

    #[test]
    fn legacy_input_and_checkpoint_json_receive_guided_loop_defaults() {
        let input: AgentEventKindV4 = serde_json::from_value(serde_json::json!({
            "kind": "input_requested",
            "question_id": "legacy-question",
            "question": "Continue?"
        }))
        .unwrap();
        assert!(matches!(
            input,
            AgentEventKindV4::InputRequested {
                reason: AgentInputReasonV4::Decision,
                ..
            }
        ));

        let checkpoint: ContextCheckpointV4 = serde_json::from_value(serde_json::json!({
            "schema_version": 4,
            "through_sequence": 3,
            "completion_criteria": [],
            "unresolved_errors": [],
            "recent_steps": [],
            "scientific_state": {}
        }))
        .unwrap();
        assert_eq!(checkpoint.task_shape, None);
        assert_eq!(checkpoint.phase, None);
        assert_eq!(checkpoint.task_revision, None);
        assert!(checkpoint.tasks.is_empty());
        assert_eq!(checkpoint.cycle_id, None);
    }

    #[test]
    fn guided_loop_events_round_trip_in_the_hash_chain() {
        let first = AgentEventV4::first(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        let shape = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::TaskShapeSelected {
                task_shape: AgentTaskShapeV4::MultiStep,
                source: AgentTaskShapeSourceV4::Model,
                reason: "requires several verified steps".into(),
            },
        );
        let tasks = AgentEventV4::next(
            &shape,
            Utc::now(),
            AgentEventKindV4::TaskListUpdated {
                revision: 1,
                change_summary: "initial work breakdown".into(),
                tasks: vec![
                    AgentTaskV4 {
                        id: "discover".into(),
                        title: "Discover context".into(),
                        status: AgentTaskStatusV4::Completed,
                        blocked_reason: None,
                    },
                    AgentTaskV4 {
                        id: "execute".into(),
                        title: "Execute request".into(),
                        status: AgentTaskStatusV4::InProgress,
                        blocked_reason: None,
                    },
                ],
            },
        );
        let encoded = [&first, &shape, &tasks]
            .into_iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>();
        let decoded = deserialize_event_chain_v4(&encoded).unwrap();
        assert_eq!(decoded, vec![first, shape, tasks]);
    }

    #[test]
    fn durable_event_verification_accepts_additive_schema_defaults() {
        let event = AgentEventV4::first(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::CompletionProposalSubmitted {
                proposal: CompletionProposalV4 {
                    schema_version: 4,
                    summary: "legacy completion".into(),
                    answer_markdown: String::new(),
                    criteria: vec![],
                },
            },
        );
        let mut value = serde_json::to_value(&event).unwrap();
        value["event"]["proposal"]
            .as_object_mut()
            .unwrap()
            .remove("answer_markdown");
        let envelope = serde_json::json!({
            "schema_version": value["schema_version"],
            "run_id": value["run_id"],
            "project_id": value["project_id"],
            "conversation_id": value["conversation_id"],
            "sequence": value["sequence"],
            "occurred_at": value["occurred_at"],
            "previous_hash": value["previous_hash"],
            "event": value["event"],
        });
        value["event_hash"] = serde_json::Value::String(hex::encode(Sha256::digest(
            serde_json::to_vec(&envelope).unwrap(),
        )));
        let serialized = serde_json::to_string(&value).unwrap();

        let decoded = deserialize_event_chain_v4(&[serialized]).unwrap();
        let AgentEventKindV4::CompletionProposalSubmitted { proposal } = &decoded[0].event else {
            panic!("expected completion proposal");
        };
        assert!(proposal.answer_markdown.is_empty());
    }

    #[test]
    fn plan_hash_and_event_chain_are_deterministic_and_tamper_evident() {
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "x".into(),
            steps: vec!["a".into()],
            completion_criteria: vec!["b".into()],
            requested_capabilities: BTreeSet::from(["runtime.execute".into()]),
        };
        assert_eq!(
            plan.canonical_hash().unwrap(),
            plan.canonical_hash().unwrap()
        );
        let first = AgentEventV4::first(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Plan,
            },
        );
        let second = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::CompletionProposed);
        assert!(validate_event_chain_v4(&[first.clone(), second.clone()]).is_ok());
        let mut tampered = second;
        tampered.previous_hash = "bad".into();
        assert!(validate_event_chain_v4(&[first, tampered]).is_err());
    }

    #[test]
    fn terminal_event_cannot_appear_before_the_end_of_a_valid_hash_chain() {
        let run_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let first = AgentEventV4::first(
            run_id,
            project_id,
            conversation_id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Plan,
            },
        );
        let terminal = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::RunCancelled);
        let after = AgentEventV4::next(
            &terminal,
            Utc::now(),
            AgentEventKindV4::ModelText {
                text: "post-terminal".into(),
            },
        );
        let serialized = [&first, &terminal, &after]
            .into_iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(
            deserialize_event_chain_v4(&serialized),
            Err(ProtocolErrorV4::BrokenEventChain)
        ));
        assert!(matches!(
            validate_event_chain_v4(&[first, terminal, after]),
            Err(ProtocolErrorV4::BrokenEventChain)
        ));
    }

    #[test]
    fn one_browser_cleanup_prompt_may_follow_a_terminal_event() {
        let first = AgentEventV4::first(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        let terminal = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::RunCancelled);
        let cleanup = AgentEventV4::next(
            &terminal,
            Utc::now(),
            AgentEventKindV4::BrowserTabCleanupRequired {
                sessions: vec![BrowserSessionKindV4::Shared],
                tabs: vec![],
                message: "confirm cleanup".into(),
            },
        );
        let serialized = [&first, &terminal, &cleanup]
            .into_iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>();
        assert!(deserialize_event_chain_v4(&serialized).is_ok());
        assert!(validate_event_chain_v4(&[first, terminal, cleanup]).is_ok());
    }

    fn local_selection() -> ComputeSelectionV4 {
        ComputeSelectionV4 {
            schema_version: 4,
            backend_id: "local".into(),
            backend_kind: ComputeBackendKindV4::Local,
            autonomy_mode: AutonomyModeV4::Supervised,
            approval_policy: ApprovalPolicyV4::RiskBased,
            environment: "system".into(),
            network_policy: NetworkPolicyV4::HostInherited,
            container_image: None,
        }
    }

    #[test]
    fn compute_selection_is_frozen_into_tamper_evident_run_spec() {
        let run_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let model_profile_id = Uuid::new_v4();
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "local analysis".into(),
            steps: vec!["analyze".into()],
            completion_criteria: vec!["verified".into()],
            requested_capabilities: BTreeSet::from(["runtime.execute".into()]),
        };
        let selection = local_selection();
        let approval = RunSpecV4::approval_hash_for(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            &plan,
            &selection,
        )
        .unwrap();
        let spec = RunSpecV4::freeze_with_compute(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            plan,
            selection,
            &approval,
            Utc::now(),
        )
        .unwrap();
        assert!(spec.validate_integrity().is_ok());

        let mut backend = spec.clone();
        backend.compute_selection.as_mut().unwrap().backend_id = "docker".into();
        assert!(backend.validate_integrity().is_err());
        let mut autonomy = spec.clone();
        autonomy.compute_selection.as_mut().unwrap().autonomy_mode = AutonomyModeV4::FullAuto;
        assert!(autonomy.validate_integrity().is_err());
        let mut environment = spec.clone();
        environment.compute_selection.as_mut().unwrap().environment = "other".into();
        assert!(environment.validate_integrity().is_err());
        let mut network = spec.clone();
        network.compute_selection.as_mut().unwrap().network_policy = NetworkPolicyV4::None;
        assert!(network.validate_integrity().is_err());
    }

    #[test]
    fn ordinary_agent_execution_kind_is_serialized_and_tamper_evident() {
        let run_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let model_profile_id = Uuid::new_v4();
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "ordinary agent request".into(),
            steps: vec!["route and execute adaptively".into()],
            completion_criteria: vec!["host requirements satisfied".into()],
            requested_capabilities: BTreeSet::new(),
        };
        let selection = local_selection();
        let approval = RunSpecV4::approval_hash_for(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            &plan,
            &selection,
        )
        .unwrap();
        let spec = RunSpecV4::freeze_ordinary_agent_with_compute(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            plan,
            selection,
            &approval,
            Utc::now(),
        )
        .unwrap();

        assert_eq!(spec.execution_kind, RunExecutionKindV4::OrdinaryAgent);
        assert_eq!(
            serde_json::to_value(&spec).unwrap()["execution_kind"],
            serde_json::json!("ordinary_agent")
        );
        assert!(spec.validate_integrity().is_ok());

        let mut tampered = spec;
        tampered.execution_kind = RunExecutionKindV4::ApprovedPlan;
        assert_eq!(
            tampered.validate_integrity(),
            Err(ProtocolErrorV4::SpecHashMismatch)
        );
    }

    #[test]
    fn approved_plan_execution_kind_keeps_legacy_serialization_shape() {
        let run_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let model_profile_id = Uuid::new_v4();
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "approved plan".into(),
            steps: vec!["execute frozen step".into()],
            completion_criteria: vec!["verified".into()],
            requested_capabilities: BTreeSet::new(),
        };
        let selection = local_selection();
        let approval = RunSpecV4::approval_hash_for(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            &plan,
            &selection,
        )
        .unwrap();
        let spec = RunSpecV4::freeze_with_compute(
            run_id,
            project_id,
            conversation_id,
            model_profile_id,
            plan,
            selection,
            &approval,
            Utc::now(),
        )
        .unwrap();

        assert_eq!(spec.execution_kind, RunExecutionKindV4::ApprovedPlan);
        assert!(
            serde_json::to_value(&spec)
                .unwrap()
                .get("execution_kind")
                .is_none()
        );
        assert!(spec.validate_integrity().is_ok());
        assert!(
            serde_json::to_value(&spec)
                .unwrap()
                .get("delegated_model")
                .is_none()
        );
        let legacy: RunSpecV4 =
            serde_json::from_value(serde_json::to_value(&spec).unwrap()).unwrap();
        assert_eq!(
            legacy.calculate_spec_hash().unwrap(),
            spec.calculate_spec_hash().unwrap()
        );
        let mut ordinary = spec.clone();
        ordinary.execution_kind = RunExecutionKindV4::OrdinaryAgent;
        ordinary.delegated_model = Some(DelegatedModelBindingV4 {
            profile_id: Uuid::new_v4(),
            configuration_hash: "a".repeat(64),
        });
        ordinary.spec_hash = Some(ordinary.calculate_spec_hash().unwrap());
        ordinary.validate_integrity().unwrap();
        let roundtrip: RunSpecV4 =
            serde_json::from_value(serde_json::to_value(&ordinary).unwrap()).unwrap();
        roundtrip.validate_integrity().unwrap();
        ordinary
            .delegated_model
            .as_mut()
            .unwrap()
            .configuration_hash = "b".repeat(64);
        assert_eq!(
            ordinary.validate_integrity(),
            Err(ProtocolErrorV4::SpecHashMismatch)
        );
        ordinary.spec_hash = Some(ordinary.calculate_spec_hash().unwrap());
        ordinary.execution_kind = RunExecutionKindV4::ApprovedPlan;
        ordinary.spec_hash = Some(ordinary.calculate_spec_hash().unwrap());
        assert_eq!(
            ordinary.validate_integrity(),
            Err(ProtocolErrorV4::SpecHashMismatch)
        );
    }

    #[test]
    fn container_selection_requires_frozen_image_and_no_network() {
        let mut selection = ComputeSelectionV4 {
            schema_version: 4,
            backend_id: "docker".into(),
            backend_kind: ComputeBackendKindV4::Docker,
            autonomy_mode: AutonomyModeV4::FullAuto,
            approval_policy: ApprovalPolicyV4::FullAccess,
            environment: "system".into(),
            network_policy: NetworkPolicyV4::None,
            container_image: Some(ContainerImageSelectionV4 {
                reference: "omicsops/test:latest".into(),
                image_id: "sha256:abc".into(),
            }),
        };
        assert!(selection.validate().is_ok());
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "container analysis".into(),
            steps: vec!["analyze".into()],
            completion_criteria: vec!["verified".into()],
            requested_capabilities: BTreeSet::new(),
        };
        let ids = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let approval =
            RunSpecV4::approval_hash_for(ids.0, ids.1, ids.2, ids.3, &plan, &selection).unwrap();
        let mut spec = RunSpecV4::freeze_with_compute(
            ids.0,
            ids.1,
            ids.2,
            ids.3,
            plan,
            selection.clone(),
            &approval,
            Utc::now(),
        )
        .unwrap();
        spec.compute_selection
            .as_mut()
            .unwrap()
            .container_image
            .as_mut()
            .unwrap()
            .reference = "omicsops/tampered:latest".into();
        assert!(spec.validate_integrity().is_err());
        selection.container_image.as_mut().unwrap().image_id = "sha256:def".into();
        assert_ne!(
            selection.canonical_hash().unwrap(),
            ComputeSelectionV4 {
                container_image: Some(ContainerImageSelectionV4 {
                    reference: "omicsops/test:latest".into(),
                    image_id: "sha256:abc".into(),
                }),
                ..selection.clone()
            }
            .canonical_hash()
            .unwrap()
        );
        selection.network_policy = NetworkPolicyV4::HostInherited;
        assert_eq!(
            selection.validate(),
            Err(ProtocolErrorV4::InvalidComputeSelection)
        );
    }

    #[test]
    fn supported_backend_selection_matrix_is_explicit() {
        let ssh = ComputeSelectionV4 {
            schema_version: 4,
            backend_id: format!("ssh:{}", Uuid::new_v4()),
            backend_kind: ComputeBackendKindV4::Ssh,
            autonomy_mode: AutonomyModeV4::Supervised,
            approval_policy: ApprovalPolicyV4::RiskBased,
            environment: "analysis-r".into(),
            network_policy: NetworkPolicyV4::HostInherited,
            container_image: None,
        };
        assert!(ssh.validate().is_ok());
        let podman = ComputeSelectionV4 {
            schema_version: 4,
            backend_id: "podman".into(),
            backend_kind: ComputeBackendKindV4::Podman,
            autonomy_mode: AutonomyModeV4::FullAuto,
            approval_policy: ApprovalPolicyV4::FullAccess,
            environment: "system".into(),
            network_policy: NetworkPolicyV4::None,
            container_image: Some(ContainerImageSelectionV4 {
                reference: "localhost/omicsops:test".into(),
                image_id: "sha256:123".into(),
            }),
        };
        assert!(podman.validate().is_ok());
        let mut invalid = ssh;
        invalid.autonomy_mode = AutonomyModeV4::FullAuto;
        assert!(invalid.validate().is_err());
        invalid.autonomy_mode = AutonomyModeV4::Supervised;
        invalid.approval_policy = ApprovalPolicyV4::FullAccess;
        assert!(invalid.validate().is_err());
        let mut mismatched_container = podman;
        mismatched_container.approval_policy = ApprovalPolicyV4::RiskBased;
        assert!(mismatched_container.validate().is_err());
    }

    #[test]
    fn tool_approval_is_bound_to_run_spec_call_and_effect() {
        let run_id = Uuid::new_v4();
        let call = ToolCallV4 {
            call_id: "call-1".into(),
            tool_id: "runtime.execute".into(),
            arguments: serde_json::json!({"code":"print(1)"}),
        };
        let request = ToolApprovalRequestV4::new(
            run_id,
            "spec-hash",
            call,
            ToolEffectV4::Runtime,
            "first local execution",
        )
        .unwrap();
        assert!(request.validate(run_id, "spec-hash").is_ok());

        let mut tampered_call = request.clone();
        tampered_call.call.arguments = serde_json::json!({"code":"print(2)"});
        assert_eq!(
            tampered_call.validate(run_id, "spec-hash"),
            Err(ProtocolErrorV4::InvalidToolApproval)
        );
        assert_eq!(
            request.validate(Uuid::new_v4(), "spec-hash"),
            Err(ProtocolErrorV4::InvalidToolApproval)
        );
        assert_eq!(
            request.validate(run_id, "other-spec"),
            Err(ProtocolErrorV4::InvalidToolApproval)
        );
        let mut tampered_effect = request;
        tampered_effect.effect = ToolEffectV4::Network;
        assert_eq!(
            tampered_effect.validate(run_id, "spec-hash"),
            Err(ProtocolErrorV4::InvalidToolApproval)
        );
    }

    #[test]
    fn plan_tool_approval_is_bound_to_scope_and_keeps_execute_compatibility() {
        let run_id = Uuid::new_v4();
        let call = ToolCallV4 {
            call_id: "plan-mcp".into(),
            tool_id: "use_mcp_tool".into(),
            arguments: serde_json::json!({
                "server_id": Uuid::new_v4(),
                "tool": "search",
                "catalog_sha256": "catalog",
                "schema_sha256": "schema",
                "arguments": {"q":"x"}
            }),
        };
        let request = ToolApprovalRequestV4::new_with_scope(
            run_id,
            "plan-scope",
            call,
            ToolEffectV4::Network,
            "third-party readOnlyHint is an unverified hint trusted by the user, not a Host guarantee",
        )
        .unwrap();
        assert_eq!(request.mode, RunModeV4::Plan);
        assert_eq!(request.scope_hash.as_deref(), Some("plan-scope"));
        assert!(
            request
                .validate_with_scope(run_id, "plan-scope", RunModeV4::Plan)
                .is_ok()
        );
        assert!(
            request
                .validate_with_scope(run_id, "other-scope", RunModeV4::Plan)
                .is_err()
        );
        assert!(
            request
                .validate_with_scope(run_id, "plan-scope", RunModeV4::Execute)
                .is_err()
        );

        let legacy = ToolApprovalRequestV4::new(
            run_id,
            "spec-hash",
            ToolCallV4 {
                call_id: "execute".into(),
                tool_id: "runtime.execute".into(),
                arguments: serde_json::json!({"code":"x"}),
            },
            ToolEffectV4::Runtime,
            "legacy",
        )
        .unwrap();
        assert!(legacy.validate(run_id, "spec-hash").is_ok());
        assert!(legacy.scope_hash.is_none());
    }

    #[test]
    fn historical_execute_approval_json_defaults_without_changing_its_id() {
        let run_id = Uuid::new_v4();
        let request = ToolApprovalRequestV4::new(
            run_id,
            "legacy-spec",
            ToolCallV4 {
                call_id: "legacy-call".into(),
                tool_id: "runtime.execute".into(),
                arguments: serde_json::json!({"code":"print(1)"}),
            },
            ToolEffectV4::Runtime,
            "legacy approval",
        )
        .unwrap();
        let mut historical = serde_json::to_value(&request).unwrap();
        let object = historical.as_object_mut().unwrap();
        object.remove("mode");
        object.remove("scope_hash");
        let decoded: ToolApprovalRequestV4 = serde_json::from_value(historical).unwrap();
        assert_eq!(decoded.mode, RunModeV4::Execute);
        assert!(decoded.scope_hash.is_none());
        assert_eq!(decoded.approval_id, request.approval_id);
        assert!(decoded.validate(run_id, "legacy-spec").is_ok());
        let serialized = serde_json::to_string(&decoded).unwrap();
        assert!(!serialized.contains("\"mode\""));
        assert!(!serialized.contains("\"scope_hash\""));
    }
}

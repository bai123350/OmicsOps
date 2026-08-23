use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const AGENT_RUNTIME_V4: &str = "omicsops.agent-runtime@4.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunModeV4 {
    Plan,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusV4 {
    Planning,
    AwaitingApproval,
    Running,
    WaitingForInput,
    Completed,
    Failed,
    Cancelled,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelErrorClassV4 {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunSpecV4 {
    pub schema_version: u8,
    pub runtime_id: String,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
    pub plan: ExecutionPlanV4,
    pub approved_plan_hash: String,
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
            approved_plan_hash: actual,
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
            compute_selection: Some(selection),
            approval_hash: Some(expected),
            spec_hash: None,
            created_at: now,
        };
        spec.spec_hash = Some(spec.calculate_spec_hash()?);
        Ok(spec)
    }

    pub fn calculate_spec_hash(&self) -> Result<String, ProtocolErrorV4> {
        let value = serde_json::json!({
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
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(&value).map_err(|_| ProtocolErrorV4::InvalidComputeSelection)?,
        )))
    }

    pub fn validate_integrity(&self) -> Result<(), ProtocolErrorV4> {
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

/// Evidence gate used before removing the legacy V2/V3 execution paths.
/// Keeping this as data makes retirement an explicit, auditable decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct LegacyRetirementReadinessV4 {
    pub python_kernel_verified: bool,
    pub r_kernel_verified: bool,
    pub pbmc3k_verified: bool,
    pub crash_recovery_verified: bool,
    pub scientific_verifier_verified: bool,
    pub security_scenarios_verified: bool,
}

impl LegacyRetirementReadinessV4 {
    pub fn ready(&self) -> bool {
        self.python_kernel_verified
            && self.r_kernel_verified
            && self.pbmc3k_verified
            && self.crash_recovery_verified
            && self.scientific_verifier_verified
            && self.security_scenarios_verified
    }

    pub fn missing_evidence(&self) -> Vec<&'static str> {
        [
            (!self.python_kernel_verified).then_some("python_kernel"),
            (!self.r_kernel_verified).then_some("r_kernel"),
            (!self.pbmc3k_verified).then_some("pbmc3k"),
            (!self.crash_recovery_verified).then_some("crash_recovery"),
            (!self.scientific_verifier_verified).then_some("scientific_verifier"),
            (!self.security_scenarios_verified).then_some("security_scenarios"),
        ]
        .into_iter()
        .flatten()
        .collect()
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
        let envelope = serde_json::json!({"schema_version":self.schema_version,"run_id":self.run_id,"project_id":self.project_id,"conversation_id":self.conversation_id,"sequence":self.sequence,"occurred_at":self.occurred_at,"previous_hash":self.previous_hash,"event":self.event});
        hex::encode(Sha256::digest(
            serde_json::to_vec(&envelope).expect("serializable V4 event"),
        ))
    }
    pub fn verify(&self) -> Result<(), ProtocolErrorV4> {
        if self.event_hash == self.calculate_hash() {
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
    Ok(())
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
    #[error("broken V4 event chain")]
    BrokenEventChain,
    #[error("invalid V4 reviewer report")]
    InvalidReview,
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn legacy_retirement_requires_every_acceptance_evidence() {
        let mut gate = LegacyRetirementReadinessV4::default();
        assert!(!gate.ready());
        assert_eq!(gate.missing_evidence().len(), 6);
        gate.python_kernel_verified = true;
        gate.r_kernel_verified = true;
        gate.pbmc3k_verified = true;
        gate.crash_recovery_verified = true;
        gate.scientific_verifier_verified = true;
        gate.security_scenarios_verified = true;
        assert!(gate.ready());
        assert!(gate.missing_evidence().is_empty());
    }

    fn local_selection() -> ComputeSelectionV4 {
        ComputeSelectionV4 {
            schema_version: 4,
            backend_id: "local".into(),
            backend_kind: ComputeBackendKindV4::Local,
            autonomy_mode: AutonomyModeV4::Supervised,
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
    fn container_selection_requires_frozen_image_and_no_network() {
        let mut selection = ComputeSelectionV4 {
            schema_version: 4,
            backend_id: "docker".into(),
            backend_kind: ComputeBackendKindV4::Docker,
            autonomy_mode: AutonomyModeV4::FullAuto,
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
    }
}

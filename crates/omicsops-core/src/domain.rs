use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{CoreError, CoreResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMethod {
    Password,
    PrivateKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub authentication: AuthenticationMethod,
    pub authentication_reference: String,
    pub host_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DataSource {
    pub label: String,
    pub url: Url,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceLimits {
    pub max_cpu_cores: u16,
    pub max_memory_gib: u32,
    pub max_disk_gib: u32,
    pub max_step_seconds: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_cpu_cores: 8,
            max_memory_gib: 32,
            max_disk_gib: 200,
            max_step_seconds: 86_400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectSpec {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub remote_root: String,
    pub plan_summary: String,
    pub data_sources: Vec<DataSource>,
    pub resource_limits: ResourceLimits,
    pub allowed_network_domains: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepRisk {
    Low,
    Medium,
    High,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompletionCondition {
    pub kind: CompletionConditionKind,
    pub target: String,
    pub expected: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompletionConditionKind {
    ExitCode,
    FileExists,
    Sha256,
    JsonField,
    CommandSucceeds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StepSpec {
    pub id: String,
    pub title: String,
    pub rationale: String,
    pub command: String,
    pub working_directory: String,
    pub timeout_seconds: u64,
    pub risk: StepRisk,
    pub completion_conditions: Vec<CompletionCondition>,
    pub expected_artifacts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StageSpec {
    pub id: String,
    pub goal: String,
    pub dependencies: Vec<String>,
    pub steps: Vec<StepSpec>,
    pub expected_artifacts: Vec<String>,
    pub completion_conditions: Vec<CompletionCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisPlan {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub stages: Vec<StageSpec>,
    pub resource_budget: ResourceLimits,
    pub highest_risk: StepRisk,
    pub approved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Draft,
    Inspecting,
    AwaitingPlanApproval,
    Preparing,
    Running,
    PausedForApproval,
    Succeeded,
    Failed,
    Canceled,
}

impl std::fmt::Display for RunState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}",
            serde_json::to_value(self).unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunEvent {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub run_id: Uuid,
    pub stage_id: Option<String>,
    pub step_id: Option<String>,
    pub attempt: u8,
    pub action: String,
    pub state: RunState,
    pub log_reference: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunCheckpoint {
    pub run_id: Uuid,
    pub profile_id: Uuid,
    pub project_id: Uuid,
    pub plan_id: Uuid,
    pub state: RunState,
    pub stage_index: usize,
    pub step_index: usize,
    pub pending_approval: Option<ApprovalRequest>,
}

impl RunCheckpoint {
    pub fn new(run_id: Uuid, profile_id: Uuid, project_id: Uuid, plan_id: Uuid) -> Self {
        Self {
            run_id,
            profile_id,
            project_id,
            plan_id,
            state: RunState::Preparing,
            stage_index: 0,
            step_index: 0,
            pending_approval: None,
        }
    }

    pub fn current_step<'a>(
        &self,
        plan: &'a AnalysisPlan,
    ) -> Option<(&'a StageSpec, &'a StepSpec)> {
        self.current_position(plan)
            .map(|(stage_index, step_index)| {
                let stage = &plan.stages[stage_index];
                (stage, &stage.steps[step_index])
            })
    }

    pub fn advance_after_success(&mut self, plan: &AnalysisPlan) -> bool {
        let Some((stage_index, step_index)) = self.current_position(plan) else {
            return true;
        };
        self.stage_index = stage_index;
        self.step_index = step_index + 1;
        if let Some((next_stage, next_step)) = self.current_position(plan) {
            self.stage_index = next_stage;
            self.step_index = next_step;
            false
        } else {
            self.stage_index = plan.stages.len();
            self.step_index = 0;
            true
        }
    }

    pub fn pause_for_approval(&mut self, reason: impl Into<String>, command: impl Into<String>) {
        self.state = RunState::PausedForApproval;
        self.pending_approval = Some(ApprovalRequest {
            id: Uuid::new_v4(),
            run_id: self.run_id,
            reason: reason.into(),
            proposed_action: command.into(),
            impact: "The approved command will run inside the remote project.".into(),
            alternatives: vec!["Reject and keep the run paused.".into()],
        });
    }

    pub fn approve(&mut self, request_id: Uuid) -> CoreResult<String> {
        let request = self.pending_approval.as_ref().ok_or_else(|| {
            CoreError::Validation("the run has no pending approval request".into())
        })?;
        if request.id != request_id {
            return Err(CoreError::Validation(
                "approval request does not match the pending action".into(),
            ));
        }
        let command = request.proposed_action.clone();
        self.pending_approval = None;
        self.state = RunState::Preparing;
        Ok(command)
    }

    fn current_position(&self, plan: &AnalysisPlan) -> Option<(usize, usize)> {
        plan.stages
            .iter()
            .enumerate()
            .skip(self.stage_index)
            .find_map(|(stage_index, stage)| {
                let first_step = if stage_index == self.stage_index {
                    self.step_index
                } else {
                    0
                };
                (first_step < stage.steps.len()).then_some((stage_index, first_step))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub run_id: Uuid,
    pub reason: String,
    pub proposed_action: String,
    pub impact: String,
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    H5ad,
    SeuratRds,
    HtmlReport,
    Figure,
    EnvironmentLock,
    AuditLog,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Artifact {
    pub remote_path: String,
    pub kind: ArtifactKind,
    pub size_bytes: u64,
    pub sha256: String,
    pub previewable: bool,
    pub downloadable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RepairDecision {
    pub diagnosis: String,
    pub strategy: String,
    pub replacement_command: String,
    pub completion_conditions: Vec<CompletionCondition>,
}

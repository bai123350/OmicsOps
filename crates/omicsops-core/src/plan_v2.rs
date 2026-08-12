use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    CoreError, CoreResult,
    domain::{ResourceLimits, StepRisk},
};

pub const PLAN_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanEnvironment {
    Micromamba {
        channels: Vec<String>,
        dependencies: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepAction {
    Tool {
        tool_id: String,
        version: String,
        arguments: Value,
    },
    LegacyShell {
        command: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationSpec {
    ExitCode {
        expected: u32,
    },
    File {
        path: String,
        min_bytes: u64,
        sha256: Option<String>,
    },
    JsonField {
        path: String,
        pointer: String,
        expected: String,
    },
    Table {
        path: String,
        delimiter: char,
        required_columns: Vec<String>,
        min_rows: u64,
    },
    DomainReport {
        path: String,
        validator: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StepSpecV2 {
    pub id: String,
    pub title: String,
    pub rationale: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub action: StepAction,
    #[serde(default = "default_working_directory")]
    pub working_directory: String,
    #[serde(default)]
    pub resources: ResourceLimits,
    #[serde(default = "default_step_risk")]
    pub risk: StepRisk,
    #[serde(default)]
    pub verifications: Vec<VerificationSpec>,
    #[serde(default)]
    pub expected_artifacts: Vec<String>,
}

fn default_working_directory() -> String {
    ".".into()
}

fn default_step_risk() -> StepRisk {
    StepRisk::Low
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanStageV2 {
    pub id: String,
    pub goal: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub steps: Vec<StepSpecV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyEnvelope {
    pub allowed_tools: Vec<String>,
    pub allowed_domains: Vec<String>,
    pub max_risk: StepRisk,
    pub allow_legacy_shell: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisPlanV2 {
    pub schema_version: u16,
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub environment: PlanEnvironment,
    pub stages: Vec<PlanStageV2>,
    pub resource_budget: ResourceLimits,
    pub policy: PolicyEnvelope,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovedPlan {
    pub id: Uuid,
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub policy: PolicyEnvelope,
    pub approved_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStateV2 {
    Preparing,
    Running,
    PausedForApproval,
    NeedsAttention,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunCheckpointV2 {
    pub run_id: Uuid,
    pub profile_id: Uuid,
    pub project_id: Uuid,
    pub approved_plan_id: Uuid,
    pub state: RunStateV2,
    pub completed_steps: std::collections::BTreeSet<String>,
    pub action_hashes: BTreeMap<String, String>,
    pub attention_reason: Option<String>,
}

impl RunCheckpointV2 {
    pub fn new(run_id: Uuid, profile_id: Uuid, project_id: Uuid, approved_plan_id: Uuid) -> Self {
        Self {
            run_id,
            profile_id,
            project_id,
            approved_plan_id,
            state: RunStateV2::Preparing,
            completed_steps: Default::default(),
            action_hashes: Default::default(),
            attention_reason: None,
        }
    }

    pub fn mark_verified(&mut self, step_id: impl Into<String>, action_hash: impl Into<String>) {
        let step_id = step_id.into();
        self.completed_steps.insert(step_id.clone());
        self.action_hashes.insert(step_id, action_hash.into());
    }

    pub fn needs_attention(&mut self, reason: impl Into<String>) {
        self.state = RunStateV2::NeedsAttention;
        self.attention_reason = Some(reason.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VerificationResult {
    pub specification: VerificationSpec,
    pub passed: bool,
    pub observed: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StepAttempt {
    pub run_id: Uuid,
    pub step_id: String,
    pub attempt: u8,
    pub action_hash: String,
    pub process_group_id: Option<u32>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub exit_code: Option<u32>,
    pub log_path: String,
    pub manifest_path: String,
    pub verifications: Vec<VerificationResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EnvironmentLock {
    pub run_id: Uuid,
    pub backend: String,
    pub remote_path: String,
    pub sha256: String,
    pub declared_dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactRecordV2 {
    pub run_id: Uuid,
    pub source_step_id: String,
    pub remote_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanningTurn {
    Clarification { questions: Vec<String> },
    Draft { plan: AnalysisPlanV2 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanningRequest {
    pub goal: String,
    pub environment_summary: String,
    #[serde(default)]
    pub answers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActionDiff {
    pub field: String,
    pub approved: String,
    pub proposed: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RepairProposalV2 {
    pub diagnosis: String,
    pub strategy: String,
    pub replacement_action: StepAction,
}

pub fn canonical_plan_hash(plan: &AnalysisPlanV2) -> CoreResult<String> {
    let value = serde_json::to_value(plan)?;
    let canonical = canonical_json(&value);
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn action_hash(action: &StepAction) -> CoreResult<String> {
    let value = serde_json::to_value(action)?;
    let bytes = serde_json::to_vec(&canonical_json(&value))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut sorted = object.iter().collect::<Vec<_>>();
            sorted.sort_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        _ => value.clone(),
    }
}

pub fn require_supported_schema(plan: &AnalysisPlanV2) -> CoreResult<()> {
    if plan.schema_version != PLAN_SCHEMA_VERSION {
        return Err(CoreError::Validation(format!(
            "unsupported plan schema version {}",
            plan.schema_version
        )));
    }
    Ok(())
}

pub fn risk_rank(risk: StepRisk) -> u8 {
    match risk {
        StepRisk::Low => 0,
        StepRisk::Medium => 1,
        StepRisk::High => 2,
        StepRisk::Destructive => 3,
    }
}

pub fn migrate_v1_plan(plan: &crate::domain::AnalysisPlan) -> AnalysisPlanV2 {
    let stages = plan
        .stages
        .iter()
        .map(|stage| PlanStageV2 {
            id: stage.id.clone(),
            goal: stage.goal.clone(),
            dependencies: stage.dependencies.clone(),
            steps: stage
                .steps
                .iter()
                .map(|step| {
                    let mut resources = plan.resource_budget.clone();
                    resources.max_step_seconds = step.timeout_seconds;
                    let mut verifications = step
                        .completion_conditions
                        .iter()
                        .filter_map(|condition| match condition.kind {
                            crate::domain::CompletionConditionKind::ExitCode => {
                                Some(VerificationSpec::ExitCode {
                                    expected: condition
                                        .expected
                                        .as_deref()
                                        .unwrap_or("0")
                                        .parse()
                                        .unwrap_or(0),
                                })
                            }
                            crate::domain::CompletionConditionKind::FileExists => {
                                Some(VerificationSpec::File {
                                    path: condition.target.clone(),
                                    min_bytes: 0,
                                    sha256: None,
                                })
                            }
                            crate::domain::CompletionConditionKind::Sha256 => {
                                Some(VerificationSpec::File {
                                    path: condition.target.clone(),
                                    min_bytes: 0,
                                    sha256: condition.expected.clone(),
                                })
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if verifications.is_empty() {
                        verifications.push(VerificationSpec::ExitCode { expected: 0 });
                    }
                    StepSpecV2 {
                        id: step.id.clone(),
                        title: step.title.clone(),
                        rationale: step.rationale.clone(),
                        dependencies: Vec::new(),
                        action: StepAction::LegacyShell {
                            command: step.command.clone(),
                        },
                        working_directory: step.working_directory.clone(),
                        resources,
                        risk: step.risk,
                        verifications,
                        expected_artifacts: step.expected_artifacts.clone(),
                    }
                })
                .collect(),
        })
        .collect();
    AnalysisPlanV2 {
        schema_version: PLAN_SCHEMA_VERSION,
        id: plan.id,
        title: plan.title.clone(),
        summary: plan.summary.clone(),
        environment: PlanEnvironment::Micromamba {
            channels: Vec::new(),
            dependencies: Vec::new(),
        },
        stages,
        resource_budget: plan.resource_budget.clone(),
        policy: PolicyEnvelope {
            allowed_tools: Vec::new(),
            allowed_domains: Vec::new(),
            max_risk: plan.highest_risk,
            allow_legacy_shell: true,
        },
        metadata: BTreeMap::from([
            ("migrated_from".into(), "v1".into()),
            ("approval_status".into(), "reapproval_required".into()),
        ]),
    }
}

pub fn topological_steps(plan: &AnalysisPlanV2) -> CoreResult<Vec<&StepSpecV2>> {
    let mut remaining = plan
        .stages
        .iter()
        .flat_map(|stage| stage.steps.iter())
        .collect::<Vec<_>>();
    let known = remaining
        .iter()
        .map(|step| step.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(dependency) = remaining
        .iter()
        .flat_map(|step| step.dependencies.iter())
        .find(|dependency| !known.contains(dependency.as_str()))
    {
        return Err(CoreError::Validation(format!(
            "unknown step dependency {dependency}"
        )));
    }
    let mut completed = std::collections::BTreeSet::new();
    let mut ordered = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let Some(index) = remaining.iter().position(|step| {
            step.dependencies
                .iter()
                .all(|dependency| completed.contains(dependency))
        }) else {
            return Err(CoreError::Validation(
                "step dependency graph contains a cycle".into(),
            ));
        };
        let step = remaining.remove(index);
        completed.insert(step.id.clone());
        ordered.push(step);
    }
    Ok(ordered)
}

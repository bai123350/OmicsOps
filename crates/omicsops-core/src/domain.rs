use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

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

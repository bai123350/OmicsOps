mod context;
mod engine;
mod events;
mod model;
mod review;
mod tools;

pub use context::*;
pub use engine::*;
pub use events::*;
pub use model::*;
pub use review::*;
pub use tools::*;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const HARNESS_ID_V3: &str = "agent.harness_v3@3.0.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLimitsV3 {
    pub max_model_steps: u32,
    pub max_tool_calls: u32,
    pub max_reviewer_corrections: u8,
    pub max_parallel_read_only: u8,
}

impl Default for RunLimitsV3 {
    fn default() -> Self {
        Self {
            max_model_steps: 64,
            max_tool_calls: 48,
            max_reviewer_corrections: 2,
            max_parallel_read_only: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionCriterionV3 {
    pub id: String,
    pub description: String,
}

impl CompletionCriterionV3 {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReferenceV3 {
    pub package_id: String,
    pub version: String,
    pub sha256: String,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunSpecV3 {
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub objective: String,
    pub completion_criteria: Vec<CompletionCriterionV3>,
    pub tool_snapshot: Vec<ToolDefinitionV3>,
    pub skill_references: Vec<SkillReferenceV3>,
    pub risk_budget: Vec<String>,
    pub harness_id: String,
    pub harness_version: u8,
    pub limits: RunLimitsV3,
}

impl AgentRunSpecV3 {
    pub fn new(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        objective: impl Into<String>,
        completion_criteria: Vec<CompletionCriterionV3>,
        tool_snapshot: Vec<ToolDefinitionV3>,
        skill_references: Vec<SkillReferenceV3>,
    ) -> Self {
        Self {
            run_id,
            project_id,
            conversation_id,
            objective: objective.into(),
            completion_criteria,
            tool_snapshot,
            skill_references,
            risk_budget: Vec::new(),
            harness_id: HARNESS_ID_V3.into(),
            harness_version: 3,
            limits: RunLimitsV3::default(),
        }
    }
}

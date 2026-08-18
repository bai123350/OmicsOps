use std::collections::BTreeSet;

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
            created_at: now,
        })
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OutputCaptureV4 {
    pub excerpt: String,
    pub total_bytes: u64,
    pub sha256: String,
    pub archive_path: String,
    pub truncated: bool,
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
    pub artifacts: Vec<String>,
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
    CompletionProposed,
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
    #[error("V4 event hash mismatch")]
    EventHashMismatch,
    #[error("broken V4 event chain")]
    BrokenEventChain,
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
}

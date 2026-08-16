use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use super::{
    AgentRunSpecV3, ReviewReportV3, ToolCallRequestV3, ToolOutcomeStatusV3, ToolOutcomeV3,
};

const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EventChainErrorV3 {
    #[error("event chain is empty")]
    Empty,
    #[error("event sequence {actual} is not the expected sequence {expected}")]
    Sequence { expected: u64, actual: u64 },
    #[error("event identity changed at sequence {0}")]
    Identity(u64),
    #[error("previous hash mismatch at sequence {0}")]
    PreviousHash(u64),
    #[error("event hash mismatch at sequence {0}")]
    EventHash(u64),
    #[error("event serialization failed: {0}")]
    Serialization(String),
    #[error("run specification does not match event chain")]
    SpecMismatch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum AgentRunEventKindV3 {
    RunStarted,
    RunCancelled,
    ModelStepStarted {
        step: u32,
    },
    ModelText {
        text: String,
    },
    ToolCallRequested {
        request: ToolCallRequestV3,
    },
    ToolCallDispatched {
        request: ToolCallRequestV3,
        read_only: bool,
    },
    ToolCallFinished {
        outcome: ToolOutcomeV3,
    },
    UserInputRequested {
        question_id: String,
        question: String,
    },
    UserInputAnswered {
        question_id: String,
        answer: String,
    },
    ContextCompacted {
        first_sequence: u64,
        last_sequence: u64,
        last_event_hash: String,
    },
    CompletionProposed,
    ReviewCompleted {
        report: ReviewReportV3,
    },
    RunCompleted,
    NeedsAttention {
        reason: String,
    },
    RunFailed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunEventV3 {
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub sequence: u64,
    pub previous_hash: String,
    pub event_hash: String,
    pub occurred_at: DateTime<Utc>,
    pub event: AgentRunEventKindV3,
}

#[derive(Serialize)]
struct HashEnvelope<'a> {
    run_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    sequence: u64,
    previous_hash: &'a str,
    occurred_at: DateTime<Utc>,
    event: &'a AgentRunEventKindV3,
}

impl AgentRunEventV3 {
    pub fn first(
        spec: &AgentRunSpecV3,
        occurred_at: DateTime<Utc>,
        event: AgentRunEventKindV3,
    ) -> Result<Self, EventChainErrorV3> {
        Self::create(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            1,
            GENESIS_HASH.into(),
            occurred_at,
            event,
        )
    }

    pub fn next(
        previous: &Self,
        occurred_at: DateTime<Utc>,
        event: AgentRunEventKindV3,
    ) -> Result<Self, EventChainErrorV3> {
        Self::create(
            previous.run_id,
            previous.project_id,
            previous.conversation_id,
            previous.sequence + 1,
            previous.event_hash.clone(),
            occurred_at,
            event,
        )
    }

    fn create(
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        sequence: u64,
        previous_hash: String,
        occurred_at: DateTime<Utc>,
        event: AgentRunEventKindV3,
    ) -> Result<Self, EventChainErrorV3> {
        let event_hash = calculate_hash(
            run_id,
            project_id,
            conversation_id,
            sequence,
            &previous_hash,
            occurred_at,
            &event,
        )?;
        Ok(Self {
            run_id,
            project_id,
            conversation_id,
            sequence,
            previous_hash,
            event_hash,
            occurred_at,
            event,
        })
    }

    pub fn verify_hash(&self) -> Result<(), EventChainErrorV3> {
        let actual = calculate_hash(
            self.run_id,
            self.project_id,
            self.conversation_id,
            self.sequence,
            &self.previous_hash,
            self.occurred_at,
            &self.event,
        )?;
        if actual == self.event_hash {
            Ok(())
        } else {
            Err(EventChainErrorV3::EventHash(self.sequence))
        }
    }
}

fn calculate_hash(
    run_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    sequence: u64,
    previous_hash: &str,
    occurred_at: DateTime<Utc>,
    event: &AgentRunEventKindV3,
) -> Result<String, EventChainErrorV3> {
    let envelope = HashEnvelope {
        run_id,
        project_id,
        conversation_id,
        sequence,
        previous_hash,
        occurred_at,
        event,
    };
    let canonical = serde_json::to_vec(&envelope)
        .map_err(|error| EventChainErrorV3::Serialization(error.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(previous_hash.as_bytes());
    hasher.update(canonical);
    Ok(hex::encode(hasher.finalize()))
}

pub fn validate_event_chain(events: &[AgentRunEventV3]) -> Result<(), EventChainErrorV3> {
    let first = events.first().ok_or(EventChainErrorV3::Empty)?;
    let identity = (first.run_id, first.project_id, first.conversation_id);
    let mut expected_sequence = 1;
    let mut previous_hash = GENESIS_HASH;
    for event in events {
        if event.sequence != expected_sequence {
            return Err(EventChainErrorV3::Sequence {
                expected: expected_sequence,
                actual: event.sequence,
            });
        }
        if (event.run_id, event.project_id, event.conversation_id) != identity {
            return Err(EventChainErrorV3::Identity(event.sequence));
        }
        if event.previous_hash != previous_hash {
            return Err(EventChainErrorV3::PreviousHash(event.sequence));
        }
        event.verify_hash()?;
        expected_sequence += 1;
        previous_hash = &event.event_hash;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusV3 {
    Queued,
    Running,
    WaitingForInput,
    Reviewing,
    Recovering,
    Completed,
    NeedsAttention,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunStateV3 {
    pub run_id: Uuid,
    pub status: RunStatusV3,
    pub last_sequence: u64,
    pub last_event_hash: String,
    pub model_steps: u32,
    pub tool_calls: u32,
    pub uncertain_side_effects: Vec<String>,
    pub successful_idempotency_keys: BTreeSet<String>,
    pub reviewer_corrections: u8,
}

impl AgentRunStateV3 {
    pub fn replay(
        spec: &AgentRunSpecV3,
        events: &[AgentRunEventV3],
    ) -> Result<Self, EventChainErrorV3> {
        validate_event_chain(events)?;
        if events[0].run_id != spec.run_id
            || events[0].project_id != spec.project_id
            || events[0].conversation_id != spec.conversation_id
        {
            return Err(EventChainErrorV3::SpecMismatch);
        }

        let mut state = Self {
            run_id: spec.run_id,
            status: RunStatusV3::Queued,
            last_sequence: 0,
            last_event_hash: GENESIS_HASH.into(),
            model_steps: 0,
            tool_calls: 0,
            uncertain_side_effects: Vec::new(),
            successful_idempotency_keys: BTreeSet::new(),
            reviewer_corrections: 0,
        };
        let mut dispatched: BTreeMap<String, (ToolCallRequestV3, bool)> = BTreeMap::new();
        for event in events {
            state.last_sequence = event.sequence;
            state.last_event_hash.clone_from(&event.event_hash);
            match &event.event {
                AgentRunEventKindV3::RunStarted => state.status = RunStatusV3::Running,
                AgentRunEventKindV3::RunCancelled => state.status = RunStatusV3::Cancelled,
                AgentRunEventKindV3::ModelStepStarted { step } => {
                    state.model_steps = state.model_steps.max(*step)
                }
                AgentRunEventKindV3::ToolCallDispatched { request, read_only } => {
                    state.tool_calls += 1;
                    dispatched.insert(request.call_id.clone(), (request.clone(), *read_only));
                }
                AgentRunEventKindV3::ToolCallFinished { outcome } => {
                    if let Some((request, _)) = dispatched.remove(&outcome.call_id) {
                        if outcome.status == ToolOutcomeStatusV3::Succeeded {
                            state
                                .successful_idempotency_keys
                                .insert(request.idempotency_key);
                        }
                    }
                }
                AgentRunEventKindV3::UserInputRequested { .. } => {
                    state.status = RunStatusV3::WaitingForInput
                }
                AgentRunEventKindV3::UserInputAnswered { .. } => {
                    state.status = RunStatusV3::Running
                }
                AgentRunEventKindV3::CompletionProposed => state.status = RunStatusV3::Reviewing,
                AgentRunEventKindV3::ReviewCompleted { report } => {
                    state.status = report.next_status(
                        state.reviewer_corrections,
                        spec.limits.max_reviewer_corrections,
                    );
                    if report.has_errors() && state.status == RunStatusV3::Running {
                        state.reviewer_corrections += 1;
                    }
                }
                AgentRunEventKindV3::RunCompleted => state.status = RunStatusV3::Completed,
                AgentRunEventKindV3::NeedsAttention { .. } => {
                    state.status = RunStatusV3::NeedsAttention
                }
                AgentRunEventKindV3::RunFailed { .. } => state.status = RunStatusV3::Failed,
                _ => {}
            }
        }
        state.uncertain_side_effects = dispatched
            .into_iter()
            .filter_map(|(call_id, (_, read_only))| (!read_only).then_some(call_id))
            .collect();
        if !state.uncertain_side_effects.is_empty()
            && matches!(state.status, RunStatusV3::Running | RunStatusV3::Queued)
        {
            state.status = RunStatusV3::Recovering;
        }
        Ok(state)
    }

    pub fn can_execute_side_effects(&self) -> Result<(), &'static str> {
        if self.uncertain_side_effects.is_empty() {
            Ok(())
        } else {
            Err("uncertain side effects must be resolved with read-only inspection")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSnapshotV3 {
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub last_sequence: u64,
    pub last_event_hash: String,
    pub state: AgentRunStateV3,
}

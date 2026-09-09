use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::{AgentResult, ModelMessage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderToolSpec {
    pub id: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub system: String,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ProviderToolSpec>,
    #[serde(default)]
    pub require_strict_json_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderStreamEvent {
    TextDelta {
        text: String,
    },
    ToolCallStarted {
        call_id: String,
        index: u32,
        tool_id: String,
    },
    ToolArgumentsDelta {
        call_id: String,
        index: u32,
        arguments: String,
    },
    ToolCallCompleted {
        call_id: String,
        index: u32,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        /// Built-in adapters retain only allowlisted numeric usage counters here,
        /// never raw response bodies. Reasoning tokens are not an effort report.
        #[serde(default)]
        provider_json: Value,
    },
    Retrying {
        attempt: u8,
        delay_ms: u64,
        message: String,
    },
    Error {
        code: String,
        message: String,
        repair_attempted: bool,
    },
    Completed,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn profile_id(&self) -> Uuid;
    async fn stream_provider(
        &self,
        request: ProviderRequest,
    ) -> AgentResult<Vec<ProviderStreamEvent>>;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolCallAssemblyError {
    #[error("tool call {0} started more than once")]
    DuplicateCall(String),
    #[error("tool arguments referenced unknown call {0}")]
    UnknownCall(String),
    #[error("tool call {call_id} changed stream index from {expected} to {actual}")]
    IndexChanged {
        call_id: String,
        expected: u32,
        actual: u32,
    },
    #[error("tool call {call_id} returned malformed JSON arguments: {message}")]
    MalformedArguments { call_id: String, message: String },
    #[error("strict JSON repair was already attempted for tool call {0}")]
    RepairAlreadyAttempted(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssembledToolCall {
    pub call_id: String,
    pub index: u32,
    pub tool_id: String,
    pub arguments: Value,
    pub repaired: bool,
}

#[derive(Debug, Clone)]
struct PendingToolCall {
    call_id: String,
    index: u32,
    tool_id: String,
    arguments: String,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderToolCallAccumulator {
    calls: BTreeMap<u32, PendingToolCall>,
    indexes_by_id: BTreeMap<String, u32>,
    repair_attempts: BTreeSet<String>,
}

impl ProviderToolCallAccumulator {
    pub fn push(&mut self, event: &ProviderStreamEvent) -> Result<(), ToolCallAssemblyError> {
        match event {
            ProviderStreamEvent::ToolCallStarted {
                call_id,
                index,
                tool_id,
            } => {
                if self.indexes_by_id.contains_key(call_id) || self.calls.contains_key(index) {
                    return Err(ToolCallAssemblyError::DuplicateCall(call_id.clone()));
                }
                self.indexes_by_id.insert(call_id.clone(), *index);
                self.calls.insert(
                    *index,
                    PendingToolCall {
                        call_id: call_id.clone(),
                        index: *index,
                        tool_id: tool_id.clone(),
                        arguments: String::new(),
                    },
                );
            }
            ProviderStreamEvent::ToolArgumentsDelta {
                call_id,
                index,
                arguments,
            } => {
                let expected = self
                    .indexes_by_id
                    .get(call_id)
                    .copied()
                    .ok_or_else(|| ToolCallAssemblyError::UnknownCall(call_id.clone()))?;
                if expected != *index {
                    return Err(ToolCallAssemblyError::IndexChanged {
                        call_id: call_id.clone(),
                        expected,
                        actual: *index,
                    });
                }
                self.calls
                    .get_mut(index)
                    .expect("tool call index is recorded with call id")
                    .arguments
                    .push_str(arguments);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn repair_once(
        &mut self,
        call_id: &str,
        strict_json: impl Into<String>,
    ) -> Result<(), ToolCallAssemblyError> {
        if !self.repair_attempts.insert(call_id.into()) {
            return Err(ToolCallAssemblyError::RepairAlreadyAttempted(
                call_id.into(),
            ));
        }
        let index = self
            .indexes_by_id
            .get(call_id)
            .copied()
            .ok_or_else(|| ToolCallAssemblyError::UnknownCall(call_id.into()))?;
        self.calls
            .get_mut(&index)
            .expect("tool call index is recorded with call id")
            .arguments = strict_json.into();
        Ok(())
    }

    pub fn finish(&self) -> Result<Vec<AssembledToolCall>, ToolCallAssemblyError> {
        self.calls
            .values()
            .map(|call| {
                let arguments: Value = serde_json::from_str(&call.arguments).map_err(|error| {
                    ToolCallAssemblyError::MalformedArguments {
                        call_id: call.call_id.clone(),
                        message: error.to_string(),
                    }
                })?;
                if !arguments.is_object() {
                    return Err(ToolCallAssemblyError::MalformedArguments {
                        call_id: call.call_id.clone(),
                        message: "tool arguments must be a JSON object".into(),
                    });
                }
                Ok(AssembledToolCall {
                    call_id: call.call_id.clone(),
                    index: call.index,
                    tool_id: call.tool_id.clone(),
                    arguments,
                    repaired: self.repair_attempts.contains(&call.call_id),
                })
            })
            .collect()
    }
}

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

/// How a provider reports the numeric values in one usage sample.
///
/// The built-in adapters currently emit cumulative samples.  `Delta` is kept
/// for a provider contract that explicitly documents deltas, while `Unknown`
/// is deliberately not treated as additive evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUsageAggregation {
    Cumulative,
    Delta,
    Unknown,
}

/// Whether a usage sample is a complete provider response or an observation
/// made while the response is still in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUsageState {
    Partial,
    Final,
    Interrupted,
}

/// Sanitized provider usage.  Every counter is optional: an absent provider
/// field remains `None`, while a provider-reported zero is `Some(0)`.  Raw
/// response JSON and unknown numeric fields intentionally have no place in
/// this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsageSample {
    /// Stable only within one provider attempt.  A new attempt starts at zero.
    pub sample_index: u32,
    pub aggregation: ProviderUsageAggregation,
    pub state: ProviderUsageState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Provider-normalized prompt/context occupancy. This is distinct from
    /// the billing/input facet: Anthropic only supplies it when its explicit
    /// input, cache-read and cache-creation counters can be checked-added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_total_tokens: Option<u64>,
}

/// Coalesces cumulative or explicitly documented delta samples from one
/// provider attempt.  Callers must create a fresh accumulator for every
/// retry attempt; the sample index is intentionally scoped to that attempt.
#[derive(Debug, Clone, Default)]
pub struct ProviderUsageAccumulator {
    seen_sample_indices: BTreeSet<u32>,
    merged: Option<ProviderUsageSample>,
}

impl ProviderUsageAccumulator {
    pub fn push(&mut self, sample: ProviderUsageSample) -> Option<ProviderUsageSample> {
        if !self.seen_sample_indices.insert(sample.sample_index) {
            return self.merged.clone();
        }

        let Some(mut merged) = self.merged.take() else {
            if sample.aggregation == ProviderUsageAggregation::Unknown {
                self.merged = Some(ProviderUsageSample {
                    input_tokens: None,
                    context_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_input_tokens: None,
                    cache_creation_input_tokens: None,
                    reported_total_tokens: None,
                    ..sample
                });
            } else {
                self.merged = Some(sample);
            }
            return self.merged.clone();
        };

        merged.sample_index = merged.sample_index.max(sample.sample_index);
        merged.state = merge_usage_state(merged.state, sample.state);

        if merged.aggregation == ProviderUsageAggregation::Unknown {
            self.merged = Some(merged);
            return self.merged.clone();
        }

        match (merged.aggregation, sample.aggregation) {
            (ProviderUsageAggregation::Cumulative, ProviderUsageAggregation::Cumulative) => {
                merge_cumulative_counters(&mut merged, &sample);
            }
            (ProviderUsageAggregation::Delta, ProviderUsageAggregation::Delta) => {
                if !add_delta_counters(&mut merged, &sample) {
                    merged.aggregation = ProviderUsageAggregation::Unknown;
                }
            }
            (ProviderUsageAggregation::Unknown, _) | (_, ProviderUsageAggregation::Unknown) => {
                merged.aggregation = ProviderUsageAggregation::Unknown;
            }
            _ => {
                // Mixing cumulative and delta values cannot be recovered
                // safely, so retain only the already verified subtotal and
                // mark the aggregate ambiguous.
                merged.aggregation = ProviderUsageAggregation::Unknown;
            }
        }

        self.merged = Some(merged);
        self.merged.clone()
    }

    pub fn snapshot(&self) -> Option<ProviderUsageSample> {
        self.merged.clone()
    }
}

pub fn coalesce_provider_usage_samples(
    samples: impl IntoIterator<Item = ProviderUsageSample>,
) -> Option<ProviderUsageSample> {
    let mut accumulator = ProviderUsageAccumulator::default();
    for sample in samples {
        accumulator.push(sample);
    }
    accumulator.snapshot()
}

fn merge_usage_state(current: ProviderUsageState, next: ProviderUsageState) -> ProviderUsageState {
    match (current, next) {
        (ProviderUsageState::Interrupted, _) | (_, ProviderUsageState::Interrupted) => {
            ProviderUsageState::Interrupted
        }
        (ProviderUsageState::Final, _) | (_, ProviderUsageState::Final) => {
            ProviderUsageState::Final
        }
        _ => ProviderUsageState::Partial,
    }
}

fn merge_max(current: &mut Option<u64>, next: Option<u64>) {
    if let Some(next) = next {
        *current = Some(current.map_or(next, |current| current.max(next)));
    }
}

fn merge_cumulative_counters(merged: &mut ProviderUsageSample, sample: &ProviderUsageSample) {
    merge_max(&mut merged.input_tokens, sample.input_tokens);
    merge_max(&mut merged.context_tokens, sample.context_tokens);
    merge_max(&mut merged.output_tokens, sample.output_tokens);
    merge_max(&mut merged.reasoning_tokens, sample.reasoning_tokens);
    merge_max(
        &mut merged.cache_read_input_tokens,
        sample.cache_read_input_tokens,
    );
    merge_max(
        &mut merged.cache_creation_input_tokens,
        sample.cache_creation_input_tokens,
    );
    merge_max(
        &mut merged.reported_total_tokens,
        sample.reported_total_tokens,
    );
}

fn add_delta(current: &mut Option<u64>, next: Option<u64>) -> bool {
    let Some(next) = next else {
        return true;
    };
    let Some(current) = current else {
        *current = Some(next);
        return true;
    };
    let Some(sum) = current.checked_add(next) else {
        return false;
    };
    *current = sum;
    true
}

fn add_delta_counters(merged: &mut ProviderUsageSample, sample: &ProviderUsageSample) -> bool {
    add_delta(&mut merged.input_tokens, sample.input_tokens)
        && add_delta(&mut merged.context_tokens, sample.context_tokens)
        && add_delta(&mut merged.output_tokens, sample.output_tokens)
        && add_delta(&mut merged.reasoning_tokens, sample.reasoning_tokens)
        && add_delta(
            &mut merged.cache_read_input_tokens,
            sample.cache_read_input_tokens,
        )
        && add_delta(
            &mut merged.cache_creation_input_tokens,
            sample.cache_creation_input_tokens,
        )
        && add_delta(
            &mut merged.reported_total_tokens,
            sample.reported_total_tokens,
        )
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
    /// Typed, optional-counter usage emitted by the built-in adapters.  The
    /// legacy `Usage` variant remains for compatibility with older providers;
    /// new consumers should use this bounded variant.
    UsageObserved {
        sample: ProviderUsageSample,
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

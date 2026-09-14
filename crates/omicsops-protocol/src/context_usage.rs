use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ContextArchiveV4;

/// Provenance for a model context limit.  Only an exact frozen catalog entry
/// is an exact provider window; locally configured and unavailable bounds stay
/// visibly distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextLimitSourceV4 {
    ExactCatalog {
        source_provider: String,
        source_sha256: String,
    },
    ConfiguredBound,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageAggregationV4 {
    /// The value is the provider's cumulative subtotal for this attempt.
    Cumulative,
    /// The value is a documented increment for this distinct sample.
    Delta,
    /// The provider did not identify the aggregation semantics.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageObservationStateV4 {
    Partial,
    Final,
    Interrupted,
}

/// The provider-facing portion of a usage callback.  It deliberately carries
/// no request or profile identity: the AgentCore invocation adds those IDs at
/// the immutable request boundary before persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelUsageSampleV4 {
    pub sample_index: u32,
    pub state: UsageObservationStateV4,
    pub aggregation: UsageAggregationV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Provider-normalized prompt/context occupancy. This remains distinct
    /// from billing input tokens when a provider exposes cache facets.
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

/// Immutable identity and frozen capability provenance for one provider
/// attempt.  It is persisted before dispatch so a missing usage callback is
/// distinguishable from a request that was never started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelRequestStartedV4 {
    pub logical_request_id: Uuid,
    pub attempt_id: Uuid,
    pub model_profile_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_configuration_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit_tokens: Option<u64>,
    pub context_limit_source: ContextLimitSourceV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serialized_request_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_bound_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<Vec<ContextUsageRowV4>>,
}

/// Host-owned, scoped projection for the context meter.  The projection keeps
/// provider usage, observed attempt totals, and the conservative admission
/// budget as separate quantities; the frontend must not reconstruct any of
/// those semantics from JSON byte lengths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextUsageSnapshotV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_configuration_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_request: Option<ModelUsageObservationV4>,
    pub observed_total: UsageTotalsV4,
    pub current_context: ContextWindowUsageV4,
    pub conservative_budget: ContextBudgetV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<Vec<ContextUsageRowV4>>,
    /// The latest durable manual-compaction outcome for this scope. It is
    /// optional so existing snapshots and older databases remain readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_compaction: Option<ContextCompactionReceiptV4>,
}

/// The bounded outcome of a manual context projection request. A successful
/// receipt describes the archive/checkpoint pair; it never replaces the
/// immutable transcript or evidence events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionStatusV4 {
    NotNeeded,
    Completed,
    Attention,
}

/// Scoped, idempotent host command for manual compaction. The host validates
/// the run's frozen specification and current lifecycle state before writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompactContextRequestV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
}

/// Durable, bounded result of a manual compaction attempt. The receipt keeps
/// identifiers and hashes only; the original transcript remains in the
/// existing context archive table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextCompactionReceiptV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub status: ContextCompactionStatusV4,
    pub source_through_sequence: u64,
    pub source_head_hash: String,
    pub before_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<ContextArchiveV4>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_through_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen_spec_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowUsageV4 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    pub limit_source: ContextLimitSourceV4,
    pub estimated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextBudgetV4 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serialized_request_bytes: Option<u64>,
    pub host_context_max_bytes: u64,
    /// Number of images included in the bounded request estimate, when the
    /// request boundary observed it. `None` means the durable usage events
    /// did not carry an image count; zero is reserved for an observed
    /// zero-image request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_bound_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fits_host_budget: Option<bool>,
}

/// Optional bounded breakdown row. Token counters remain absent when the host
/// only knows serialized byte sizes; `estimated` qualifies byte-only rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextUsageRowV4 {
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    pub estimated: bool,
}

/// A bounded, provider-independent usage observation.  Optional counters are
/// intentional: missing provider fields are unavailable, whereas `Some(0)`
/// is an explicitly reported zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelUsageObservationV4 {
    pub logical_request_id: Uuid,
    pub attempt_id: Uuid,
    pub sample_index: u32,
    pub model_profile_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_configuration_hash: Option<String>,
    pub state: UsageObservationStateV4,
    pub aggregation: UsageAggregationV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit_tokens: Option<u64>,
    pub context_limit_source: ContextLimitSourceV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serialized_request_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_bound_tokens: Option<u64>,
}

/// A counter in the observed total.  A known partial subtotal remains
/// qualified by `incomplete_attempts` so consumers cannot present it as an
/// exact complete total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ObservedCounterV4 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known: Option<u64>,
    #[serde(default)]
    pub incomplete_attempts: u32,
}

/// Aggregate observations across unique provider attempts.  A retry has a
/// distinct `attempt_id` and therefore contributes its own provider-reported
/// subtotal.  Repeated sample indices within an attempt are deduplicated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct UsageTotalsV4 {
    pub input_tokens: ObservedCounterV4,
    pub output_tokens: ObservedCounterV4,
    pub reasoning_tokens: ObservedCounterV4,
    pub cache_read_input_tokens: ObservedCounterV4,
    pub cache_creation_input_tokens: ObservedCounterV4,
    pub reported_total_tokens: ObservedCounterV4,
    pub observed_attempts: u32,
    pub final_attempts: u32,
    pub partial_attempts: u32,
    pub interrupted_attempts: u32,
    pub unknown_attempts: u32,
}

impl UsageTotalsV4 {
    pub fn from_observations(
        observations: impl IntoIterator<Item = ModelUsageObservationV4>,
    ) -> Self {
        let mut by_attempt: BTreeMap<(Uuid, Uuid), BTreeMap<u32, ModelUsageObservationV4>> =
            BTreeMap::new();
        for observation in observations {
            by_attempt
                .entry((observation.logical_request_id, observation.attempt_id))
                .or_default()
                .entry(observation.sample_index)
                .or_insert(observation);
        }

        let mut totals = Self::default();
        for samples in by_attempt.into_values() {
            totals.observed_attempts = totals.observed_attempts.saturating_add(1);
            let has_final = samples
                .values()
                .any(|sample| sample.state == UsageObservationStateV4::Final);
            let has_partial = samples
                .values()
                .any(|sample| sample.state == UsageObservationStateV4::Partial);
            let has_interrupted = samples
                .values()
                .any(|sample| sample.state == UsageObservationStateV4::Interrupted);
            if has_final {
                totals.final_attempts = totals.final_attempts.saturating_add(1);
            }
            if has_partial {
                totals.partial_attempts = totals.partial_attempts.saturating_add(1);
            }
            if has_interrupted {
                totals.interrupted_attempts = totals.interrupted_attempts.saturating_add(1);
            }

            let aggregate = aggregate_attempt(samples.values());
            if aggregate.unknown || aggregate.no_known_counter {
                totals.unknown_attempts = totals.unknown_attempts.saturating_add(1);
            }
            let incomplete = !has_final || aggregate.unknown;
            add_counter(&mut totals.input_tokens, aggregate.input_tokens, incomplete);
            add_counter(
                &mut totals.output_tokens,
                aggregate.output_tokens,
                incomplete,
            );
            add_counter(
                &mut totals.reasoning_tokens,
                aggregate.reasoning_tokens,
                incomplete,
            );
            add_counter(
                &mut totals.cache_read_input_tokens,
                aggregate.cache_read_input_tokens,
                incomplete,
            );
            add_counter(
                &mut totals.cache_creation_input_tokens,
                aggregate.cache_creation_input_tokens,
                incomplete,
            );
            add_counter(
                &mut totals.reported_total_tokens,
                aggregate.reported_total_tokens,
                incomplete,
            );
        }
        totals
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct AttemptAggregate {
    aggregation: Option<UsageAggregationV4>,
    unknown: bool,
    overflowed: bool,
    no_known_counter: bool,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    reported_total_tokens: Option<u64>,
}

impl AttemptAggregate {
    fn merge_sample(&mut self, sample: &ModelUsageObservationV4) {
        if self.unknown {
            return;
        }
        let Some(current_aggregation) = self.aggregation else {
            match sample.aggregation {
                UsageAggregationV4::Unknown => {
                    self.unknown = true;
                }
                aggregation => {
                    self.aggregation = Some(aggregation);
                    self.merge_counters(sample);
                }
            }
            return;
        };
        if sample.aggregation != current_aggregation {
            self.unknown = true;
            return;
        }
        self.merge_counters(sample);
    }

    fn merge_counters(&mut self, sample: &ModelUsageObservationV4) {
        match self.aggregation {
            Some(UsageAggregationV4::Cumulative) => {
                merge_max(&mut self.input_tokens, sample.input_tokens);
                merge_max(&mut self.output_tokens, sample.output_tokens);
                merge_max(&mut self.reasoning_tokens, sample.reasoning_tokens);
                merge_max(
                    &mut self.cache_read_input_tokens,
                    sample.cache_read_input_tokens,
                );
                merge_max(
                    &mut self.cache_creation_input_tokens,
                    sample.cache_creation_input_tokens,
                );
                merge_max(
                    &mut self.reported_total_tokens,
                    sample.reported_total_tokens,
                );
            }
            Some(UsageAggregationV4::Delta) => {
                self.input_tokens =
                    add_counter_value(self.input_tokens, sample.input_tokens, &mut self.overflowed);
                self.output_tokens = add_counter_value(
                    self.output_tokens,
                    sample.output_tokens,
                    &mut self.overflowed,
                );
                self.reasoning_tokens = add_counter_value(
                    self.reasoning_tokens,
                    sample.reasoning_tokens,
                    &mut self.overflowed,
                );
                self.cache_read_input_tokens = add_counter_value(
                    self.cache_read_input_tokens,
                    sample.cache_read_input_tokens,
                    &mut self.overflowed,
                );
                self.cache_creation_input_tokens = add_counter_value(
                    self.cache_creation_input_tokens,
                    sample.cache_creation_input_tokens,
                    &mut self.overflowed,
                );
                self.reported_total_tokens = add_counter_value(
                    self.reported_total_tokens,
                    sample.reported_total_tokens,
                    &mut self.overflowed,
                );
            }
            Some(UsageAggregationV4::Unknown) | None => {}
        }
    }

    fn finish(mut self) -> Self {
        self.unknown |= self.overflowed;
        self.no_known_counter = self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.reasoning_tokens.is_none()
            && self.cache_read_input_tokens.is_none()
            && self.cache_creation_input_tokens.is_none()
            && self.reported_total_tokens.is_none();
        self
    }
}

fn aggregate_attempt<'a>(
    samples: impl IntoIterator<Item = &'a ModelUsageObservationV4>,
) -> AttemptAggregate {
    let mut aggregate = AttemptAggregate::default();
    for sample in samples {
        aggregate.merge_sample(sample);
    }
    aggregate.finish()
}

fn merge_max(current: &mut Option<u64>, next: Option<u64>) {
    if let Some(next) = next {
        *current = Some(current.map_or(next, |current| current.max(next)));
    }
}

fn add_counter_value(
    current: Option<u64>,
    next: Option<u64>,
    overflowed: &mut bool,
) -> Option<u64> {
    match (current, next) {
        (Some(current), Some(next)) => match current.checked_add(next) {
            Some(total) => Some(total),
            None => {
                *overflowed = true;
                None
            }
        },
        (None, Some(next)) => Some(next),
        (current, None) => current,
    }
}

fn add_counter(target: &mut ObservedCounterV4, value: Option<u64>, incomplete: bool) {
    let overflow = matches!((target.known, value), (Some(current), Some(next)) if current.checked_add(next).is_none());
    if incomplete || value.is_none() || overflow {
        target.incomplete_attempts = target.incomplete_attempts.saturating_add(1);
    }
    if let Some(value) = value {
        target.known = Some(target.known.map_or(value, |current| {
            current.checked_add(value).unwrap_or(current)
        }));
    }
}

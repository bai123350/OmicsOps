use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ContextBudgetV4, ContextLimitSourceV4, ContextUsageRowV4,
    ContextUsageSnapshotV4, ContextWindowUsageV4, ModelRequestStartedV4, ModelUsageObservationV4,
    ModelUsageSampleV4, ObservedCounterV4, UsageAggregationV4, UsageObservationStateV4,
    UsageTotalsV4,
};
use serde_json::json;
use uuid::Uuid;

fn observation(
    logical_request_id: Uuid,
    attempt_id: Uuid,
    sample_index: u32,
    state: UsageObservationStateV4,
    aggregation: UsageAggregationV4,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
) -> ModelUsageObservationV4 {
    ModelUsageObservationV4 {
        logical_request_id,
        attempt_id,
        sample_index,
        model_profile_id: Uuid::new_v4(),
        model_configuration_hash: None,
        state,
        aggregation,
        input_tokens,
        output_tokens,
        reasoning_tokens: None,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        reported_total_tokens: None,
        context_tokens: None,
        context_limit_tokens: None,
        context_limit_source: ContextLimitSourceV4::Unknown,
        serialized_request_bytes: None,
        image_bound_tokens: None,
    }
}

#[test]
fn optional_counters_round_trip_missing_separately_from_explicit_zero() {
    let logical = Uuid::new_v4();
    let mut value = observation(
        logical,
        Uuid::new_v4(),
        0,
        UsageObservationStateV4::Final,
        UsageAggregationV4::Cumulative,
        Some(0),
        None,
    );
    value.context_limit_source = ContextLimitSourceV4::ExactCatalog {
        source_provider: "openai_compatible".into(),
        source_sha256: "catalog-sha".into(),
    };
    let encoded = serde_json::to_value(&value).unwrap();
    assert_eq!(encoded["input_tokens"], json!(0));
    assert!(encoded.get("output_tokens").is_none());
    assert_eq!(
        encoded["context_limit_source"],
        json!({"kind":"exact_catalog","source_provider":"openai_compatible","source_sha256":"catalog-sha"})
    );
    let decoded: ModelUsageObservationV4 = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.input_tokens, Some(0));
    assert_eq!(decoded.output_tokens, None);
}

#[test]
fn overflow_and_missing_final_facets_cannot_look_like_exact_totals() {
    let logical = Uuid::new_v4();
    let samples = [Some(u64::MAX), Some(1), None]
        .into_iter()
        .enumerate()
        .map(|(index, input)| {
            observation(
                logical,
                Uuid::from_u128(index as u128 + 1),
                0,
                UsageObservationStateV4::Final,
                UsageAggregationV4::Cumulative,
                input,
                Some(0),
            )
        });
    let totals = UsageTotalsV4::from_observations(samples);
    assert_eq!(totals.input_tokens.known, Some(u64::MAX));
    assert_eq!(totals.input_tokens.incomplete_attempts, 2);
    assert_eq!(totals.output_tokens.known, Some(0));
    assert_eq!(totals.output_tokens.incomplete_attempts, 0);
}

#[test]
fn every_limit_source_variant_has_an_explicit_wire_shape() {
    for source in [
        ContextLimitSourceV4::ExactCatalog {
            source_provider: "provider".into(),
            source_sha256: "hash".into(),
        },
        ContextLimitSourceV4::ConfiguredBound,
        ContextLimitSourceV4::Unknown,
    ] {
        let encoded = serde_json::to_value(source).unwrap();
        assert!(
            encoded
                .get("kind")
                .and_then(|value| value.as_str())
                .is_some()
        );
    }
}

#[test]
fn aggregation_deduplicates_samples_merges_cumulative_retries_and_keeps_partial_qualification() {
    let logical = Uuid::new_v4();
    let first_attempt = Uuid::new_v4();
    let retry_attempt = Uuid::new_v4();
    let samples = vec![
        observation(
            logical,
            first_attempt,
            0,
            UsageObservationStateV4::Partial,
            UsageAggregationV4::Cumulative,
            Some(10),
            Some(1),
        ),
        observation(
            logical,
            first_attempt,
            1,
            UsageObservationStateV4::Partial,
            UsageAggregationV4::Cumulative,
            None,
            Some(7),
        ),
        observation(
            logical,
            first_attempt,
            1,
            UsageObservationStateV4::Partial,
            UsageAggregationV4::Cumulative,
            None,
            Some(700),
        ),
        observation(
            logical,
            retry_attempt,
            0,
            UsageObservationStateV4::Final,
            UsageAggregationV4::Cumulative,
            Some(3),
            Some(2),
        ),
    ];
    let totals = UsageTotalsV4::from_observations(samples);
    assert_eq!(
        totals.input_tokens,
        ObservedCounterV4 {
            known: Some(13),
            incomplete_attempts: 1,
        }
    );
    assert_eq!(totals.output_tokens.known, Some(9));
    assert_eq!(totals.output_tokens.incomplete_attempts, 1);
    assert_eq!(totals.observed_attempts, 2);
    assert_eq!(totals.final_attempts, 1);
    assert_eq!(totals.partial_attempts, 1);
    assert_eq!(totals.interrupted_attempts, 0);
    assert_eq!(totals.unknown_attempts, 0);
}

#[test]
fn unknown_aggregation_does_not_add_unknown_sample_values() {
    let logical = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let totals = UsageTotalsV4::from_observations([
        observation(
            logical,
            attempt,
            0,
            UsageObservationStateV4::Partial,
            UsageAggregationV4::Cumulative,
            Some(10),
            Some(7),
        ),
        observation(
            logical,
            attempt,
            1,
            UsageObservationStateV4::Partial,
            UsageAggregationV4::Unknown,
            Some(99),
            Some(99),
        ),
    ]);
    assert_eq!(totals.input_tokens.known, Some(10));
    assert_eq!(totals.output_tokens.known, Some(7));
    assert_eq!(totals.unknown_attempts, 1);
}

#[test]
fn documented_delta_samples_add_only_distinct_sample_indices() {
    let logical = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let totals = UsageTotalsV4::from_observations([
        observation(
            logical,
            attempt,
            0,
            UsageObservationStateV4::Partial,
            UsageAggregationV4::Delta,
            Some(2),
            Some(1),
        ),
        observation(
            logical,
            attempt,
            1,
            UsageObservationStateV4::Final,
            UsageAggregationV4::Delta,
            Some(3),
            Some(6),
        ),
        observation(
            logical,
            attempt,
            1,
            UsageObservationStateV4::Final,
            UsageAggregationV4::Delta,
            Some(30),
            Some(60),
        ),
    ]);
    assert_eq!(totals.input_tokens.known, Some(5));
    assert_eq!(totals.output_tokens.known, Some(7));
    assert_eq!(totals.final_attempts, 1);
    assert_eq!(totals.unknown_attempts, 0);
}

#[test]
fn delta_overflow_is_unavailable_and_qualified_as_incomplete() {
    let logical = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let totals = UsageTotalsV4::from_observations([
        observation(
            logical,
            attempt,
            0,
            UsageObservationStateV4::Partial,
            UsageAggregationV4::Delta,
            Some(u64::MAX),
            None,
        ),
        observation(
            logical,
            attempt,
            1,
            UsageObservationStateV4::Final,
            UsageAggregationV4::Delta,
            Some(1),
            None,
        ),
    ]);
    assert_eq!(totals.input_tokens.known, None);
    assert_eq!(totals.input_tokens.incomplete_attempts, 1);
    assert_eq!(totals.unknown_attempts, 1);
}

#[test]
fn observation_rejects_unknown_serialized_fields() {
    let value = json!({
        "logical_request_id": Uuid::new_v4(),
        "attempt_id": Uuid::new_v4(),
        "sample_index": 0,
        "model_profile_id": Uuid::new_v4(),
        "state": "final",
        "aggregation": "cumulative",
        "context_limit_source": "unknown",
        "raw_provider_response": "secret"
    });
    assert!(serde_json::from_value::<ModelUsageObservationV4>(value).is_err());
}

#[test]
fn stream_usage_sample_and_request_started_events_round_trip_without_raw_provider_data() {
    let run_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let logical_request_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let request = ModelRequestStartedV4 {
        logical_request_id,
        attempt_id,
        model_profile_id: Uuid::new_v4(),
        model_configuration_hash: Some("configuration-hash".into()),
        context_limit_tokens: None,
        context_limit_source: ContextLimitSourceV4::Unknown,
        serialized_request_bytes: None,
        image_count: None,
        image_bound_tokens: None,
        breakdown: None,
    };
    let started = AgentEventV4::first(
        run_id,
        project_id,
        Uuid::new_v4(),
        chrono::Utc::now(),
        AgentEventKindV4::ModelRequestStarted { request },
    );
    let observed = AgentEventV4::next(
        &started,
        chrono::Utc::now(),
        AgentEventKindV4::ModelUsageObserved {
            observation: ModelUsageObservationV4 {
                logical_request_id,
                attempt_id,
                sample_index: 0,
                model_profile_id: Uuid::new_v4(),
                model_configuration_hash: None,
                state: UsageObservationStateV4::Final,
                aggregation: UsageAggregationV4::Cumulative,
                input_tokens: Some(0),
                output_tokens: Some(7),
                reasoning_tokens: None,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                reported_total_tokens: None,
                context_tokens: None,
                context_limit_tokens: None,
                context_limit_source: ContextLimitSourceV4::Unknown,
                serialized_request_bytes: None,
                image_bound_tokens: None,
            },
        },
    );
    let encoded = serde_json::to_value(&vec![started, observed]).unwrap();
    let decoded: Vec<AgentEventV4> = serde_json::from_value(encoded).unwrap();
    assert!(matches!(
        decoded[0].event,
        AgentEventKindV4::ModelRequestStarted { .. }
    ));
    assert!(matches!(
        decoded[1].event,
        AgentEventKindV4::ModelUsageObserved { .. }
    ));

    let sample = ModelUsageSampleV4 {
        sample_index: 2,
        state: UsageObservationStateV4::Interrupted,
        aggregation: UsageAggregationV4::Cumulative,
        input_tokens: None,
        context_tokens: None,
        output_tokens: Some(0),
        reasoning_tokens: None,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        reported_total_tokens: None,
    };
    let sample_value = serde_json::to_value(sample).unwrap();
    assert_eq!(sample_value["output_tokens"], json!(0));
    assert!(sample_value.get("input_tokens").is_none());
    let decoded_sample: ModelUsageSampleV4 = serde_json::from_value(sample_value).unwrap();
    assert_eq!(decoded_sample, sample);
}

#[test]
fn context_snapshot_keeps_usage_budget_and_byte_only_breakdown_separate() {
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let snapshot = ContextUsageSnapshotV4 {
        project_id,
        conversation_id,
        run_id: None,
        model_profile_id: None,
        model_configuration_hash: None,
        last_request: None,
        observed_total: UsageTotalsV4::default(),
        current_context: ContextWindowUsageV4 {
            used_tokens: None,
            max_tokens: None,
            limit_source: ContextLimitSourceV4::Unknown,
            estimated: true,
        },
        conservative_budget: ContextBudgetV4 {
            serialized_request_bytes: Some(1024),
            host_context_max_bytes: 16 * 1024,
            image_count: Some(1),
            image_bound_tokens: None,
            fits_host_budget: Some(true),
        },
        breakdown: Some(vec![ContextUsageRowV4 {
            category: "tools".into(),
            bytes: Some(1024),
            tokens: None,
            estimated: true,
        }]),
        latest_compaction: None,
    };
    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(
        encoded["conservative_budget"]["serialized_request_bytes"],
        json!(1024)
    );
    assert!(encoded["current_context"].get("used_tokens").is_none());
    assert!(encoded["breakdown"][0].get("tokens").is_none());
    let decoded: ContextUsageSnapshotV4 = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, snapshot);
}

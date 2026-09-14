use omicsops_agent::provider::{
    ProviderUsageAggregation, ProviderUsageSample, ProviderUsageState,
    coalesce_provider_usage_samples,
};

fn sample(
    sample_index: u32,
    aggregation: ProviderUsageAggregation,
    state: ProviderUsageState,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
) -> ProviderUsageSample {
    ProviderUsageSample {
        sample_index,
        aggregation,
        state,
        input_tokens,
        context_tokens: None,
        output_tokens,
        reasoning_tokens: None,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        reported_total_tokens: None,
    }
}

#[test]
fn cumulative_samples_merge_by_presence_and_max_without_fabricating_zero() {
    let merged = coalesce_provider_usage_samples([
        sample(
            0,
            ProviderUsageAggregation::Cumulative,
            ProviderUsageState::Partial,
            Some(10),
            Some(1),
        ),
        sample(
            1,
            ProviderUsageAggregation::Cumulative,
            ProviderUsageState::Final,
            None,
            Some(7),
        ),
    ])
    .expect("at least one usage sample");

    assert_eq!(merged.input_tokens, Some(10));
    assert_eq!(merged.output_tokens, Some(7));
    assert_eq!(merged.state, ProviderUsageState::Final);
    assert_eq!(merged.sample_index, 1);
    assert_eq!(merged.reasoning_tokens, None);
}

#[test]
fn explicit_zero_is_present_and_duplicate_sample_identity_is_ignored() {
    let mut zero = sample(
        0,
        ProviderUsageAggregation::Cumulative,
        ProviderUsageState::Final,
        Some(0),
        None,
    );
    zero.reported_total_tokens = Some(0);
    let mut later = zero.clone();
    later.sample_index = 1;
    later.input_tokens = Some(4);
    later.reported_total_tokens = Some(4);

    let duplicate = later.clone();
    let merged = coalesce_provider_usage_samples([zero, later, duplicate]).unwrap();

    assert_eq!(merged.input_tokens, Some(4));
    assert_eq!(merged.reported_total_tokens, Some(4));
}

#[test]
fn unknown_aggregation_is_retained_as_ambiguous_without_being_summed() {
    let mut unknown = sample(
        1,
        ProviderUsageAggregation::Unknown,
        ProviderUsageState::Partial,
        Some(99),
        Some(99),
    );
    unknown.reasoning_tokens = Some(12);
    let merged = coalesce_provider_usage_samples([
        sample(
            0,
            ProviderUsageAggregation::Cumulative,
            ProviderUsageState::Partial,
            Some(10),
            Some(7),
        ),
        unknown,
    ])
    .unwrap();

    assert_eq!(merged.aggregation, ProviderUsageAggregation::Unknown);
    assert_eq!(merged.input_tokens, Some(10));
    assert_eq!(merged.output_tokens, Some(7));
    assert_eq!(merged.reasoning_tokens, None);
}

#[test]
fn unknown_first_sample_does_not_expose_its_unverified_counters() {
    let mut unknown = sample(
        0,
        ProviderUsageAggregation::Unknown,
        ProviderUsageState::Partial,
        Some(99),
        Some(99),
    );
    unknown.cache_read_input_tokens = Some(8);
    let merged = coalesce_provider_usage_samples([unknown]).unwrap();

    assert_eq!(merged.aggregation, ProviderUsageAggregation::Unknown);
    assert_eq!(merged.input_tokens, None);
    assert_eq!(merged.output_tokens, None);
    assert_eq!(merged.cache_read_input_tokens, None);
}

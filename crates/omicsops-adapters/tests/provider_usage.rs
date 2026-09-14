use omicsops_adapters::llm::{
    ProviderProtocol, ProviderToolStreamDecoder, parse_provider_tool_response,
};
use omicsops_agent::provider::{
    ProviderStreamEvent, ProviderUsageAggregation, ProviderUsageState,
    coalesce_provider_usage_samples,
};
use serde_json::json;

fn usage_events(
    events: &[ProviderStreamEvent],
) -> Vec<omicsops_agent::provider::ProviderUsageSample> {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderStreamEvent::UsageObserved { sample } => Some(sample.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn anthropic_partial_usage_is_typed_cumulative_and_cache_facets_stay_separate() {
    let wire = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":1,\"cache_read_input_tokens\":4}}}\n\n",
        "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7,\"cache_creation_input_tokens\":2,\"private\":\"sentinel\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::Anthropic);
    let events = decoder.push(wire.as_bytes()).unwrap();
    let samples = usage_events(&events);
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].aggregation, ProviderUsageAggregation::Cumulative);
    assert_eq!(samples[0].state, ProviderUsageState::Partial);
    assert_eq!(samples[0].input_tokens, Some(10));
    assert_eq!(samples[0].context_tokens, None);
    assert_eq!(samples[0].output_tokens, Some(1));
    assert_eq!(samples[0].cache_read_input_tokens, Some(4));
    assert_eq!(samples[1].output_tokens, Some(7));
    assert_eq!(samples[1].cache_creation_input_tokens, Some(2));
    assert_eq!(samples[1].context_tokens, None);
    assert_eq!(samples[1].reasoning_tokens, None);
    assert!(
        !serde_json::to_string(&samples[1])
            .unwrap()
            .contains("sentinel")
    );

    let merged = coalesce_provider_usage_samples(samples).unwrap();
    assert_eq!(merged.input_tokens, Some(10));
    assert_eq!(merged.output_tokens, Some(7));
    assert_eq!(merged.cache_read_input_tokens, Some(4));
    assert_eq!(merged.cache_creation_input_tokens, Some(2));
    assert_eq!(merged.context_tokens, None);
}

#[test]
fn anthropic_context_tokens_require_all_explicit_input_facets() {
    let response = json!({
        "type": "message_delta",
        "usage": {
            "input_tokens": 10,
            "cache_read_input_tokens": 4,
            "cache_creation_input_tokens": 2,
            "output_tokens": 7
        }
    });
    let events = parse_provider_tool_response(ProviderProtocol::Anthropic, &response).unwrap();
    let samples = usage_events(&events);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].input_tokens, Some(10));
    assert_eq!(samples[0].cache_read_input_tokens, Some(4));
    assert_eq!(samples[0].cache_creation_input_tokens, Some(2));
    assert_eq!(samples[0].context_tokens, Some(16));

    let overflow = json!({
        "type": "message_delta",
        "usage": {
            "input_tokens": u64::MAX,
            "cache_read_input_tokens": 1,
            "cache_creation_input_tokens": 0
        }
    });
    let overflow_events =
        parse_provider_tool_response(ProviderProtocol::Anthropic, &overflow).unwrap();
    let overflow_samples = usage_events(&overflow_events);
    assert_eq!(overflow_samples.len(), 1);
    assert_eq!(overflow_samples[0].input_tokens, Some(u64::MAX));
    assert_eq!(overflow_samples[0].context_tokens, None);
}

#[test]
fn openai_explicit_zero_is_not_collapsed_into_missing_and_unknown_fields_are_dropped() {
    let response = json!({
        "choices": [{"message": {"content": "answer"}}],
        "usage": {
            "prompt_tokens": 0,
            "total_tokens": 0,
            "private": 44,
            "completion_tokens_details": {"reasoning_tokens": 0, "private": "secret"}
        }
    });
    let events =
        parse_provider_tool_response(ProviderProtocol::OpenAiCompatible, &response).unwrap();
    let samples = usage_events(&events);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].state, ProviderUsageState::Final);
    assert_eq!(samples[0].input_tokens, Some(0));
    assert_eq!(samples[0].context_tokens, Some(0));
    assert_eq!(samples[0].output_tokens, None);
    assert_eq!(samples[0].reported_total_tokens, Some(0));
    assert_eq!(samples[0].reasoning_tokens, Some(0));
    assert_eq!(samples[0].context_tokens, Some(0));
    assert!(
        !serde_json::to_string(&samples[0])
            .unwrap()
            .contains("private")
    );
}

#[test]
fn openai_usage_after_finish_marker_is_final_even_when_batched_in_one_chunk() {
    let wire = concat!(
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
    let events = decoder.push(wire.as_bytes()).unwrap();
    let samples = usage_events(&events);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].state, ProviderUsageState::Final);
    assert_eq!(samples[0].input_tokens, Some(2));
    assert_eq!(samples[0].output_tokens, Some(3));
}

#[test]
fn missing_usage_object_stays_unavailable_instead_of_becoming_zero() {
    let response = json!({"choices": [{"message": {"content": "answer"}}]});
    let events =
        parse_provider_tool_response(ProviderProtocol::OpenAiCompatible, &response).unwrap();
    assert!(usage_events(&events).is_empty());
}

#[test]
fn ollama_final_usage_keeps_explicit_zero_and_missing_fields_distinct() {
    let response = json!({
        "message": {"content": "answer"},
        "done": true,
        "prompt_eval_count": 0,
        "eval_count": 4,
        "private": "sentinel"
    });
    let events = parse_provider_tool_response(ProviderProtocol::Ollama, &response).unwrap();
    let samples = usage_events(&events);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].state, ProviderUsageState::Final);
    assert_eq!(samples[0].input_tokens, Some(0));
    assert_eq!(samples[0].context_tokens, Some(0));
    assert_eq!(samples[0].output_tokens, Some(4));
    assert_eq!(samples[0].reasoning_tokens, None);
    assert!(
        !serde_json::to_string(&samples[0])
            .unwrap()
            .contains("sentinel")
    );
}

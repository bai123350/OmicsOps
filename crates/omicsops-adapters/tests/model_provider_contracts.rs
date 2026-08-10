use omicsops_adapters::llm::{
    ProviderProtocol, ProviderStreamDecoder, UnifiedModelClient, build_provider_request,
    parse_provider_event,
};
use omicsops_agent::{ModelRequest, ModelStreamEvent};
use serde_json::json;
use url::Url;
use uuid::Uuid;

fn request() -> ModelRequest {
    ModelRequest {
        system: "You are a scientific assistant.".into(),
        messages: vec![omicsops_agent::ModelMessage {
            role: "user".into(),
            content: "Inspect PBMC quality control".into(),
        }],
        tool_name: Some("submit_plan".into()),
        tool_schema: Some(json!({"type": "object"})),
    }
}

#[test]
fn provider_protocols_map_to_native_desktop_endpoints() {
    let openai = build_provider_request(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://models.example/").unwrap(),
        "gpt-science",
        &request(),
    )
    .unwrap();
    assert_eq!(
        openai.endpoint.as_str(),
        "https://models.example/v1/chat/completions"
    );
    assert!(openai.requires_credential);
    assert_eq!(openai.body["stream"], true);
    assert_eq!(openai.body["tool_choice"]["function"]["name"], "submit_plan");

    let anthropic = build_provider_request(
        ProviderProtocol::Anthropic,
        Url::parse("https://api.anthropic.com/").unwrap(),
        "claude-science",
        &request(),
    )
    .unwrap();
    assert_eq!(
        anthropic.endpoint.as_str(),
        "https://api.anthropic.com/v1/messages"
    );
    assert_eq!(anthropic.body["tools"][0]["name"], "submit_plan");
    assert_eq!(anthropic.body["tool_choice"]["name"], "submit_plan");

    let ollama = build_provider_request(
        ProviderProtocol::Ollama,
        Url::parse("http://127.0.0.1:11434/").unwrap(),
        "qwen3",
        &request(),
    )
    .unwrap();
    assert_eq!(ollama.endpoint.as_str(), "http://127.0.0.1:11434/api/chat");
    assert!(!ollama.requires_credential);
}

#[test]
fn provider_stream_fragments_normalize_to_one_event_contract() {
    assert_eq!(
        parse_provider_event(
            ProviderProtocol::OpenAiCompatible,
            &json!({"choices":[{"delta":{"content":"QC"}}]})
        ),
        Some(ModelStreamEvent::TextDelta("QC".into()))
    );
    assert_eq!(
        parse_provider_event(
            ProviderProtocol::Anthropic,
            &json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"完成"}})
        ),
        Some(ModelStreamEvent::TextDelta("完成".into()))
    );
    assert_eq!(
        parse_provider_event(
            ProviderProtocol::Ollama,
            &json!({"message":{"content":"ready"},"done":false})
        ),
        Some(ModelStreamEvent::TextDelta("ready".into()))
    );
}

#[test]
fn streaming_decoder_survives_network_chunk_boundaries() {
    let mut decoder = ProviderStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
    assert!(
        decoder
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"Q")
            .unwrap()
            .is_empty()
    );
    let events = decoder.push(b"C\"}}]}\n\ndata: [DONE]\n\n").unwrap();
    assert_eq!(
        events,
        vec![
            ModelStreamEvent::TextDelta("QC".into()),
            ModelStreamEvent::Completed
        ]
    );

    let mut ollama = ProviderStreamDecoder::new(ProviderProtocol::Ollama);
    let events = ollama
        .push(b"{\"message\":{\"content\":\"ready\"},\"done\":false}\n{\"done\":true}\n")
        .unwrap();
    assert_eq!(
        events,
        vec![
            ModelStreamEvent::TextDelta("ready".into()),
            ModelStreamEvent::Completed
        ]
    );
}

#[test]
fn tool_argument_deltas_are_normalized_for_openai_and_anthropic() {
    assert_eq!(
        parse_provider_event(
            ProviderProtocol::OpenAiCompatible,
            &json!({"choices":[{"delta":{"tool_calls":[{"function":{"name":"submit_plan","arguments":"{\"title\":"}}]}}]})
        ),
        Some(ModelStreamEvent::ToolArgumentsDelta {
            name: "submit_plan".into(),
            json_fragment: "{\"title\":".into()
        })
    );
    let mut anthropic = ProviderStreamDecoder::new(ProviderProtocol::Anthropic);
    let events = anthropic.push(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"tool_use\",\"name\":\"submit_plan\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n").unwrap();
    assert_eq!(
        events,
        vec![ModelStreamEvent::ToolArgumentsDelta {
            name: "submit_plan".into(),
            json_fragment: "{}".into()
        }]
    );
}

#[test]
fn remote_providers_require_a_credential_but_ollama_does_not() {
    let base = Url::parse("https://models.example/").unwrap();
    assert!(
        UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::Anthropic,
            base.clone(),
            "claude",
            None
        )
        .is_err()
    );
    assert!(
        UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::OpenAiCompatible,
            base,
            "model",
            None
        )
        .is_err()
    );
    assert!(
        UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::Ollama,
            Url::parse("http://127.0.0.1:11434/").unwrap(),
            "qwen3",
            None
        )
        .is_ok()
    );
}

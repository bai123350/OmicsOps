use omicsops_adapters::llm::{ProviderProtocol, build_provider_request, parse_provider_event};
use omicsops_agent::{ModelRequest, ModelStreamEvent};
use serde_json::json;
use url::Url;

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

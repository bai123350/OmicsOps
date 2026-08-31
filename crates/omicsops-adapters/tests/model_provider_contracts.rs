use omicsops_adapters::llm::{
    ProviderProtocol, ProviderToolStreamDecoder, UnifiedModelClient,
    build_provider_request_with_tools, parse_provider_tool_response_for_request,
};
use omicsops_agent::{
    ModelContentPart, ModelMessage, ModelMessageContent,
    provider::{Provider, ProviderRequest, ProviderStreamEvent, ProviderToolSpec},
};
use serde_json::json;
use url::Url;

fn request() -> ProviderRequest {
    ProviderRequest {
        system: "Use whichever tools are needed".into(),
        messages: vec![ModelMessage {
            role: "user".into(),
            content: "inspect the project".into(),
        }],
        tools: vec![
            ProviderToolSpec {
                id: "remote.list".into(),
                description: "List project files".into(),
                input_schema: json!({"type":"object"}),
            },
            ProviderToolSpec {
                id: "remote.read".into(),
                description: "Read one file".into(),
                input_schema: json!({"type":"object","required":["path"]}),
            },
        ],
        require_strict_json_fallback: true,
    }
}

#[test]
fn multimodal_messages_are_serialized_for_each_provider_protocol() {
    let mut request = request();
    request.messages = vec![ModelMessage {
        role: "user".into(),
        content: ModelMessageContent::Parts(vec![
            ModelContentPart::Text {
                text: "inspect screenshot".into(),
            },
            ModelContentPart::Image {
                media_type: "image/png".into(),
                data_base64: "aW1hZ2U=".into(),
            },
        ]),
    }];
    let openai = build_provider_request_with_tools(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://example.test").unwrap(),
        "vision-exact-id",
        &request,
    )
    .unwrap();
    assert_eq!(
        openai.body["messages"][1]["content"][1]["type"],
        "image_url"
    );

    let anthropic = build_provider_request_with_tools(
        ProviderProtocol::Anthropic,
        Url::parse("https://example.test").unwrap(),
        "vision-exact-id",
        &request,
    )
    .unwrap();
    assert_eq!(anthropic.body["messages"][0]["content"][1]["type"], "image");

    let ollama = build_provider_request_with_tools(
        ProviderProtocol::Ollama,
        Url::parse("http://localhost:11434").unwrap(),
        "vision-exact-id",
        &request,
    )
    .unwrap();
    assert_eq!(ollama.body["messages"][1]["images"][0], "aW1hZ2U=");
}

#[test]
fn unified_client_implements_the_provider_boundary() {
    fn assert_provider<T: Provider>() {}
    assert_provider::<UnifiedModelClient>();
}

#[test]
fn every_provider_request_contains_all_tools_without_forcing_one() {
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Anthropic,
        ProviderProtocol::Ollama,
    ] {
        let built = build_provider_request_with_tools(
            protocol,
            Url::parse("http://localhost:11434").unwrap(),
            "model",
            &request(),
        )
        .unwrap();
        assert_eq!(built.body["tools"].as_array().unwrap().len(), 2);
        assert!(built.body.get("tool_choice").is_none());
    }
}

#[test]
fn provider_aliases_round_trip_to_canonical_tool_ids() {
    let request = request();
    let built = build_provider_request_with_tools(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://example.test/v1").unwrap(),
        "model",
        &request,
    )
    .unwrap();
    let alias = built.body["tools"][0]["function"]["name"].as_str().unwrap();
    assert_ne!(alias, "remote.list");

    let mut decoder =
        ProviderToolStreamDecoder::for_request(ProviderProtocol::OpenAiCompatible, &request);
    let payload = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call-a\",\"function\":{{\"name\":\"{alias}\",\"arguments\":\"{{}}\"}}}}]}}}}]}}\n\ndata: [DONE]\n\n"
    );
    let events = decoder.push(payload.as_bytes()).unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallStarted { tool_id, .. } if tool_id == "remote.list"
    )));

    let events = parse_provider_tool_response_for_request(
        ProviderProtocol::OpenAiCompatible,
        &json!({"choices":[{"message":{"tool_calls":[{"id":"call-a","function":{"name":alias,"arguments":"{}"}}]}}]}),
        &request,
    )
    .unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallStarted { tool_id, .. } if tool_id == "remote.list"
    )));
}

#[test]
fn streaming_decoder_preserves_utf8_split_across_network_chunks() {
    let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
    let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"我会检索肝癌文献。\"}}]}\n\n";
    let bytes = payload.as_bytes();
    let chinese = payload.find('肝').unwrap();
    let split = chinese + 1;

    assert!(decoder.push(&bytes[..split]).unwrap().is_empty());
    let events = decoder.push(&bytes[split..]).unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::TextDelta { text } if text == "我会检索肝癌文献。"
    )));
}

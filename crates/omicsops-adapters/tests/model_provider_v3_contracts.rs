use omicsops_adapters::llm::{
    ProviderProtocol, ProviderStreamDecoderV2, UnifiedModelClient, build_provider_request_v2,
    parse_provider_response_v2, parse_provider_response_v2_for_request,
};
use omicsops_agent::{
    ModelMessage,
    harness_v3::{ModelProviderV2, ModelRequestV2, ModelStreamEventV2, ModelToolSpec},
};
use omicsops_core::workspace::ModelProfile;
use serde_json::json;
use url::Url;

fn request() -> ModelRequestV2 {
    ModelRequestV2 {
        system: "Use whichever tools are needed".into(),
        messages: vec![ModelMessage {
            role: "user".into(),
            content: "inspect the project".into(),
        }],
        tools: vec![
            ModelToolSpec {
                id: "remote.list".into(),
                description: "List project files".into(),
                input_schema: json!({"type":"object"}),
            },
            ModelToolSpec {
                id: "remote.read".into(),
                description: "Read one file".into(),
                input_schema: json!({"type":"object","required":["path"]}),
            },
        ],
        require_strict_json_fallback: true,
    }
}

#[test]
fn unified_client_implements_the_v3_model_provider_boundary() {
    fn assert_provider<T: ModelProviderV2>() {}
    assert_provider::<UnifiedModelClient>();
}

#[test]
fn every_provider_request_contains_all_tools_without_forcing_one() {
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Anthropic,
        ProviderProtocol::Ollama,
    ] {
        let built = build_provider_request_v2(
            protocol,
            Url::parse("http://localhost:11434").unwrap(),
            "model",
            &request(),
        )
        .unwrap();
        assert_eq!(built.body["tools"].as_array().unwrap().len(), 2);
        assert!(built.body.get("tool_choice").is_none());
        for tool in built.body["tools"].as_array().unwrap() {
            let name = if protocol == ProviderProtocol::Anthropic {
                tool["name"].as_str().unwrap()
            } else {
                tool["function"]["name"].as_str().unwrap()
            };
            assert!(name.chars().all(
                |character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            ));
            assert!(name.len() <= 64);
            assert!(!name.contains('.'));
        }
    }
}

#[test]
fn provider_safe_aliases_round_trip_to_canonical_harness_tool_ids() {
    let request = request();
    let built = build_provider_request_v2(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://example.test/v1").unwrap(),
        "model",
        &request,
    )
    .unwrap();
    let alias = built.body["tools"][0]["function"]["name"].as_str().unwrap();
    assert_ne!(alias, "remote.list");

    let mut decoder =
        ProviderStreamDecoderV2::for_request(ProviderProtocol::OpenAiCompatible, &request);
    let payload = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call-a\",\"function\":{{\"name\":\"{alias}\",\"arguments\":\"{{}}\"}}}}]}}}}]}}\n\ndata: [DONE]\n\n"
    );
    let events = decoder.push(payload.as_bytes()).unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::ToolCallStarted { tool_id, .. } if tool_id == "remote.list"
    )));

    let events = parse_provider_response_v2_for_request(
        ProviderProtocol::OpenAiCompatible,
        &json!({"choices":[{"message":{"tool_calls":[{"id":"call-a","function":{"name":alias,"arguments":"{}"}}]}}]}),
        &request,
    )
    .unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::ToolCallStarted { tool_id, .. } if tool_id == "remote.list"
    )));
}

#[test]
fn openai_decoder_keeps_interleaved_call_ids_indexes_arguments_and_usage() {
    let mut decoder = ProviderStreamDecoderV2::new(ProviderProtocol::OpenAiCompatible);
    let first = json!({
        "choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call-a","function":{"name":"remote.list","arguments":"{"}},
            {"index":1,"id":"call-b","function":{"name":"remote.read","arguments":"{\"path\":"}}
        ]}}]
    });
    let second = json!({
        "choices":[{"delta":{"tool_calls":[
            {"index":1,"function":{"arguments":"\"README.md\"}"}},
            {"index":0,"function":{"arguments":"}"}}
        ]}}],
        "usage":{"prompt_tokens":123,"completion_tokens":17}
    });
    let payload = format!("data: {first}\n\ndata: {second}\n\ndata: [DONE]\n\n");
    let events = decoder.push(payload.as_bytes()).unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::ToolCallStarted { call_id, index: 0, tool_id }
            if call_id == "call-a" && tool_id == "remote.list"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::ToolArgumentsDelta { call_id, index: 1, arguments }
            if call_id == "call-b" && arguments == "\"README.md\"}"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::Usage {
            input_tokens: 123,
            output_tokens: 17,
            ..
        }
    )));
    assert!(events.contains(&ModelStreamEventV2::Completed));
}

#[test]
fn anthropic_decoder_tracks_content_block_identity_and_usage() {
    let mut decoder = ProviderStreamDecoderV2::new(ProviderProtocol::Anthropic);
    let events = decoder
        .push(
            br#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":44,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_1","name":"remote.read","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a.txt\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":3}

event: message_delta
data: {"type":"message_delta","usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::ToolCallStarted { call_id, index: 3, tool_id }
            if call_id == "toolu_1" && tool_id == "remote.read"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::ToolArgumentsDelta { call_id, index: 3, arguments }
            if call_id == "toolu_1" && arguments == "{\"path\":\"a.txt\"}"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::Usage {
            input_tokens: 0,
            output_tokens: 9,
            ..
        }
    )));
}

#[test]
fn non_streaming_ollama_normalizes_multiple_calls_with_stable_synthetic_ids() {
    let events = parse_provider_response_v2(
        ProviderProtocol::Ollama,
        &json!({
            "message": {
                "content": "",
                "tool_calls": [
                    {"function":{"name":"remote.list","arguments":{}}},
                    {"function":{"name":"remote.read","arguments":{"path":"README.md"}}}
                ]
            },
            "prompt_eval_count": 20,
            "eval_count": 5
        }),
    )
    .unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::ToolCallStarted { call_id, index: 1, tool_id }
            if call_id == "ollama-1" && tool_id == "remote.read"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEventV2::Usage {
            input_tokens: 20,
            output_tokens: 5,
            ..
        }
    )));
}

#[test]
fn old_model_profiles_use_a_conservative_context_default() {
    let profile: ModelProfile = serde_json::from_value(json!({
        "id": "773dd047-1224-4d51-a5d8-2c1f12cf8a69",
        "label": "Legacy",
        "provider": "ollama",
        "base_url": "http://localhost:11434",
        "model": "qwen",
        "credential_reference": null,
        "supports_tools": true,
        "supports_vision": false
    }))
    .unwrap();

    assert_eq!(profile.context_window_tokens, None);
    assert_eq!(profile.effective_context_window_tokens(), 32_768);
}

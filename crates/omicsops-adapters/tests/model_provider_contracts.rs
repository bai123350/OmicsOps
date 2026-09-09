use omicsops_adapters::llm::{
    ProviderProtocol, ProviderToolStreamDecoder, RequestBudget, UnifiedModelClient,
    build_provider_request_with_tools, build_provider_request_with_tools_and_budget,
    parse_provider_tool_response_for_request,
};
use omicsops_agent::{
    ModelContentPart, ModelMessage, ModelMessageContent,
    provider::{Provider, ProviderRequest, ProviderStreamEvent, ProviderToolSpec},
};
use serde_json::json;
use url::Url;
use uuid::Uuid;

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

fn generous_budget() -> RequestBudget {
    RequestBudget {
        context_window_tokens: 100_000,
        reserved_output_tokens: 128,
        safety_margin_tokens: 32,
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
fn budget_estimates_the_complete_provider_json_including_unicode_and_schema() {
    let mut request = request();
    request.system = "系统指令 🧪".into();
    request.messages = vec![ModelMessage {
        role: "user".into(),
        content: "请检查项目中的表达矩阵和统计结果。".into(),
    }];
    request.tools[0].description = "读取带有中文说明的项目文件".into();
    request.tools[0].input_schema = json!({
        "type": "object",
        "required": ["path", "format"],
        "properties": {
            "path": {"type": "string", "description": "项目相对路径"},
            "format": {"type": "string", "enum": ["tsv", "h5ad"]}
        }
    });
    let built = build_provider_request_with_tools(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://example.test/v1").unwrap(),
        "model",
        &request,
    )
    .unwrap();
    let budget = RequestBudget {
        context_window_tokens: 100_000,
        reserved_output_tokens: 128,
        safety_margin_tokens: 32,
    };
    let estimated = budget.estimate_input_tokens(&built.body).unwrap();
    assert_eq!(
        estimated,
        serde_json::to_vec(&built.body).unwrap().len() as u64
    );
    assert!(estimated > request.system.len() as u64);
    budget.validate_provider_json(&built.body).unwrap();
}

#[test]
fn budget_accepts_exact_boundary_and_rejects_one_token_over() {
    let budget_shape = generous_budget();
    let built = build_provider_request_with_tools_and_budget(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://example.test/v1").unwrap(),
        "model",
        &request(),
        budget_shape,
    )
    .unwrap();
    let input = budget_shape.estimate_input_tokens(&built.body).unwrap();
    let exact = RequestBudget {
        context_window_tokens: u32::try_from(
            input
                + u64::from(budget_shape.reserved_output_tokens)
                + u64::from(budget_shape.safety_margin_tokens),
        )
        .unwrap(),
        ..budget_shape
    };
    exact.validate_provider_json(&built.body).unwrap();

    let over = RequestBudget {
        context_window_tokens: exact.context_window_tokens - 1,
        ..exact
    };
    let error = over.validate_provider_json(&built.body).unwrap_err();
    assert!(error.to_string().contains("request budget:"));
}

#[test]
fn image_cost_is_unknown_but_image_words_in_tool_schema_are_not_images() {
    let mut request = request();
    request.tools[0].input_schema = json!({
        "type": "object",
        "examples": [{"type": "image", "description": "a file name"}]
    });
    let no_image = build_provider_request_with_tools(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://example.test/v1").unwrap(),
        "model",
        &request,
    )
    .unwrap();
    generous_budget()
        .validate_provider_json(&no_image.body)
        .unwrap();

    request.messages = vec![ModelMessage {
        role: "user".into(),
        content: ModelMessageContent::Parts(vec![ModelContentPart::Image {
            media_type: "image/png".into(),
            data_base64: "aW1hZ2U=".into(),
        }]),
    }];
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Anthropic,
        ProviderProtocol::Ollama,
    ] {
        let base_url = if protocol == ProviderProtocol::Ollama {
            Url::parse("http://localhost:11434").unwrap()
        } else {
            Url::parse("https://example.test/v1").unwrap()
        };
        let built =
            build_provider_request_with_tools(protocol, base_url, "model", &request).unwrap();
        let error = generous_budget()
            .validate_provider_json(&built.body)
            .unwrap_err();
        assert!(error.to_string().contains("request budget:"));
        assert!(error.to_string().contains("image token cost is unknown"));
    }
}

#[test]
fn budget_reserves_output_in_each_provider_native_field() {
    let budget = RequestBudget {
        context_window_tokens: 100_000,
        reserved_output_tokens: 777,
        safety_margin_tokens: 32,
    };
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Anthropic,
        ProviderProtocol::Ollama,
    ] {
        let base_url = if protocol == ProviderProtocol::Ollama {
            Url::parse("http://localhost:11434").unwrap()
        } else {
            Url::parse("https://example.test/v1").unwrap()
        };
        let built = build_provider_request_with_tools_and_budget(
            protocol,
            base_url,
            "model",
            &request(),
            budget,
        )
        .unwrap();
        match protocol {
            ProviderProtocol::OpenAiCompatible | ProviderProtocol::Anthropic => {
                assert_eq!(built.body["max_tokens"], 777);
            }
            ProviderProtocol::Ollama => {
                assert_eq!(built.body["options"]["num_predict"], 777);
            }
        }
        budget.validate_provider_json(&built.body).unwrap();
    }

    let official = build_provider_request_with_tools_and_budget(
        ProviderProtocol::OpenAiCompatible,
        Url::parse("https://api.openai.com/v1").unwrap(),
        "o3-mini",
        &request(),
        budget,
    )
    .unwrap();
    assert_eq!(official.body["max_completion_tokens"], 777);
    assert!(official.body.get("max_tokens").is_none());
    budget.validate_provider_json(&official.body).unwrap();
}

#[test]
fn budget_validation_is_local_and_happens_before_provider_network_io() {
    let client = UnifiedModelClient::new(
        Uuid::new_v4(),
        ProviderProtocol::Ollama,
        Url::parse("http://127.0.0.1:1").unwrap(),
        "model",
        None,
    )
    .unwrap()
    .with_request_budget(RequestBudget {
        context_window_tokens: 32,
        reserved_output_tokens: 16,
        safety_margin_tokens: 8,
    });
    let error = client.validate_request(&request()).unwrap_err();
    assert!(error.to_string().contains("request budget:"));
    assert!(error.to_string().contains("exceeding context window"));
}

#[tokio::test]
async fn budget_enabled_stream_stops_before_network_or_retry_events() {
    let client = UnifiedModelClient::new(
        Uuid::new_v4(),
        ProviderProtocol::Ollama,
        Url::parse("http://127.0.0.1:1").unwrap(),
        "model",
        None,
    )
    .unwrap()
    .with_request_budget(RequestBudget {
        context_window_tokens: 32,
        reserved_output_tokens: 16,
        safety_margin_tokens: 8,
    });
    let mut events = Vec::new();
    let error = client
        .stream_with_provider(request(), |event| events.push(event))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("request budget:"));
    assert!(events.is_empty());
}

#[tokio::test]
async fn budget_enabled_probe_cannot_bypass_preflight() {
    let client = UnifiedModelClient::new(
        Uuid::new_v4(),
        ProviderProtocol::Ollama,
        Url::parse("http://127.0.0.1:1").unwrap(),
        "model",
        None,
    )
    .unwrap()
    .with_request_budget(RequestBudget {
        context_window_tokens: 1,
        reserved_output_tokens: 1,
        safety_margin_tokens: 0,
    });
    let error = client.probe().await.unwrap_err();
    assert!(error.to_string().contains("request budget:"));
}

#[test]
fn zero_reserved_output_is_rejected_by_budget_validation() {
    let built = build_provider_request_with_tools(
        ProviderProtocol::Ollama,
        Url::parse("http://localhost:11434").unwrap(),
        "model",
        &request(),
    )
    .unwrap();
    let error = RequestBudget {
        context_window_tokens: 100_000,
        reserved_output_tokens: 0,
        safety_margin_tokens: 0,
    }
    .validate_provider_json(&built.body)
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("reserved output must be greater than zero")
    );
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

#[test]
fn truncated_responses_never_turn_into_completed_tool_calls() {
    for (protocol, value) in [
        (
            ProviderProtocol::OpenAiCompatible,
            json!({"choices":[{"finish_reason":"length","message":{"content":"partial"}}]}),
        ),
        (
            ProviderProtocol::Anthropic,
            json!({"stop_reason":"max_tokens","content":[{"type":"text","text":"partial"}]}),
        ),
        (
            ProviderProtocol::Ollama,
            json!({"done":true,"done_reason":"length","message":{"content":"partial"}}),
        ),
    ] {
        assert!(
            parse_provider_tool_response_for_request(protocol, &value, &request())
                .unwrap_err()
                .to_string()
                .contains("truncated_output")
        );
    }
    let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
    assert!(
        decoder
            .push(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n")
            .unwrap_err()
            .to_string()
            .contains("truncated_output")
    );
    let mut anthropic = ProviderToolStreamDecoder::new(ProviderProtocol::Anthropic);
    assert!(anthropic.push(b"data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n").is_err());
}

#[test]
fn stream_eof_without_terminal_event_is_not_success() {
    let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
    decoder
        .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
        .unwrap();
    assert!(
        decoder
            .finish()
            .unwrap_err()
            .to_string()
            .contains("no terminal provider event")
    );
    let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
    decoder.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\n").unwrap();
    decoder.finish().unwrap();
}

#[test]
fn provider_reasoning_fields_are_never_mapped_to_public_text() {
    let fixtures = [
        (
            ProviderProtocol::OpenAiCompatible,
            json!({"choices":[{"message":{"content":"public update","reasoning_content":"private chain"}}]}),
        ),
        (
            ProviderProtocol::Anthropic,
            json!({"content":[
                {"type":"thinking","thinking":"private chain"},
                {"type":"text","text":"public update"}
            ]}),
        ),
        (
            ProviderProtocol::Ollama,
            json!({"message":{"content":"public update","thinking":"private chain"}}),
        ),
    ];
    for (protocol, fixture) in fixtures {
        let events =
            parse_provider_tool_response_for_request(protocol, &fixture, &request()).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            ProviderStreamEvent::TextDelta { text } if text == "public update"
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            ProviderStreamEvent::TextDelta { text } if text.contains("private chain")
        )));
    }

    let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
    let events = decoder
        .push(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private chain\"}}]}\n\n")
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::TextDelta { .. }))
    );
}

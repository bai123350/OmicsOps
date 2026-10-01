use async_trait::async_trait;
use omicsops_adapters::{AdapterResult, llm::RequestBudget, responses::*};
use omicsops_agent::{
    ModelMessage,
    provider::{
        ProviderRequest, ProviderStreamEvent, ProviderToolSpec, coalesce_provider_usage_samples,
    },
};
use omicsops_protocol::{ModelReplayItemV4, ToolCallV4};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use url::Url;
use uuid::Uuid;

fn request() -> ProviderRequest {
    ProviderRequest {
        system: "Host policy".into(),
        messages: vec![ModelMessage {
            role: "user".into(),
            content: "计算 α".into(),
        }],
        tools: vec![ProviderToolSpec {
            id: "host.read".into(),
            description: "Read".into(),
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        }],
        require_strict_json_fallback: false,
        replay: vec![],
    }
}
fn frame(v: Value) -> String {
    format!("data: {v}\n\n")
}
fn completed(output: Value) -> String {
    frame(
        json!({"type":"response.completed","response":{"id":"resp_1","status":"completed","output":output,"usage":{"input_tokens":23,"output_tokens":17,"output_tokens_details":{"reasoning_tokens":8},"private":"SECRET_SENTINEL"}}}),
    )
}
fn call(name: &str, id: &str, args: &str) -> Value {
    json!({"type":"function_call","id":format!("fc_{id}"),"call_id":id,"name":name,"arguments":args,"status":"completed"})
}
fn alias() -> String {
    build_responses_request(
        ResponsesEndpointKind::CodexSubscription,
        &Url::parse("https://chatgpt.com/backend-api").unwrap(),
        "gpt-5.5",
        &request(),
        None,
        None,
    )
    .unwrap()
    .body["tools"][0]["name"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn go_reasoning_options_require_exact_reviewed_model_and_protocol() {
    let go = Url::parse("https://opencode.ai/zen/go/v1").unwrap();
    for (model, effort, allowed) in [
        ("grok-4.7", "none", false),
        ("grok-4.7", "high", true),
        ("gpt-6-luna", "max", true),
        ("gpt-6-luna", "minimal", false),
        ("muse-spark-1.3-contributor", "minimal", true),
        ("grok-4.7-sibling", "high", false),
    ] {
        let shaped = build_responses_request(
            ResponsesEndpointKind::OpenCodeGo,
            &go,
            model,
            &request(),
            None,
            Some(effort),
        );
        assert_eq!(shaped.is_ok(), allowed);
        if let Ok(wire) = shaped {
            assert_eq!(wire.body["reasoning"]["effort"], effort);
        }
    }
    assert!(
        build_responses_request(
            ResponsesEndpointKind::CodexSubscription,
            &Url::parse("https://chatgpt.com/backend-api").unwrap(),
            "gpt-6-luna",
            &request(),
            None,
            Some("max")
        )
        .is_err()
    );
}

#[test]
fn responses_shapes_native_tool_history_and_exact_endpoints() {
    let mut r = request();
    r.replay = vec![
        ModelReplayItemV4::ResponsesReasoning {
            id: "rs_1".into(),
            encrypted_content: "opaque".into(),
        },
        ModelReplayItemV4::ToolCall {
            call: ToolCallV4 {
                call_id: "host_call".into(),
                tool_id: "host.read".into(),
                arguments: json!({"path":"a"}),
            },
        },
        ModelReplayItemV4::ToolResult {
            call_id: "host_call".into(),
            output: "host evidence".into(),
        },
    ];
    let wire = build_responses_request(
        ResponsesEndpointKind::CodexSubscription,
        &Url::parse("https://chatgpt.com/backend-api/").unwrap(),
        "gpt-5.5",
        &r,
        None,
        Some("high"),
    )
    .unwrap();
    assert_eq!(
        wire.endpoint.as_str(),
        "https://chatgpt.com/backend-api/codex/responses"
    );
    assert_eq!(wire.body["instructions"], "Host policy");
    assert_eq!(wire.body["store"], false);
    assert_eq!(wire.body["stream"], true);
    assert!(wire.body.get("max_output_tokens").is_none());
    assert!(wire.body.get("previous_response_id").is_none());
    assert_eq!(wire.body["input"][2]["call_id"], "host_call");
    assert_eq!(wire.body["input"][3]["call_id"], "host_call");
    assert_eq!(wire.body["input"][3]["type"], "function_call_output");
    assert_eq!(wire.body["tools"][0]["type"], "function");
    for foreign in [
        "https://chatgpt.com.evil/backend-api",
        "http://chatgpt.com/backend-api",
        "https://chatgpt.com:444/backend-api",
        "https://chatgpt.com/backend-api?q=1",
        "https://x@chatgpt.com/backend-api",
        "https://chatgpt.com/backend-api/other",
    ] {
        assert!(
            responses_endpoint(
                ResponsesEndpointKind::CodexSubscription,
                &Url::parse(foreign).unwrap()
            )
            .is_err()
        );
    }
    assert!(
        responses_endpoint(
            ResponsesEndpointKind::OpenCodeGo,
            &Url::parse("https://opencode.ai/zen/v1").unwrap()
        )
        .is_err()
    );
    let budget = RequestBudget {
        context_window_tokens: 8192,
        reserved_output_tokens: 100,
        safety_margin_tokens: 20,
    };
    let small = build_responses_request(
        ResponsesEndpointKind::OpenCodeGo,
        &Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
        "grok-4.7",
        &r,
        Some(budget),
        None,
    )
    .unwrap();
    r.replay[0] = ModelReplayItemV4::ResponsesReasoning {
        id: "rs_1".into(),
        encrypted_content: "x".repeat(9000),
    };
    assert!(
        build_responses_request(
            ResponsesEndpointKind::OpenCodeGo,
            &Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
            "grok-4.7",
            &r,
            Some(budget),
            None
        )
        .is_err()
    );
    assert!(serde_json::to_vec(&small.body).unwrap().len() > 200);
}

#[test]
fn responses_keepalive_does_not_advance_content_and_historical_tools_are_not_regranted() {
    let r = request();
    let mut decoder = ResponsesStreamDecoder::for_request(&r);
    for kind in [
        "response.in_progress",
        "response.output_item.done",
        "response.content_part.done",
        "response.output_text.done",
    ] {
        let bytes = format!("data: {}\n\n", json!({"type":kind})).into_bytes();
        assert!(
            !decoder
                .push(&bytes)
                .unwrap()
                .iter()
                .any(|event| matches!(event, ProviderStreamEvent::ContentProgress { .. }))
        );
    }
    let mut history = r;
    history.replay = vec![
        ModelReplayItemV4::ToolCall {
            call: ToolCallV4 {
                call_id: "prior".into(),
                tool_id: "previous.page_tool".into(),
                arguments: json!({}),
            },
        },
        ModelReplayItemV4::ToolResult {
            call_id: "prior".into(),
            output: "host evidence".into(),
        },
    ];
    let wire = build_responses_request(
        ResponsesEndpointKind::CodexSubscription,
        &Url::parse("https://chatgpt.com/backend-api").unwrap(),
        "fixture",
        &history,
        None,
        None,
    )
    .unwrap();
    assert_eq!(wire.body["tools"].as_array().unwrap().len(), 1);
    assert_ne!(wire.body["input"][1]["name"], wire.body["tools"][0]["name"]);
}

#[test]
fn responses_requires_completed_terminal_and_complete_arguments() {
    let name = alias();
    let mut wire = frame(
        json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"fc_c1","call_id":"c1","name":name,"arguments":""}}),
    );
    wire += &frame(
        json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"fc_c1","delta":"{\"path\":\"α\"}"}),
    );
    wire += &completed(json!([call(&name, "c1", "{\"path\":\"α\"}")]));
    let mut decoder = ResponsesStreamDecoder::for_request(&request());
    let mut events = vec![];
    for b in wire.bytes() {
        events.extend(decoder.push(&[b]).unwrap());
    }
    events.extend(decoder.finish().unwrap());
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ProviderStreamEvent::ToolCallCompleted { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(
        |e| matches!(e, ProviderStreamEvent::ToolCallStarted{tool_id,..} if tool_id=="host.read")
    ));
    for invalid in [
        "data: [DONE]\n\n".into(),
        frame(json!({"type":"response.incomplete"})),
        completed(json!([call(&name, "c1", "{")])),
        completed(json!([call(&name, "c1", "{}"), call(&name, "c1", "{}")])),
    ] {
        let mut decoder = ResponsesStreamDecoder::for_request(&request());
        let result = decoder
            .push(invalid.as_bytes())
            .and_then(|_| decoder.finish());
        assert!(result.is_err());
    }
    let mut truncated = ResponsesStreamDecoder::for_request(&request());
    truncated
        .push(frame(json!({"type":"response.output_text.delta","delta":"partial"})).as_bytes())
        .unwrap();
    assert!(truncated.finish().is_err());
    let mut clean = ResponsesStreamDecoder::for_request(&request());
    let events = clean.push(completed(json!([{ "type":"message","id":"msg","role":"assistant","status":"completed","content":[{"type":"output_text","text":"新结果"}]}])).as_bytes()).unwrap();
    assert!(!format!("{events:?}").contains("partial"));
}

struct MockTransport {
    calls: AtomicUsize,
    status: u16,
    wire: String,
}
#[test]
fn responses_failure_codes_are_sanitized_and_preserve_recovery_categories() {
    for (event, expected) in [
        (
            json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}),
            "truncated_output:",
        ),
        (
            json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"content_filter"}}}),
            "responses_content_filter",
        ),
        (
            json!({"type":"response.failed","response":{"error":{"code":"rate_limit_exceeded","message":"SENTINEL"}}}),
            "responses_http_429",
        ),
        (
            json!({"type":"error","code":"context_length_exceeded","message":"SENTINEL"}),
            "context_length_exceeded",
        ),
        (
            json!({"type":"error","code":"insufficient_quota","message":"SENTINEL"}),
            "responses_quota_exhausted",
        ),
        (
            json!({"type":"error","code":"permission_denied","message":"SENTINEL"}),
            "responses_http_403",
        ),
        (
            json!({"type":"error","code":"server_error","message":"SENTINEL"}),
            "responses_http_503",
        ),
        (
            json!({"type":"error","code":"UNKNOWN_SENTINEL","message":"SENTINEL"}),
            "responses_generation_failed",
        ),
    ] {
        let mut decoder = ResponsesStreamDecoder::for_request(&request());
        let error = decoder
            .push(frame(event).as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("SENTINEL"));
        assert!(decoder.finish().is_err());
    }
}
#[tokio::test]
async fn responses_http_error_details_are_bounded_allowlisted_and_single_dispatch() {
    for (body, expected) in [
        (
            json!({"error":{"type":"insufficient_quota","message":"SENTINEL"}}).to_string(),
            "responses_quota_exhausted",
        ),
        (
            json!({"error":{"code":"context_length_exceeded","message":"SENTINEL"}}).to_string(),
            "context_length_exceeded",
        ),
        ("SENTINEL".repeat(150_000), "responses_http_429"),
    ] {
        let transport = Arc::new(MockTransport {
            calls: AtomicUsize::new(0),
            status: 429,
            wire: body,
        });
        let client = ResponsesHttpClient::new(
            ResponsesEndpointKind::OpenCodeGo,
            Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
            "fixture".into(),
            None,
            Uuid::new_v4(),
        )
        .unwrap()
        .with_transport(transport.clone());
        let error = client
            .stream_once(
                request(),
                ResponsesAuthorization::bearer("fixture-token".into(), None),
                |_| {},
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("SENTINEL"));
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn responses_interleaved_calls_keep_item_identity_and_reject_native_tools() {
    let name = alias();
    let added = |index: u32, id: &str| {
        frame(
            json!({"type":"response.output_item.added","output_index":index,"item":{"type":"function_call","id":format!("fc_{id}"),"call_id":id,"name":name,"arguments":""}}),
        )
    };
    let delta = |index: u32, id: &str, args: &str| {
        frame(
            json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":format!("fc_{id}"),"delta":args}),
        )
    };
    let mut d = ResponsesStreamDecoder::for_request(&request());
    let stream = added(2, "a")
        + &added(5, "b")
        + &delta(5, "b", "{\"path\":\"b\"}")
        + &delta(2, "a", "{\"path\":\"a\"}")
        + &completed(json!([
            call(&name, "a", "{\"path\":\"a\"}"),
            call(&name, "b", "{\"path\":\"b\"}")
        ]));
    let events = d.push(stream.as_bytes()).unwrap();
    d.finish().unwrap();
    let indices: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let ProviderStreamEvent::ToolCallStarted { index, .. } = e {
                Some(*index)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(indices, vec![2, 5]);
    for stream in [
        added(2, "a") + &delta(2, "b", "{}"),
        added(2, "a") + &added(5, "a"),
        completed(json!([{"type":"web_search_call","id":"native"}])),
    ] {
        assert!(
            ResponsesStreamDecoder::for_request(&request())
                .push(stream.as_bytes())
                .is_err()
        );
    }
}
#[async_trait]
impl ResponsesTransport for MockTransport {
    async fn post(
        &self,
        endpoint: Url,
        headers: reqwest::header::HeaderMap,
        _body: Value,
        _timeout: std::time::Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(endpoint.as_str(), "https://opencode.ai/zen/go/v1/responses");
        assert!(
            headers["x-opencode-session"]
                .to_str()
                .unwrap()
                .parse::<Uuid>()
                .is_ok()
        );
        assert_eq!(headers["user-agent"], "OmicsOps/0.1");
        Ok(ResponsesHttpStream {
            status: self.status,
            body: Box::pin(futures_util::stream::iter(vec![Ok(self
                .wire
                .as_bytes()
                .to_vec())])),
        })
    }
}
#[tokio::test]
async fn responses_preserves_usage_and_attempt_boundaries() {
    let transport = Arc::new(MockTransport {
        calls: AtomicUsize::new(0),
        status: 200,
        wire: completed(json!([])),
    });
    let client = ResponsesHttpClient::new(
        ResponsesEndpointKind::OpenCodeGo,
        Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
        "grok-4.7".into(),
        None,
        Uuid::new_v4(),
    )
    .unwrap()
    .with_transport(transport.clone());
    let mut events = vec![];
    client
        .stream_once(
            request(),
            ResponsesAuthorization::bearer("fixture".into(), None),
            |e| events.push(e),
        )
        .await
        .unwrap();
    let samples = events.iter().filter_map(|e| {
        if let ProviderStreamEvent::UsageObserved { sample } = e {
            Some(sample.clone())
        } else {
            None
        }
    });
    assert_eq!(
        coalesce_provider_usage_samples(samples)
            .unwrap()
            .output_tokens,
        Some(17)
    );
    assert!(!format!("{events:?}").contains("SECRET_SENTINEL"));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let redirected = Arc::new(MockTransport {
        calls: AtomicUsize::new(0),
        status: 307,
        wire: "".into(),
    });
    assert!(
        client
            .with_transport(redirected.clone())
            .stream_once(
                request(),
                ResponsesAuthorization::bearer("fixture".into(), None),
                |_| {}
            )
            .await
            .is_err()
    );
    assert_eq!(redirected.calls.load(Ordering::SeqCst), 1);
}

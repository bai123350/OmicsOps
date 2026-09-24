use async_trait::async_trait;
use futures_util::StreamExt;
use omicsops_agent::provider::{
    Provider, ProviderRequest as ProviderModelRequest, ProviderStreamEvent,
    ProviderUsageAggregation, ProviderUsageSample, ProviderUsageState,
};
use omicsops_agent::{AgentError, AgentResult, ModelContentPart, ModelMessageContent};
use schemars::{JsonSchema, schema_for};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use url::Url;
use uuid::Uuid;

use crate::{AdapterError, AdapterResult};

const MODEL_REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
const MODEL_MAX_RETRIES: u8 = 3;
pub const MODEL_PROBE_OUTPUT_TOKENS: u32 = 4096;
const OPENCODE_GO_USER_AGENT: &str = concat!("OmicsOps/", env!("CARGO_PKG_VERSION"));

fn is_opencode_go_base_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("opencode.ai")
        && url.port_or_known_default() == Some(443)
        && matches!(url.path(), "/zen/go/v1" | "/zen/go/v1/")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn retryable_model_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504)
}

#[cfg(test)]
mod retry_tests {
    use super::retryable_model_status;

    #[test]
    fn retries_only_transient_provider_statuses() {
        for status in [429, 500, 502, 503, 504] {
            assert!(retryable_model_status(
                reqwest::StatusCode::from_u16(status).unwrap()
            ));
        }
        for status in [400, 401, 403, 404, 422] {
            assert!(!retryable_model_status(
                reqwest::StatusCode::from_u16(status).unwrap()
            ));
        }
    }
}

#[cfg(test)]
mod opencode_go_tests {
    use super::*;

    fn client(base_url: &str, session_id: Uuid) -> UnifiedModelClient {
        UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::OpenAiCompatible,
            Url::parse(base_url).unwrap(),
            "glm-5.3-flash",
            Some("test-only".into()),
        )
        .unwrap()
        .with_session_id(session_id)
    }

    fn header<'a>(request: &'a reqwest::Request, name: &str) -> Option<&'a str> {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
    }

    #[test]
    fn exact_opencode_go_requests_carry_client_and_stable_conversation_identity() {
        let conversation = Uuid::new_v4();
        let expected_session = conversation.to_string();
        let turn_one = client("https://opencode.ai/zen/go/v1", conversation);
        let turn_two = client("https://opencode.ai:443/zen/go/v1/", conversation);
        let other = client("https://opencode.ai/zen/go/v1", Uuid::new_v4());
        let body = json!({"model":"glm-5.3-flash"});

        for request in [
            turn_one
                .post_json_request(
                    Url::parse("https://opencode.ai/zen/go/v1/chat/completions").unwrap(),
                    &body,
                )
                .build()
                .unwrap(),
            turn_two
                .post_json_request(
                    Url::parse("https://opencode.ai/zen/go/v1/chat/completions").unwrap(),
                    &body,
                )
                .build()
                .unwrap(),
            turn_one
                .get_request(Url::parse("https://opencode.ai/zen/go/v1/models").unwrap())
                .build()
                .unwrap(),
        ] {
            assert_eq!(
                header(&request, "user-agent"),
                Some(concat!("OmicsOps/", env!("CARGO_PKG_VERSION")))
            );
            assert_eq!(
                header(&request, "x-opencode-session"),
                Some(expected_session.as_str())
            );
        }

        let other_request = other
            .get_request(Url::parse("https://opencode.ai/zen/go/v1/models").unwrap())
            .build()
            .unwrap();
        assert_ne!(
            header(&other_request, "x-opencode-session"),
            Some(expected_session.as_str())
        );
    }

    #[test]
    fn opencode_headers_never_leak_to_lookalike_or_noncanonical_base_urls() {
        let session = Uuid::new_v4();
        let body = json!({"model":"glm-5.3-flash"});
        for base_url in [
            "http://opencode.ai/zen/go/v1",
            "https://opencode.ai:8443/zen/go/v1",
            "https://opencode.ai/zen/go/v1?forward=1",
            "https://opencode.ai/zen/go/v1#fragment",
            "https://user@opencode.ai/zen/go/v1",
            "https://opencode.ai.example/zen/go/v1",
            "https://opencode.ai/zen/go/v10",
        ] {
            let request = client(base_url, session)
                .post_json_request(
                    Url::parse("https://gateway.example/v1/chat/completions").unwrap(),
                    &body,
                )
                .build()
                .unwrap();
            assert_eq!(header(&request, "x-opencode-session"), None, "{base_url}");
            assert_ne!(
                header(&request, "user-agent"),
                Some(concat!("OmicsOps/", env!("CARGO_PKG_VERSION"))),
                "{base_url}"
            );
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::{ProviderProtocol, RequestBudget, UnifiedModelClient};
    use serde_json::json;
    use url::Url;
    use uuid::Uuid;

    #[tokio::test]
    async fn final_send_boundary_rechecks_budget_for_non_streaming_fallback_shape() {
        let client = UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::Ollama,
            Url::parse("http://127.0.0.1:1").unwrap(),
            "model",
            None,
        )
        .unwrap()
        .with_request_budget(RequestBudget {
            context_window_tokens: 8,
            reserved_output_tokens: 1,
            safety_margin_tokens: 1,
        });
        let body = json!({
            "model": "model",
            "stream": false,
            "messages": [{"role": "user", "content": "a request too large for this budget"}]
        });
        let mut events = Vec::new();
        let error = client
            .send_with_retry_provider(
                &Url::parse("http://127.0.0.1:1/api/chat").unwrap(),
                &body,
                &mut |event| events.push(event),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("request budget:"));
        assert!(events.is_empty());
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    fn probe_server(
        response_for: impl FnOnce(&Value) -> Value + Send + 'static,
    ) -> (Url, mpsc::Receiver<Value>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            let header_end = loop {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "probe request ended before its headers");
                request.extend_from_slice(&chunk[..read]);
                if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = std::str::from_utf8(&request[..header_end]).unwrap();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            while request.len() - header_end < content_length {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "probe request ended before its JSON body");
                request.extend_from_slice(&chunk[..read]);
            }
            let body: Value =
                serde_json::from_slice(&request[header_end..header_end + content_length]).unwrap();
            let response = serde_json::to_vec(&response_for(&body)).unwrap();
            let _ = request_tx.send(body);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            )
            .unwrap();
            stream.write_all(&response).unwrap();
        });
        (
            Url::parse(&format!("http://{address}/v1")).unwrap(),
            request_rx,
            server,
        )
    }

    fn client(protocol: ProviderProtocol, base_url: Url) -> UnifiedModelClient {
        UnifiedModelClient::new(
            Uuid::new_v4(),
            protocol,
            base_url,
            "thinking-model",
            Some("test-only".into()),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn plain_probe_reserves_room_for_models_with_implicit_reasoning() {
        let (base_url, request, server) = probe_server(|body| {
            if body["max_tokens"].as_u64().unwrap_or_default() >= 4096 {
                json!({"choices":[{"finish_reason":"stop","message":{"content":"OK"}}]})
            } else {
                json!({"choices":[{"finish_reason":"length","message":{"content":"","reasoning_content":"still thinking"}}]})
            }
        });

        let result = client(ProviderProtocol::OpenAiCompatible, base_url)
            .probe()
            .await;
        let body = request.recv().unwrap();
        server.join().unwrap();

        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(result.unwrap().response_preview, "OK");
    }

    #[tokio::test]
    async fn probe_clamps_a_larger_runtime_budget_to_its_own_limit() {
        let (base_url, request, server) = probe_server(
            |_| json!({"choices":[{"finish_reason":"stop","message":{"content":"OK"}}]}),
        );

        let result = client(ProviderProtocol::OpenAiCompatible, base_url)
            .with_request_budget(RequestBudget {
                context_window_tokens: 100_000,
                reserved_output_tokens: 32_000,
                safety_margin_tokens: 1024,
            })
            .probe()
            .await;
        let body = request.recv().unwrap();
        server.join().unwrap();

        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(result.unwrap().response_preview, "OK");
    }

    #[tokio::test]
    async fn truncated_probe_reports_an_incomplete_test_reply() {
        let (base_url, _, server) = probe_server(
            |_| json!({"choices":[{"finish_reason":"length","message":{"content":"partial"}}]}),
        );

        let error = client(ProviderProtocol::OpenAiCompatible, base_url)
            .probe()
            .await
            .unwrap_err();
        server.join().unwrap();

        assert!(error.to_string().contains("probe_response_incomplete:"));
        assert!(error.to_string().contains("test reply"));
        assert!(!error.to_string().contains("partial tool calls"));
    }

    #[tokio::test]
    async fn probe_rejects_a_blank_final_answer() {
        let (base_url, _, server) = probe_server(
            |_| json!({"choices":[{"finish_reason":"stop","message":{"content":"  \n"}}]}),
        );

        let error = client(ProviderProtocol::OpenAiCompatible, base_url)
            .probe()
            .await
            .unwrap_err();
        server.join().unwrap();

        assert!(
            error
                .to_string()
                .contains("response did not match the selected provider protocol")
        );
    }

    #[tokio::test]
    async fn probe_requires_a_protocol_terminal_completion() {
        let cases = [
            (
                ProviderProtocol::OpenAiCompatible,
                json!({"choices":[{"finish_reason":null,"message":{"content":"OK"}}]}),
            ),
            (
                ProviderProtocol::Anthropic,
                json!({"stop_reason":null,"content":[{"type":"text","text":"OK"}]}),
            ),
            (
                ProviderProtocol::Ollama,
                json!({"done":false,"message":{"content":"OK"}}),
            ),
        ];

        for (protocol, response) in cases {
            let (base_url, _, server) = probe_server(move |_| response);
            let error = client(protocol, base_url).probe().await.unwrap_err();
            server.join().unwrap();
            assert!(
                error.to_string().contains("probe_response_incomplete:"),
                "{protocol:?}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn probe_rejects_unsolicited_tool_payloads() {
        let cases = [
            (
                ProviderProtocol::OpenAiCompatible,
                json!({
                    "choices":[{
                        "finish_reason":"stop",
                        "message":{"content":"OK","tool_calls":[{"id":"call-1"}]}
                    }]
                }),
            ),
            (
                ProviderProtocol::Anthropic,
                json!({
                    "stop_reason":"end_turn",
                    "content":[
                        {"type":"text","text":"OK"},
                        {"type":"tool_use","id":"call-1","name":"unexpected","input":{}}
                    ]
                }),
            ),
            (
                ProviderProtocol::Ollama,
                json!({
                    "done":true,
                    "done_reason":"stop",
                    "message":{"content":"OK","tool_calls":[{"function":{"name":"unexpected"}}]}
                }),
            ),
        ];

        for (protocol, response) in cases {
            let (base_url, _, server) = probe_server(move |_| response);
            let error = client(protocol, base_url).probe().await.unwrap_err();
            server.join().unwrap();
            assert!(
                error.to_string().contains("probe_response_incomplete:"),
                "{protocol:?}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn anthropic_probe_reads_text_after_a_thinking_block() {
        let (base_url, _, server) = probe_server(|_| {
            json!({
                "stop_reason":"end_turn",
                "content":[
                    {"type":"thinking","thinking":"checking"},
                    {"type":"text","text":"OK"}
                ]
            })
        });

        let result = client(ProviderProtocol::Anthropic, base_url).probe().await;
        server.join().unwrap();

        assert_eq!(result.unwrap().response_preview, "OK");
    }

    #[tokio::test]
    async fn anthropic_probe_does_not_treat_non_text_blocks_as_a_final_answer() {
        let (base_url, _, server) = probe_server(|_| {
            json!({
                "stop_reason":"end_turn",
                "content":[{"type":"thinking","text":"not a final answer"}]
            })
        });

        let error = client(ProviderProtocol::Anthropic, base_url)
            .probe()
            .await
            .unwrap_err();
        server.join().unwrap();

        assert!(
            error
                .to_string()
                .contains("response did not match the selected provider protocol")
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProtocol {
    Anthropic,
    OpenAiCompatible,
    Ollama,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRequest {
    pub endpoint: Url,
    pub body: Value,
    pub requires_credential: bool,
}

/// A conservative preflight budget for one complete provider request.
///
/// The adapter currently estimates input tokens by counting the UTF-8 bytes of
/// the compact, serialized provider JSON. This is deliberately conservative
/// and is not a substitute for a provider tokenizer. Image token costs are
/// provider/model dependent, so this model-independent public type continues
/// to reject image requests; the unified client has a separate, exact
/// trusted-model estimator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestBudget {
    pub context_window_tokens: u32,
    pub reserved_output_tokens: u32,
    pub safety_margin_tokens: u32,
}

/// Measurements of one already-shaped provider request. JSON bytes describe
/// the actual wire body; the text shape and image payload are reported as
/// separate byte categories so callers never convert base64 bytes to tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestBudgetMetrics {
    pub serialized_request_bytes: u64,
    pub text_shape_bytes: u64,
    pub image_payload_bytes: u64,
    pub image_count: u32,
    pub image_bound_tokens: Option<u64>,
}

impl RequestBudget {
    const ERROR_PREFIX: &'static str = "request budget:";

    /// Estimate the input token count for a fully shaped provider JSON body.
    ///
    /// This uses compact UTF-8 byte length as a conservative estimate, rather
    /// than pretending to know the selected model's tokenizer. The complete
    /// JSON body is measured, so system content, messages, tool schemas and
    /// provider formatting are all included. Image requests remain rejected by
    /// this public model-independent method; the unified client has a separate,
    /// exact trusted-model path for bounded image requests.
    pub fn estimate_input_tokens(&self, provider_json: &Value) -> AdapterResult<u64> {
        if contains_unknown_image(provider_json) {
            return Err(AdapterError::Llm(format!(
                "{} image token cost is unknown; refusing to estimate it as zero",
                Self::ERROR_PREFIX
            )));
        }
        serde_json::to_vec(provider_json)
            .map(|json| json.len() as u64)
            .map_err(|error| {
                AdapterError::Llm(format!(
                    "{} could not serialize provider JSON for conservative UTF-8 byte estimation: {error}",
                    Self::ERROR_PREFIX
                ))
            })
    }

    fn estimate_input_tokens_with_image_budget(
        &self,
        provider_json: &Value,
        per_image_tokens: u64,
    ) -> AdapterResult<u64> {
        let (text_shape, image_count) = image_budget_shape(provider_json)?;
        let text_tokens = serde_json::to_vec(&text_shape)
            .map(|json| json.len() as u64)
            .map_err(|error| {
                AdapterError::Llm(format!(
                    "{} could not serialize provider JSON for conservative UTF-8 byte estimation: {error}",
                    Self::ERROR_PREFIX
                ))
            })?;
        Ok(text_tokens.saturating_add(per_image_tokens.saturating_mul(image_count)))
    }

    /// Validate the complete provider JSON against this budget without doing
    /// any network I/O. The inequality enforced is
    /// `estimated_input + reserved_output + safety_margin <= context_window`.
    pub fn validate_provider_json(&self, provider_json: &Value) -> AdapterResult<()> {
        if self.context_window_tokens == 0 {
            return Err(AdapterError::Llm(format!(
                "{} context window must be greater than zero",
                Self::ERROR_PREFIX
            )));
        }
        if self.reserved_output_tokens == 0 {
            return Err(AdapterError::Llm(format!(
                "{} reserved output must be greater than zero",
                Self::ERROR_PREFIX
            )));
        }
        let estimated_input = self.estimate_input_tokens(provider_json)?;
        self.validate_estimated_input(estimated_input)
    }

    fn validate_provider_json_with_image_budget(
        &self,
        provider_json: &Value,
        per_image_tokens: u64,
    ) -> AdapterResult<()> {
        if self.context_window_tokens == 0 {
            return Err(AdapterError::Llm(format!(
                "{} context window must be greater than zero",
                Self::ERROR_PREFIX
            )));
        }
        if self.reserved_output_tokens == 0 {
            return Err(AdapterError::Llm(format!(
                "{} reserved output must be greater than zero",
                Self::ERROR_PREFIX
            )));
        }
        let estimated_input =
            self.estimate_input_tokens_with_image_budget(provider_json, per_image_tokens)?;
        self.validate_estimated_input(estimated_input)
    }

    fn validate_estimated_input(&self, estimated_input: u64) -> AdapterResult<()> {
        let required = estimated_input
            .saturating_add(u64::from(self.reserved_output_tokens))
            .saturating_add(u64::from(self.safety_margin_tokens));
        let context_window = u64::from(self.context_window_tokens);
        if required > context_window {
            return Err(AdapterError::Llm(format!(
                "{} provider JSON needs {required} tokens (input estimate {estimated_input} conservative UTF-8 bytes, reserved output {}, safety margin {}), exceeding context window {context_window}; estimate is not an exact tokenizer count",
                Self::ERROR_PREFIX,
                self.reserved_output_tokens,
                self.safety_margin_tokens,
            )));
        }
        Ok(())
    }
}

fn contains_unknown_image(value: &Value) -> bool {
    value
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| messages.iter().any(message_contains_image))
}

fn message_contains_image(message: &Value) -> bool {
    if let Some(images) = message.get("images") {
        match images {
            Value::Array(images) if images.is_empty() => {}
            Value::Null => {}
            _ => return true,
        }
    }
    let Some(content) = message.get("content") else {
        return false;
    };
    match content {
        Value::Array(parts) => parts.iter().any(|part| {
            part.get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "image" | "image_url"))
        }),
        Value::Object(part) => part
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "image" | "image_url")),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

const IMAGE_BUDGET_PLACEHOLDER: &str = "data:image/placeholder;base64,IMAGE";

/// Return a copy suitable for conservative text estimation and the number of
/// image payloads represented by the original body. The image URL payload is
/// deliberately replaced only in the copy; the actual request body keeps its
/// original bytes and URL.
fn image_budget_shape(provider_json: &Value) -> AdapterResult<(Value, u64)> {
    let mut shape = provider_json.clone();
    let Some(messages) = shape.get_mut("messages").and_then(Value::as_array_mut) else {
        if contains_unknown_image(provider_json) {
            return Err(AdapterError::Llm(
                "request budget: image content has an unrecognized messages shape".into(),
            ));
        }
        return Ok((shape, 0));
    };

    let mut image_count = 0_u64;
    for message in messages {
        if let Some(images) = message.get("images") {
            if !images.as_array().is_some_and(Vec::is_empty) {
                return Err(AdapterError::Llm(
                    "request budget: image content uses an unsupported provider image field".into(),
                ));
            }
        }
        let Some(content) = message.get_mut("content") else {
            continue;
        };
        match content {
            Value::Array(parts) => {
                for part in parts {
                    sanitize_image_part(part, &mut image_count)?;
                }
            }
            Value::Object(_) => sanitize_image_part(content, &mut image_count)?,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok((shape, image_count))
}

fn image_payload_base64_bytes(provider_json: &Value) -> u64 {
    fn add_data_url_bytes(url: Option<&Value>, bytes: &mut u64) {
        let Some(url) = url
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("data:image/") && url.contains(','))
        else {
            return;
        };
        if let Some((_, payload)) = url.split_once(',') {
            *bytes = bytes.saturating_add(payload.len() as u64);
        }
    }

    fn add_image_part_bytes(part: &Value, bytes: &mut u64) {
        match part.get("type").and_then(Value::as_str) {
            Some("image_url") => add_data_url_bytes(
                part.get("image_url").and_then(|image| image.get("url")),
                bytes,
            ),
            Some("image") => {
                let data = part
                    .get("source")
                    .and_then(|source| source.get("data"))
                    .and_then(Value::as_str)
                    .filter(|data| !data.is_empty());
                if let Some(data) = data {
                    *bytes = bytes.saturating_add(data.len() as u64);
                }
            }
            _ => {}
        }
    }

    let Some(messages) = provider_json.get("messages").and_then(Value::as_array) else {
        return 0;
    };
    let mut bytes: u64 = 0;
    for message in messages {
        if let Some(images) = message.get("images").and_then(Value::as_array) {
            for image in images.iter().filter_map(Value::as_str) {
                bytes = bytes.saturating_add(image.len() as u64);
            }
        }
        let Some(content) = message.get("content") else {
            continue;
        };
        match content {
            Value::Array(parts) => parts
                .iter()
                .for_each(|part| add_image_part_bytes(part, &mut bytes)),
            Value::Object(_) => add_image_part_bytes(content, &mut bytes),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    bytes
}

fn sanitize_image_part(part: &mut Value, image_count: &mut u64) -> AdapterResult<()> {
    let Some(kind) = part.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    match kind {
        "image_url" => {
            let image_url = part
                .get_mut("image_url")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    AdapterError::Llm(
                        "request budget: image_url content is malformed; refusing to estimate it"
                            .into(),
                    )
                })?;
            let url = image_url
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AdapterError::Llm(
                        "request budget: image_url content has no URL; refusing to estimate it"
                            .into(),
                    )
                })?;
            let Some((metadata, payload)) = url.split_once(',') else {
                return Err(AdapterError::Llm(
                    "request budget: remote or malformed image URL is not supported".into(),
                ));
            };
            if !metadata.starts_with("data:image/")
                || !metadata.ends_with(";base64")
                || payload.is_empty()
                || payload.bytes().any(|byte| {
                    !byte.is_ascii_alphanumeric()
                        && !matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_')
                })
            {
                return Err(AdapterError::Llm(
                    "request budget: remote or malformed image URL is not supported".into(),
                ));
            }
            image_url.insert("url".into(), Value::String(IMAGE_BUDGET_PLACEHOLDER.into()));
            *image_count = image_count.saturating_add(1);
            Ok(())
        }
        "image" => Err(AdapterError::Llm(
            "request budget: image content uses an unsupported provider shape".into(),
        )),
        _ => Ok(()),
    }
}

fn visit_image_url_parts_mut(value: &mut Value, mut visit: impl FnMut(&mut Value)) {
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        let Some(content) = message.get_mut("content") else {
            continue;
        };
        match content {
            Value::Array(parts) => {
                for part in parts {
                    if part.get("type").and_then(Value::as_str) == Some("image_url") {
                        visit(part);
                    }
                }
            }
            Value::Object(part)
                if part.get("type").and_then(Value::as_str) == Some("image_url") =>
            {
                visit(content);
            }
            _ => {}
        }
    }
}

fn apply_high_image_detail(value: &mut Value) {
    visit_image_url_parts_mut(value, |part| {
        if let Some(image_url) = part.get_mut("image_url").and_then(Value::as_object_mut) {
            image_url.insert("detail".into(), Value::String("high".into()));
        }
    });
}

fn image_urls_have_high_detail(value: &Value) -> bool {
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return true;
    };
    for message in messages {
        let Some(content) = message.get("content") else {
            continue;
        };
        let parts: Vec<&Value> = match content {
            Value::Array(parts) => parts.iter().collect(),
            Value::Object(_) => vec![content],
            _ => Vec::new(),
        };
        for part in parts {
            if part.get("type").and_then(Value::as_str) != Some("image_url") {
                continue;
            }
            if part
                .get("image_url")
                .and_then(Value::as_object)
                .and_then(|image_url| image_url.get("detail"))
                .and_then(Value::as_str)
                != Some("high")
            {
                return false;
            }
        }
    }
    true
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelProbeResult {
    pub endpoint: String,
    pub protocol: String,
    pub model: String,
    pub latency_ms: u128,
    pub response_preview: String,
}

pub fn provider_endpoint(protocol: ProviderProtocol, mut base_url: Url) -> AdapterResult<Url> {
    let path = base_url.path().trim_end_matches('/');
    let suffix = match protocol {
        ProviderProtocol::OpenAiCompatible if path.ends_with("/v1") => "chat/completions",
        ProviderProtocol::OpenAiCompatible => "v1/chat/completions",
        ProviderProtocol::Anthropic if path.ends_with("/v1") => "messages",
        ProviderProtocol::Anthropic => "v1/messages",
        ProviderProtocol::Ollama if path.ends_with("/api") => "chat",
        ProviderProtocol::Ollama => "api/chat",
    };
    if !base_url.path().ends_with('/') {
        base_url.set_path(&format!("{}/", base_url.path()));
    }
    base_url
        .join(suffix)
        .map_err(|error| AdapterError::Llm(error.to_string()))
}

pub fn provider_models_endpoint(
    protocol: ProviderProtocol,
    mut base_url: Url,
) -> AdapterResult<Url> {
    let path = base_url.path().trim_end_matches('/');
    let suffix = match protocol {
        ProviderProtocol::OpenAiCompatible | ProviderProtocol::Anthropic
            if path.ends_with("/v1") =>
        {
            "models"
        }
        ProviderProtocol::OpenAiCompatible | ProviderProtocol::Anthropic => "v1/models",
        ProviderProtocol::Ollama if path.ends_with("/api") => "tags",
        ProviderProtocol::Ollama => "api/tags",
    };
    if !base_url.path().ends_with('/') {
        base_url.set_path(&format!("{}/", base_url.path()));
    }
    base_url
        .join(suffix)
        .map_err(|error| AdapterError::Llm(error.to_string()))
}

pub fn build_provider_request_with_tools(
    protocol: ProviderProtocol,
    base_url: Url,
    model: &str,
    request: &ProviderModelRequest,
) -> AdapterResult<ProviderRequest> {
    build_provider_request_with_optional_budget(protocol, base_url, model, request, None)
}

/// Build a provider request while reserving the requested output allowance in
/// the provider's native request field. This is the budget-aware counterpart
/// to [`build_provider_request_with_tools`].
pub fn build_provider_request_with_tools_and_budget(
    protocol: ProviderProtocol,
    base_url: Url,
    model: &str,
    request: &ProviderModelRequest,
    budget: RequestBudget,
) -> AdapterResult<ProviderRequest> {
    build_provider_request_with_optional_budget(protocol, base_url, model, request, Some(budget))
}

fn build_provider_request_with_optional_budget(
    protocol: ProviderProtocol,
    base_url: Url,
    model: &str,
    request: &ProviderModelRequest,
    budget: Option<RequestBudget>,
) -> AdapterResult<ProviderRequest> {
    let messages = request
        .messages
        .iter()
        .map(|message| provider_message(protocol, &message.role, &message.content))
        .collect::<AdapterResult<Vec<_>>>()?;
    let tool_aliases = provider_tool_aliases(request);
    let openai_tools = request
        .tools
        .iter()
        .zip(&tool_aliases)
        .map(|(tool, (provider_name, _))| {
            json!({
                "type": "function",
                "function": {
                    "name": provider_name,
                    "description": provider_tool_description(tool, provider_name),
                    "parameters": tool.input_schema
                }
            })
        })
        .collect::<Vec<_>>();

    match protocol {
        ProviderProtocol::OpenAiCompatible => {
            let mut body = json!({
                "model": model,
                "stream": true,
                "stream_options": {"include_usage": true},
                "messages": std::iter::once(json!({"role":"system", "content":request.system}))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "tools": openai_tools
            });
            if let Some(budget) = budget {
                let output_field = if is_official_openai_endpoint(&base_url) {
                    "max_completion_tokens"
                } else {
                    "max_tokens"
                };
                body[output_field] = json!(budget.reserved_output_tokens);
            }
            Ok(ProviderRequest {
                endpoint: provider_endpoint(protocol, base_url)?,
                body,
                requires_credential: true,
            })
        }
        ProviderProtocol::Anthropic => {
            let tools = request
                .tools
                .iter()
                .zip(&tool_aliases)
                .map(|(tool, (provider_name, _))| {
                    json!({
                        "name": provider_name,
                        "description": provider_tool_description(tool, provider_name),
                        "input_schema": tool.input_schema
                    })
                })
                .collect::<Vec<_>>();
            let mut body = json!({
                "model": model,
                "system": request.system,
                "max_tokens": 4096,
                "stream": true,
                "messages": messages,
                "tools": tools
            });
            if let Some(budget) = budget {
                body["max_tokens"] = json!(budget.reserved_output_tokens);
            }
            Ok(ProviderRequest {
                endpoint: provider_endpoint(protocol, base_url)?,
                body,
                requires_credential: true,
            })
        }
        ProviderProtocol::Ollama => {
            let mut body = json!({
                "model": model,
                "stream": true,
                "messages": std::iter::once(json!({"role":"system", "content":request.system}))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "tools": openai_tools
            });
            if let Some(budget) = budget {
                body["options"] = json!({"num_predict": budget.reserved_output_tokens});
            }
            Ok(ProviderRequest {
                endpoint: provider_endpoint(protocol, base_url)?,
                body,
                requires_credential: false,
            })
        }
    }
}

fn is_official_openai_endpoint(base_url: &Url) -> bool {
    base_url.scheme() == "https"
        && base_url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.openai.com"))
        && base_url.port_or_known_default() == Some(443)
}

fn is_trusted_openai_image_endpoint(base_url: &Url) -> bool {
    if !is_official_openai_endpoint(base_url) {
        return false;
    }
    matches!(base_url.path(), "" | "/" | "/v1" | "/v1/")
}

/// Conservative high-detail image bounds for the exact official model IDs
/// whose vision token accounting is known to this adapter. These are model
/// IDs, not families: a provider alias, proxy, or future sibling must fail
/// closed until it receives its own reviewed bound.
fn trusted_image_token_budget(
    protocol: ProviderProtocol,
    base_url: &Url,
    model: &str,
) -> Option<u64> {
    if protocol != ProviderProtocol::OpenAiCompatible || !is_trusted_openai_image_endpoint(base_url)
    {
        return None;
    }
    match model {
        "gpt-6-astra" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" | "gpt-5.5"
        | "gpt-5.4" | "gpt-5.4-mini" | "gpt-5.4-nano" => Some(3_001),
        "gpt-5.2" => Some(7_374),
        "gpt-4.1-mini" | "gpt-4.1-mini-2025-04-14" => Some(9_955),
        "gpt-4o" | "gpt-4.1" | "gpt-4.1-2025-04-14" => Some(1_446),
        "gpt-4o-mini" => Some(48_170),
        "gpt-5.1" => Some(1_191),
        _ => None,
    }
}

/// Credential-free capability check for a model profile before a client is
/// constructed. This deliberately shares the exact host/path/protocol/model
/// policy used by [`UnifiedModelClient::has_image_budget`].
pub fn supports_image_budget(protocol: ProviderProtocol, base_url: &Url, model: &str) -> bool {
    trusted_image_token_budget(protocol, base_url, model).is_some()
}

fn provider_message(
    protocol: ProviderProtocol,
    role: &str,
    content: &ModelMessageContent,
) -> AdapterResult<Value> {
    let ModelMessageContent::Parts(parts) = content else {
        let ModelMessageContent::Text(text) = content else {
            unreachable!()
        };
        return Ok(json!({"role":role,"content":text}));
    };
    let mut text = String::new();
    let mut images = Vec::new();
    for part in parts {
        match part {
            ModelContentPart::Text { text: value } => text.push_str(value),
            ModelContentPart::Image {
                media_type,
                data_base64,
            } => images.push((media_type, data_base64)),
        }
    }
    match protocol {
        ProviderProtocol::OpenAiCompatible => Ok(json!({
            "role":role,
            "content":parts.iter().map(|part| match part {
                ModelContentPart::Text { text } => json!({"type":"text","text":text}),
                ModelContentPart::Image { media_type, data_base64 } => json!({
                    "type":"image_url",
                    "image_url":{"url":format!("data:{media_type};base64,{data_base64}")}
                }),
            }).collect::<Vec<_>>()
        })),
        ProviderProtocol::Anthropic => Ok(json!({
            "role":role,
            "content":parts.iter().map(|part| match part {
                ModelContentPart::Text { text } => json!({"type":"text","text":text}),
                ModelContentPart::Image { media_type, data_base64 } => json!({
                    "type":"image",
                    "source":{"type":"base64","media_type":media_type,"data":data_base64}
                }),
            }).collect::<Vec<_>>()
        })),
        ProviderProtocol::Ollama => Ok(json!({
            "role":role,
            "content":text,
            "images":images.into_iter().map(|(_, data)| data).collect::<Vec<_>>()
        })),
    }
}

fn provider_tool_aliases(request: &ProviderModelRequest) -> Vec<(String, String)> {
    let mut occupied = request
        .tools
        .iter()
        .filter(|tool| provider_tool_name_is_valid(&tool.id))
        .map(|tool| tool.id.clone())
        .collect::<BTreeSet<_>>();
    request
        .tools
        .iter()
        .enumerate()
        .map(|(index, tool)| {
            if provider_tool_name_is_valid(&tool.id) {
                return (tool.id.clone(), tool.id.clone());
            }
            let mut alias = format!("omicsops_tool_{index}");
            while occupied.contains(&alias) {
                alias.push('_');
            }
            occupied.insert(alias.clone());
            (alias, tool.id.clone())
        })
        .collect()
}

fn provider_tool_name_is_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn provider_tool_description(
    tool: &omicsops_agent::provider::ProviderToolSpec,
    provider_name: &str,
) -> String {
    if provider_name == tool.id {
        tool.description.clone()
    } else {
        format!("{} [OmicsOps tool id: {}]", tool.description, tool.id)
    }
}

fn provider_tool_alias_map(request: &ProviderModelRequest) -> BTreeMap<String, String> {
    provider_tool_aliases(request).into_iter().collect()
}

fn canonical_tool_id(aliases: &BTreeMap<String, String>, provider_name: &str) -> String {
    aliases
        .get(provider_name)
        .cloned()
        .unwrap_or_else(|| provider_name.to_owned())
}

pub fn parse_provider_tool_response(
    protocol: ProviderProtocol,
    value: &Value,
) -> AdapterResult<Vec<ProviderStreamEvent>> {
    parse_provider_tool_response_with_aliases(protocol, value, &BTreeMap::new())
}

pub fn parse_provider_tool_response_for_request(
    protocol: ProviderProtocol,
    value: &Value,
    request: &ProviderModelRequest,
) -> AdapterResult<Vec<ProviderStreamEvent>> {
    parse_provider_tool_response_with_aliases(protocol, value, &provider_tool_alias_map(request))
}

fn parse_provider_tool_response_with_aliases(
    protocol: ProviderProtocol,
    value: &Value,
    aliases: &BTreeMap<String, String>,
) -> AdapterResult<Vec<ProviderStreamEvent>> {
    validate_response_end(protocol, value)?;
    let mut events = Vec::new();
    let text = match protocol {
        ProviderProtocol::OpenAiCompatible => value.pointer("/choices/0/message/content"),
        ProviderProtocol::Anthropic => {
            value
                .get("content")
                .and_then(Value::as_array)
                .and_then(|content| {
                    content
                        .iter()
                        .find(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                        .and_then(|item| item.get("text"))
                })
        }
        ProviderProtocol::Ollama => value.pointer("/message/content"),
    }
    .and_then(Value::as_str);
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        events.push(ProviderStreamEvent::TextDelta { text: text.into() });
    }

    let calls = match protocol {
        ProviderProtocol::OpenAiCompatible => value
            .pointer("/choices/0/message/tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        ProviderProtocol::Anthropic => value
            .get("content")
            .and_then(Value::as_array)
            .map(|content| {
                content
                    .iter()
                    .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        ProviderProtocol::Ollama => value
            .pointer("/message/tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    };
    for (position, call) in calls.iter().enumerate() {
        let index = call
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or(position as u64) as u32;
        let (prefix, call_id, tool_id, arguments) = match protocol {
            ProviderProtocol::OpenAiCompatible => (
                "openai",
                call.get("id").and_then(Value::as_str),
                call.pointer("/function/name").and_then(Value::as_str),
                call.pointer("/function/arguments"),
            ),
            ProviderProtocol::Anthropic => (
                "anthropic",
                call.get("id").and_then(Value::as_str),
                call.get("name").and_then(Value::as_str),
                call.get("input"),
            ),
            ProviderProtocol::Ollama => (
                "ollama",
                call.get("id").and_then(Value::as_str),
                call.pointer("/function/name").and_then(Value::as_str),
                call.pointer("/function/arguments"),
            ),
        };
        let Some(tool_id) = tool_id else {
            return Err(AdapterError::Llm(format!(
                "provider tool call at index {index} has no tool name"
            )));
        };
        let call_id = call_id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{prefix}-{index}"));
        events.push(ProviderStreamEvent::ToolCallStarted {
            call_id: call_id.clone(),
            index,
            tool_id: canonical_tool_id(aliases, tool_id),
        });
        if let Some(arguments) = arguments {
            events.push(ProviderStreamEvent::ToolArgumentsDelta {
                call_id: call_id.clone(),
                index,
                arguments: match arguments {
                    Value::String(arguments) => arguments.clone(),
                    arguments => arguments.to_string(),
                },
            });
        }
        events.push(ProviderStreamEvent::ToolCallCompleted { call_id, index });
    }
    if let Some(sample) = usage_sample_from_value(protocol, value, 0, ProviderUsageState::Final) {
        push_usage_events(&mut events, protocol, value, sample);
    }
    if events.is_empty() {
        return Err(AdapterError::Llm(
            "non-streaming response contained neither text nor tool calls".into(),
        ));
    }
    events.push(ProviderStreamEvent::Completed);
    Ok(events)
}

fn usage_value(protocol: ProviderProtocol, value: &Value) -> Option<&Value> {
    let usage = match protocol {
        ProviderProtocol::OpenAiCompatible => value.get("usage")?,
        ProviderProtocol::Anthropic => value
            .get("usage")
            .or_else(|| value.pointer("/message/usage"))?,
        ProviderProtocol::Ollama => value,
    };
    usage.is_object().then_some(usage)
}

fn usage_counter(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn usage_sample_from_value(
    protocol: ProviderProtocol,
    value: &Value,
    sample_index: u32,
    state: ProviderUsageState,
) -> Option<ProviderUsageSample> {
    let usage = usage_value(protocol, value)?;
    let (
        input_tokens,
        context_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        reported_total_tokens,
    ) = match protocol {
        ProviderProtocol::OpenAiCompatible => (
            usage_counter(usage, "prompt_tokens"),
            usage_counter(usage, "prompt_tokens"),
            usage_counter(usage, "completion_tokens"),
            usage
                .get("completion_tokens_details")
                .and_then(|details| usage_counter(details, "reasoning_tokens")),
            usage
                .get("prompt_tokens_details")
                .and_then(|details| usage_counter(details, "cached_tokens")),
            None,
            usage_counter(usage, "total_tokens"),
        ),
        ProviderProtocol::Anthropic => (
            usage_counter(usage, "input_tokens"),
            {
                let input = usage_counter(usage, "input_tokens");
                let cache_read = usage_counter(usage, "cache_read_input_tokens");
                let cache_creation = usage_counter(usage, "cache_creation_input_tokens");
                input.zip(cache_read).zip(cache_creation).and_then(
                    |((input, cache_read), cache_creation)| {
                        input
                            .checked_add(cache_read)
                            .and_then(|total| total.checked_add(cache_creation))
                    },
                )
            },
            usage_counter(usage, "output_tokens"),
            None,
            usage_counter(usage, "cache_read_input_tokens"),
            usage_counter(usage, "cache_creation_input_tokens"),
            None,
        ),
        ProviderProtocol::Ollama => (
            usage_counter(usage, "prompt_eval_count"),
            usage_counter(usage, "prompt_eval_count"),
            usage_counter(usage, "eval_count"),
            None,
            None,
            None,
            None,
        ),
    };
    let has_counter = [
        input_tokens,
        context_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        reported_total_tokens,
    ]
    .iter()
    .any(Option::is_some);
    has_counter.then_some(ProviderUsageSample {
        sample_index,
        aggregation: ProviderUsageAggregation::Cumulative,
        state,
        input_tokens,
        context_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        reported_total_tokens,
    })
}

/// Compatibility view for the old tuple-only unit tests and callers inside
/// this module.  New provider events use `ProviderUsageSample`, so missing
/// counters are not collapsed at the adapter boundary.
#[cfg(test)]
fn usage_from_value(protocol: ProviderProtocol, value: &Value) -> Option<(u64, u64, Value)> {
    let usage = usage_value(protocol, value)?;
    let sample = usage_sample_from_value(protocol, value, 0, ProviderUsageState::Partial)?;
    Some((
        sample.input_tokens.unwrap_or(0),
        sample.output_tokens.unwrap_or(0),
        safe_usage_metadata(protocol, usage),
    ))
}

fn push_usage_events(
    events: &mut Vec<ProviderStreamEvent>,
    protocol: ProviderProtocol,
    value: &Value,
    sample: ProviderUsageSample,
) {
    // Keep the old bounded event for older consumers while making the typed
    // event the source of truth for new accounting. The compatibility event
    // contains only the existing allowlisted metadata and never raw JSON.
    events.push(ProviderStreamEvent::UsageObserved {
        sample: sample.clone(),
    });
    if let Some(usage) = usage_value(protocol, value) {
        events.push(ProviderStreamEvent::Usage {
            input_tokens: sample.input_tokens.unwrap_or(0),
            output_tokens: sample.output_tokens.unwrap_or(0),
            provider_json: safe_usage_metadata(protocol, usage),
        });
    }
}

/// Usage is audit metadata, never a copy of the provider response. Keep a
/// bounded set of numeric counters, including explicitly reported reasoning
/// tokens; these counters do not establish an effective reasoning effort.
fn safe_usage_metadata(protocol: ProviderProtocol, usage: &Value) -> Value {
    fn counters(value: &Value, keys: &[&str]) -> serde_json::Map<String, Value> {
        keys.iter()
            .filter_map(|key| {
                value
                    .get(*key)
                    .and_then(Value::as_u64)
                    .map(|count| ((*key).to_owned(), json!(count)))
            })
            .collect()
    }
    let keys: &[&str] = match protocol {
        ProviderProtocol::OpenAiCompatible => {
            &["prompt_tokens", "completion_tokens", "total_tokens"]
        }
        ProviderProtocol::Anthropic => &[
            "input_tokens",
            "output_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ],
        ProviderProtocol::Ollama => &["prompt_eval_count", "eval_count"],
    };
    let mut safe = counters(usage, keys);
    if protocol == ProviderProtocol::OpenAiCompatible {
        for (name, keys) in [
            (
                "prompt_tokens_details",
                &["cached_tokens", "audio_tokens"][..],
            ),
            (
                "completion_tokens_details",
                &[
                    "reasoning_tokens",
                    "audio_tokens",
                    "accepted_prediction_tokens",
                    "rejected_prediction_tokens",
                ][..],
            ),
        ] {
            if let Some(details) = usage.get(name) {
                let details = counters(details, keys);
                if !details.is_empty() {
                    safe.insert(name.into(), Value::Object(details));
                }
            }
        }
    }
    Value::Object(safe)
}

#[cfg(test)]
mod usage_metadata_tests {
    use super::*;

    #[test]
    fn reasoning_only_stream_packets_preserve_distinct_reasoning_deltas() {
        let secret = "private reasoning must stay out of events";
        for (protocol, packet) in [
            (
                ProviderProtocol::OpenAiCompatible,
                format!(
                    "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":\"{secret}\"}}}}]}}\n\n"
                ),
            ),
            (
                ProviderProtocol::Anthropic,
                format!(
                    "data: {{\"type\":\"content_block_delta\",\"delta\":{{\"thinking\":\"{secret}\"}}}}\n\n"
                ),
            ),
            (
                ProviderProtocol::Ollama,
                format!("{{\"message\":{{\"thinking\":\"{secret}\"}}}}\n"),
            ),
        ] {
            let mut decoder = ProviderToolStreamDecoder::new(protocol);
            let events = decoder.push(packet.as_bytes()).unwrap();
            assert_eq!(
                events,
                vec![ProviderStreamEvent::ReasoningDelta {
                    text: secret.into()
                }]
            );
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, ProviderStreamEvent::TextDelta { .. }))
            );
        }
    }

    #[test]
    fn fragmented_openai_reasoning_is_emitted_before_final_content_and_completion() {
        let mut decoder = ProviderToolStreamDecoder::new(ProviderProtocol::OpenAiCompatible);
        let prefix =
            b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":null,\"reasoning\":\"Inspect";
        assert!(decoder.push(prefix).unwrap().is_empty());
        let reasoning = decoder.push(b"ing inputs\"}}]}\n\n").unwrap();
        assert_eq!(
            reasoning,
            vec![ProviderStreamEvent::ReasoningDelta {
                text: "Inspecting inputs".into()
            }]
        );
        let final_events = decoder.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"Done\"},\"finish_reason\":\"stop\"}]}\n\n").unwrap();
        assert!(
            matches!(final_events.as_slice(), [ProviderStreamEvent::TextDelta { text }, ProviderStreamEvent::Completed] if text == "Done")
        );
    }

    fn only_usage(events: &[ProviderStreamEvent]) -> (u64, u64, Value) {
        let usage: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ProviderStreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                    provider_json,
                } => Some((*input_tokens, *output_tokens, provider_json.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(usage.len(), 1);
        usage[0].clone()
    }

    #[test]
    fn stream_and_non_streaming_usage_exclude_provider_payloads() {
        for (protocol, response, expected) in [
            (
                ProviderProtocol::OpenAiCompatible,
                json!({"choices":[{"message":{"content":"answer"},"delta":{"content":"answer"}}],"usage":{"prompt_tokens":12,"completion_tokens":8,"total_tokens":20,"private":"sentinel","completion_tokens_details":{"reasoning_tokens":3,"private":"sentinel"},"prompt_tokens_details":{"cached_tokens":0,"private":"sentinel"}}}),
                json!({"prompt_tokens":12,"completion_tokens":8,"total_tokens":20,"completion_tokens_details":{"reasoning_tokens":3},"prompt_tokens_details":{"cached_tokens":0}}),
            ),
            (
                ProviderProtocol::Anthropic,
                json!({"type":"message_delta","content":[{"type":"text","text":"answer"}],"usage":{"input_tokens":12,"output_tokens":8,"cache_read_input_tokens":4,"private":"sentinel"}}),
                json!({"input_tokens":12,"output_tokens":8,"cache_read_input_tokens":4}),
            ),
            (
                ProviderProtocol::Ollama,
                json!({"message":{"content":"sentinel answer","thinking":"sentinel"},"done":true,"prompt_eval_count":12,"eval_count":8,"context":[999],"private":"sentinel"}),
                json!({"prompt_eval_count":12,"eval_count":8}),
            ),
        ] {
            let parsed = parse_provider_tool_response(protocol, &response).unwrap();
            assert_eq!(only_usage(&parsed), (12, 8, expected.clone()));
            let wire = match protocol {
                ProviderProtocol::Ollama => format!("{response}\n"),
                ProviderProtocol::OpenAiCompatible => {
                    format!("data: {response}\n\ndata: [DONE]\n\n")
                }
                ProviderProtocol::Anthropic => {
                    format!("data: {response}\n\ndata: {{\"type\":\"message_stop\"}}\n\n")
                }
            };
            let mut decoder = ProviderToolStreamDecoder::new(protocol);
            let mut events = Vec::new();
            for chunk in wire.as_bytes().chunks(7) {
                events.extend(decoder.push(chunk).unwrap());
            }
            events.extend(decoder.finish().unwrap());
            let usage = only_usage(&events);
            assert_eq!(usage, (12, 8, expected));
            assert!(!usage.2.to_string().contains("sentinel"));
        }
    }

    #[test]
    fn rejects_non_integer_metadata_and_preserves_explicit_zero_details() {
        let value = json!({"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":"secret","completion_tokens_details":{"reasoning_tokens":0,"audio_tokens":-1,"accepted_prediction_tokens":1.5,"rejected_prediction_tokens":{"secret":true}},"prompt_tokens_details":"secret"}});
        let (_, _, safe) = usage_from_value(ProviderProtocol::OpenAiCompatible, &value).unwrap();
        assert_eq!(
            safe,
            json!({"prompt_tokens":4,"completion_tokens":2,"completion_tokens_details":{"reasoning_tokens":0}})
        );
        for invalid in [
            Value::Null,
            json!("secret"),
            json!({"prompt_tokens":-1,"completion_tokens":"secret"}),
        ] {
            assert!(
                usage_from_value(
                    ProviderProtocol::OpenAiCompatible,
                    &json!({"usage":invalid})
                )
                .is_none()
            );
        }
        let (_, _, missing) = usage_from_value(
            ProviderProtocol::OpenAiCompatible,
            &json!({"usage":{"prompt_tokens":1}}),
        )
        .unwrap();
        assert!(missing.get("completion_tokens_details").is_none());
    }

    #[test]
    fn anthropic_message_start_and_partial_updates_stay_separate() {
        let initial =
            json!({"message":{"usage":{"input_tokens":10,"output_tokens":1,"private":"sentinel"}}});
        let delta = json!({"usage":{"output_tokens":7,"private":"sentinel"}});
        assert_eq!(
            usage_from_value(ProviderProtocol::Anthropic, &initial).unwrap(),
            (10, 1, json!({"input_tokens":10,"output_tokens":1}))
        );
        assert_eq!(
            usage_from_value(ProviderProtocol::Anthropic, &delta).unwrap(),
            (0, 7, json!({"output_tokens":7}))
        );
    }
}

#[derive(Debug, Clone)]
pub struct ProviderToolStreamDecoder {
    protocol: ProviderProtocol,
    pending: Vec<u8>,
    active_calls: BTreeMap<u32, (String, String)>,
    tool_aliases: BTreeMap<String, String>,
    next_usage_sample_index: u32,
    saw_completion: bool,
}

impl ProviderToolStreamDecoder {
    pub fn new(protocol: ProviderProtocol) -> Self {
        Self {
            protocol,
            pending: Vec::new(),
            active_calls: BTreeMap::new(),
            tool_aliases: BTreeMap::new(),
            next_usage_sample_index: 0,
            saw_completion: false,
        }
    }

    pub fn for_request(protocol: ProviderProtocol, request: &ProviderModelRequest) -> Self {
        Self {
            protocol,
            pending: Vec::new(),
            active_calls: BTreeMap::new(),
            tool_aliases: provider_tool_alias_map(request),
            next_usage_sample_index: 0,
            saw_completion: false,
        }
    }

    fn usage_sample(&mut self, value: &Value) -> Option<ProviderUsageSample> {
        let sample_index = self.next_usage_sample_index;
        let state = match self.protocol {
            ProviderProtocol::OpenAiCompatible => {
                let terminal = value
                    .pointer("/choices/0/finish_reason")
                    .is_some_and(|reason| !reason.is_null())
                    || self.saw_completion;
                if terminal {
                    ProviderUsageState::Final
                } else {
                    ProviderUsageState::Partial
                }
            }
            ProviderProtocol::Anthropic => value
                .get("type")
                .and_then(Value::as_str)
                .filter(|kind| *kind == "message_stop")
                .map(|_| ProviderUsageState::Final)
                .or_else(|| {
                    value
                        .get("type")
                        .and_then(Value::as_str)
                        .filter(|kind| *kind == "message_delta")
                        .and_then(|_| {
                            value
                                .pointer("/delta/stop_reason")
                                .or_else(|| value.get("stop_reason"))
                        })
                        .and_then(Value::as_str)
                        .map(|_| ProviderUsageState::Final)
                })
                .or_else(|| self.saw_completion.then_some(ProviderUsageState::Final))
                .unwrap_or(ProviderUsageState::Partial),
            ProviderProtocol::Ollama => value
                .get("done")
                .and_then(Value::as_bool)
                .filter(|done| *done)
                .map(|_| ProviderUsageState::Final)
                .or_else(|| self.saw_completion.then_some(ProviderUsageState::Final))
                .unwrap_or(ProviderUsageState::Partial),
        };
        let sample = usage_sample_from_value(self.protocol, value, sample_index, state);
        if sample.is_some() {
            self.next_usage_sample_index = self.next_usage_sample_index.saturating_add(1);
        }
        sample
    }

    pub fn push(&mut self, chunk: &[u8]) -> AdapterResult<Vec<ProviderStreamEvent>> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line_bytes = self.pending.drain(..=newline).collect::<Vec<_>>();
            let line = std::str::from_utf8(&line_bytes[..newline])
                .map_err(|error| {
                    AdapterError::Llm(format!("model stream returned invalid UTF-8: {error}"))
                })?
                .trim_end_matches('\r')
                .trim()
                .to_owned();
            if line.is_empty() || line.starts_with("event:") {
                continue;
            }
            let data = if self.protocol == ProviderProtocol::Ollama {
                line.as_str()
            } else if let Some(data) = line.strip_prefix("data:") {
                data.trim()
            } else {
                continue;
            };
            if data == "[DONE]" {
                self.saw_completion = true;
                for (index, (call_id, _)) in &self.active_calls {
                    events.push(ProviderStreamEvent::ToolCallCompleted {
                        call_id: call_id.clone(),
                        index: *index,
                    });
                }
                events.push(ProviderStreamEvent::Completed);
                continue;
            }
            let value: Value = serde_json::from_str(data)?;
            validate_response_end(self.protocol, &value)?;
            match self.protocol {
                ProviderProtocol::OpenAiCompatible => {
                    self.push_openai(&value, &mut events)?;
                }
                ProviderProtocol::Anthropic => {
                    self.push_anthropic(&value, &mut events)?;
                }
                ProviderProtocol::Ollama => {
                    self.push_ollama(&value, &mut events)?;
                }
            }
            if events
                .iter()
                .any(|event| matches!(event, ProviderStreamEvent::Completed))
            {
                self.saw_completion = true;
            }
        }
        self.saw_completion |= events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::Completed));
        Ok(events)
    }

    fn push_openai(
        &mut self,
        value: &Value,
        events: &mut Vec<ProviderStreamEvent>,
    ) -> AdapterResult<()> {
        if let Some(text) = value
            .pointer("/choices/0/delta/reasoning_content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .or_else(|| {
                value
                    .pointer("/choices/0/delta/reasoning")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
            })
        {
            events.push(ProviderStreamEvent::ReasoningDelta { text: text.into() });
        }
        if let Some(text) = value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            events.push(ProviderStreamEvent::TextDelta { text: text.into() });
        }
        if let Some(calls) = value
            .pointer("/choices/0/delta/tool_calls")
            .and_then(Value::as_array)
        {
            for (position, call) in calls.iter().enumerate() {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(position as u64) as u32;
                let prior = self.active_calls.get(&index).cloned();
                let call_id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| prior.as_ref().map(|entry| entry.0.clone()))
                    .unwrap_or_else(|| format!("openai-{index}"));
                let tool_id = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .map(|name| canonical_tool_id(&self.tool_aliases, name))
                    .or_else(|| prior.as_ref().map(|entry| entry.1.clone()))
                    .unwrap_or_default();
                if prior.is_none() && !tool_id.is_empty() {
                    events.push(ProviderStreamEvent::ToolCallStarted {
                        call_id: call_id.clone(),
                        index,
                        tool_id: tool_id.clone(),
                    });
                }
                self.active_calls.insert(index, (call_id.clone(), tool_id));
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str)
                    && !arguments.is_empty()
                {
                    events.push(ProviderStreamEvent::ToolArgumentsDelta {
                        call_id,
                        index,
                        arguments: arguments.into(),
                    });
                }
            }
        }
        if let Some(sample) = self.usage_sample(value) {
            push_usage_events(events, self.protocol, value, sample);
        }
        if value
            .pointer("/choices/0/finish_reason")
            .is_some_and(|reason| !reason.is_null())
        {
            for (index, (call_id, _)) in &self.active_calls {
                events.push(ProviderStreamEvent::ToolCallCompleted {
                    call_id: call_id.clone(),
                    index: *index,
                });
            }
            events.push(ProviderStreamEvent::Completed);
        }
        Ok(())
    }

    fn push_anthropic(
        &mut self,
        value: &Value,
        events: &mut Vec<ProviderStreamEvent>,
    ) -> AdapterResult<()> {
        match value.get("type").and_then(Value::as_str) {
            Some("content_block_start")
                if value.pointer("/content_block/type").and_then(Value::as_str)
                    == Some("tool_use") =>
            {
                let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                let call_id = value
                    .pointer("/content_block/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("anthropic-{index}"));
                let provider_name = value
                    .pointer("/content_block/name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AdapterError::Llm("Anthropic tool block has no name".into()))?;
                let tool_id = canonical_tool_id(&self.tool_aliases, provider_name);
                self.active_calls
                    .insert(index, (call_id.clone(), tool_id.clone()));
                events.push(ProviderStreamEvent::ToolCallStarted {
                    call_id,
                    index,
                    tool_id,
                });
            }
            Some("content_block_delta") => {
                if let Some(text) = value
                    .pointer("/delta/thinking")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    events.push(ProviderStreamEvent::ReasoningDelta { text: text.into() });
                }
                if let Some(arguments) =
                    value.pointer("/delta/partial_json").and_then(Value::as_str)
                {
                    let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                    let (call_id, _) = self.active_calls.get(&index).ok_or_else(|| {
                        AdapterError::Llm(format!(
                            "Anthropic arguments arrived before tool block {index}"
                        ))
                    })?;
                    events.push(ProviderStreamEvent::ToolArgumentsDelta {
                        call_id: call_id.clone(),
                        index,
                        arguments: arguments.into(),
                    });
                } else if let Some(text) = value.pointer("/delta/text").and_then(Value::as_str) {
                    events.push(ProviderStreamEvent::TextDelta { text: text.into() });
                }
            }
            Some("content_block_stop") => {
                let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                if let Some((call_id, _)) = self.active_calls.get(&index) {
                    events.push(ProviderStreamEvent::ToolCallCompleted {
                        call_id: call_id.clone(),
                        index,
                    });
                }
            }
            Some("message_stop") => events.push(ProviderStreamEvent::Completed),
            _ => {}
        }
        if let Some(sample) = self.usage_sample(value) {
            push_usage_events(events, self.protocol, value, sample);
        }
        Ok(())
    }

    fn push_ollama(
        &mut self,
        value: &Value,
        events: &mut Vec<ProviderStreamEvent>,
    ) -> AdapterResult<()> {
        if let Some(text) = value
            .pointer("/message/thinking")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            events.push(ProviderStreamEvent::ReasoningDelta { text: text.into() });
        }
        if let Some(text) = value
            .pointer("/message/content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            events.push(ProviderStreamEvent::TextDelta { text: text.into() });
        }
        if let Some(calls) = value
            .pointer("/message/tool_calls")
            .and_then(Value::as_array)
        {
            for (position, call) in calls.iter().enumerate() {
                let index = position as u32;
                let call_id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("ollama-{index}"));
                let provider_name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AdapterError::Llm("Ollama tool call has no name".into()))?;
                let tool_id = canonical_tool_id(&self.tool_aliases, provider_name);
                if !self.active_calls.contains_key(&index) {
                    events.push(ProviderStreamEvent::ToolCallStarted {
                        call_id: call_id.clone(),
                        index,
                        tool_id: tool_id.clone(),
                    });
                }
                self.active_calls.insert(index, (call_id.clone(), tool_id));
                if let Some(arguments) = call.pointer("/function/arguments") {
                    events.push(ProviderStreamEvent::ToolArgumentsDelta {
                        call_id,
                        index,
                        arguments: match arguments {
                            Value::String(arguments) => arguments.clone(),
                            arguments => arguments.to_string(),
                        },
                    });
                }
            }
        }
        if let Some(sample) = self.usage_sample(value) {
            push_usage_events(events, self.protocol, value, sample);
        }
        if value.get("done").and_then(Value::as_bool) == Some(true) {
            for (index, (call_id, _)) in &self.active_calls {
                events.push(ProviderStreamEvent::ToolCallCompleted {
                    call_id: call_id.clone(),
                    index: *index,
                });
            }
            events.push(ProviderStreamEvent::Completed);
        }
        Ok(())
    }

    pub fn finish(&mut self) -> AdapterResult<Vec<ProviderStreamEvent>> {
        let events = if self.pending.iter().all(u8::is_ascii_whitespace) {
            Vec::new()
        } else {
            self.pending.push(b'\n');
            self.push(&[])?
        };
        if !self.saw_completion {
            return Err(AdapterError::Llm("incomplete model stream: no terminal provider event; tool calls were not dispatched".into()));
        }
        Ok(events)
    }
}

fn validate_response_end(protocol: ProviderProtocol, value: &Value) -> AdapterResult<()> {
    let reason = response_end_reason(protocol, value);
    match reason {
        Some("length" | "max_tokens" | "model_context_window_exceeded") => Err(AdapterError::Llm(
            "truncated_output: provider exhausted its output allowance; partial tool calls cannot execute".into())),
        Some("content_filter" | "refusal") => Err(AdapterError::Llm(
            "unsuccessful_model_response: provider did not finish the requested response".into())),
        _ => Ok(()),
    }
}

fn response_end_reason<'a>(protocol: ProviderProtocol, value: &'a Value) -> Option<&'a str> {
    match protocol {
        ProviderProtocol::OpenAiCompatible => value.pointer("/choices/0/finish_reason"),
        ProviderProtocol::Anthropic => value
            .get("stop_reason")
            .or_else(|| value.pointer("/delta/stop_reason")),
        ProviderProtocol::Ollama => value.get("done_reason"),
    }
    .and_then(Value::as_str)
}

fn validate_probe_response_end(protocol: ProviderProtocol, value: &Value) -> AdapterResult<()> {
    if matches!(
        response_end_reason(protocol, value),
        Some("length" | "max_tokens" | "model_context_window_exceeded")
    ) {
        return Err(AdapterError::Llm(
            "probe_response_incomplete: model endpoint responded but exhausted the probe output allowance before completing its test reply".into(),
        ));
    }
    validate_response_end(protocol, value)?;
    let has_tool_payload =
        match protocol {
            ProviderProtocol::OpenAiCompatible => {
                value
                    .pointer("/choices/0/message/tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty())
                    || value
                        .pointer("/choices/0/message/function_call")
                        .is_some_and(|call| !call.is_null())
            }
            ProviderProtocol::Anthropic => value
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part.get("type").and_then(Value::as_str) == Some("tool_use"))
                }),
            ProviderProtocol::Ollama => value
                .pointer("/message/tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|calls| !calls.is_empty()),
        };
    let completed = match protocol {
        ProviderProtocol::OpenAiCompatible => response_end_reason(protocol, value) == Some("stop"),
        ProviderProtocol::Anthropic => matches!(
            response_end_reason(protocol, value),
            Some("end_turn" | "stop_sequence")
        ),
        ProviderProtocol::Ollama => {
            value.get("done").and_then(Value::as_bool) == Some(true)
                && matches!(response_end_reason(protocol, value), None | Some("stop"))
        }
    };
    if has_tool_payload || !completed {
        return Err(AdapterError::Llm(
            "probe_response_incomplete: model endpoint responded without a complete text-only test reply".into(),
        ));
    }
    Ok(())
}

fn probe_response_text<'a>(protocol: ProviderProtocol, value: &'a Value) -> Option<&'a str> {
    let text = match protocol {
        ProviderProtocol::OpenAiCompatible => value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str),
        ProviderProtocol::Anthropic => value
            .get("content")
            .and_then(Value::as_array)
            .and_then(|parts| {
                parts
                    .iter()
                    .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            })
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str),
        ProviderProtocol::Ollama => value.pointer("/message/content").and_then(Value::as_str),
    }?;
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

#[derive(Debug, Clone)]
pub struct UnifiedModelClient {
    profile_id: Uuid,
    protocol: ProviderProtocol,
    base_url: Url,
    model: String,
    credential: Option<String>,
    session_id: Uuid,
    request_budget: Option<RequestBudget>,
    reasoning_effort: Option<String>,
    fast_mode: Option<bool>,
    http: reqwest::Client,
}

impl UnifiedModelClient {
    fn authenticate(&self, mut builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder = builder.header(reqwest::header::ACCEPT_ENCODING, "identity");
        if is_opencode_go_base_url(&self.base_url) {
            builder = builder
                .header(reqwest::header::USER_AGENT, OPENCODE_GO_USER_AGENT)
                .header("x-opencode-session", self.session_id.to_string());
        }
        match self.protocol {
            ProviderProtocol::Anthropic => builder
                .header("x-api-key", self.credential.as_deref().unwrap_or_default())
                .header("anthropic-version", "2023-06-01"),
            ProviderProtocol::OpenAiCompatible => {
                builder.bearer_auth(self.credential.as_deref().unwrap_or_default())
            }
            ProviderProtocol::Ollama => builder,
        }
    }

    pub fn new(
        profile_id: Uuid,
        protocol: ProviderProtocol,
        base_url: Url,
        model: impl Into<String>,
        credential: Option<String>,
    ) -> AdapterResult<Self> {
        if protocol != ProviderProtocol::Ollama
            && credential
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(AdapterError::Credential(
                "model provider credential is required".into(),
            ));
        }
        let opencode_go = is_opencode_go_base_url(&base_url);
        Ok(Self {
            profile_id,
            protocol,
            base_url,
            model: model.into(),
            credential,
            session_id: Uuid::new_v4(),
            request_budget: None,
            reasoning_effort: None,
            fast_mode: None,
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(MODEL_REQUEST_TIMEOUT)
                .redirect(if opencode_go {
                    reqwest::redirect::Policy::none()
                } else {
                    reqwest::redirect::Policy::limited(10)
                })
                .build()
                .map_err(|error| AdapterError::Llm(error.to_string()))?,
        })
    }

    /// Bind requests to a stable opaque conversation identity.
    pub fn with_session_id(mut self, session_id: Uuid) -> Self {
        self.session_id = session_id;
        self
    }

    fn post_json_request(&self, endpoint: Url, body: &Value) -> reqwest::RequestBuilder {
        self.authenticate(self.http.post(endpoint).json(body))
    }

    fn get_request(&self, endpoint: Url) -> reqwest::RequestBuilder {
        self.authenticate(self.http.get(endpoint))
    }

    /// Attach a preflight budget to subsequent model generation requests.
    /// Existing callers that do not opt in retain the historical request
    /// shape and behavior.
    pub fn with_request_budget(mut self, budget: RequestBudget) -> Self {
        self.request_budget = Some(budget);
        self
    }

    pub fn request_budget(&self) -> Option<RequestBudget> {
        self.request_budget
    }

    /// Send an explicit wire value unchanged. This does not assert that a
    /// particular model or gateway supports it, nor silently downgrade it.
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> AdapterResult<Self> {
        use omicsops_core::workspace::{ModelProviderKind, validate_reasoning_effort};
        let provider = match self.protocol {
            ProviderProtocol::OpenAiCompatible => ModelProviderKind::OpenAiCompatible,
            ProviderProtocol::Anthropic => ModelProviderKind::Anthropic,
            ProviderProtocol::Ollama => ModelProviderKind::Ollama,
        };
        validate_reasoning_effort(provider, effort.as_deref())
            .map_err(|error| AdapterError::Llm(error.into()))?;
        self.reasoning_effort = effort;
        Ok(self)
    }

    /// Attach the optional Fast mode setting. The wire field is emitted only
    /// for the exact reviewed OpenAI endpoint and model; unsupported profiles
    /// therefore retain the historical request shape.
    pub fn with_fast_mode(mut self, fast_mode: Option<bool>) -> Self {
        self.fast_mode = fast_mode;
        self
    }

    fn apply_reasoning_effort(&self, body: &mut Value) {
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }
    }

    fn apply_fast_mode(&self, body: &mut Value) -> AdapterResult<()> {
        let Some(fast_mode) = self.fast_mode else {
            return Ok(());
        };
        if self.protocol != ProviderProtocol::OpenAiCompatible
            || !omicsops_core::workspace::supports_fast_mode(
                omicsops_core::workspace::ModelProviderKind::OpenAiCompatible,
                self.base_url.as_str(),
                &self.model,
            )
        {
            if fast_mode {
                return Err(AdapterError::Llm(
                    "explicit Fast mode requires an exact supported OpenAI endpoint and model"
                        .into(),
                ));
            }
            return Ok(());
        }
        body["service_tier"] = json!(if fast_mode { "priority" } else { "default" });
        Ok(())
    }

    fn build_provider_request(
        &self,
        request: &ProviderModelRequest,
    ) -> AdapterResult<ProviderRequest> {
        let mut built = build_provider_request_with_optional_budget(
            self.protocol,
            self.base_url.clone(),
            &self.model,
            request,
            self.request_budget,
        )?;
        self.apply_reasoning_effort(&mut built.body);
        self.apply_fast_mode(&mut built.body)?;
        if self.image_token_budget().is_some() {
            apply_high_image_detail(&mut built.body);
        }
        Ok(built)
    }

    fn image_token_budget(&self) -> Option<u64> {
        trusted_image_token_budget(self.protocol, &self.base_url, &self.model)
    }

    /// Whether this exact provider protocol, API host, base path, and model
    /// have a reviewed high-detail image bound. Callers can use this before
    /// persisting an image-capable request so an unknown model cannot be
    /// treated as vision-capable by inference.
    pub fn has_image_budget(&self) -> bool {
        supports_image_budget(self.protocol, &self.base_url, &self.model)
    }

    /// Measure the same provider-shaped body that will be sent. The full JSON
    /// length is exact for this serialized request; the text shape is the
    /// conservative byte-only estimate after image payload replacement, and
    /// image tokens use the reviewed per-image bound for this exact endpoint
    /// and model.
    pub fn measure_provider_request(
        &self,
        provider_request: &ProviderRequest,
    ) -> AdapterResult<RequestBudgetMetrics> {
        self.validate_provider_body(&provider_request.body)?;
        let serialized_request_bytes = serde_json::to_vec(&provider_request.body)
            .map_err(|error| AdapterError::Llm(format!("request budget: {error}")))?
            .len() as u64;
        let (text_shape, image_count) = image_budget_shape(&provider_request.body)?;
        let text_shape_bytes = serde_json::to_vec(&text_shape)
            .map_err(|error| AdapterError::Llm(format!("request budget: {error}")))?
            .len() as u64;
        let image_count = u32::try_from(image_count).map_err(|_| {
            AdapterError::Llm("request budget: image count exceeds the supported bound".into())
        })?;
        let image_bound_tokens = if image_count == 0 {
            Some(0)
        } else {
            self.image_token_budget()
                .and_then(|per_image| per_image.checked_mul(u64::from(image_count)))
        };
        Ok(RequestBudgetMetrics {
            serialized_request_bytes,
            text_shape_bytes,
            image_payload_bytes: image_payload_base64_bytes(&provider_request.body),
            image_count,
            image_bound_tokens,
        })
    }

    /// Measure a model-shaped request after applying the same provider
    /// formatting and reviewed image detail that the send path uses.
    pub fn measure_model_request(
        &self,
        request: &ProviderModelRequest,
    ) -> AdapterResult<RequestBudgetMetrics> {
        let provider_request = self.build_provider_request(request)?;
        self.measure_provider_request(&provider_request)
    }

    fn validate_provider_body(&self, body: &Value) -> AdapterResult<()> {
        if body.get("model").and_then(Value::as_str) != Some(self.model.as_str()) {
            return Err(AdapterError::Llm(format!(
                "request budget: provider body model does not match selected model {}",
                self.model
            )));
        }

        if contains_unknown_image(body) {
            let per_image_tokens = self.image_token_budget().ok_or_else(|| {
                AdapterError::Llm(
                    "request budget: image token cost is unknown for this protocol, API host, base path, or exact model; refusing to send".into(),
                )
            })?;
            if !image_urls_have_high_detail(body) {
                return Err(AdapterError::Llm(
                    "request budget: trusted image requests must use detail high".into(),
                ));
            }
            if let Some(budget) = self.request_budget {
                budget.validate_provider_json_with_image_budget(body, per_image_tokens)?;
            } else {
                // Validate the wire shape even when no context reservation is
                // configured. The actual image payload remains untouched.
                image_budget_shape(body)?;
            }
            return Ok(());
        }

        if let Some(budget) = self.request_budget {
            budget.validate_provider_json(body)?;
        }
        Ok(())
    }

    /// Validate the complete provider-shaped request before any network I/O.
    pub fn validate_request(&self, request: &ProviderModelRequest) -> AdapterResult<()> {
        let provider_request = self.build_provider_request(request)?;
        self.validate_provider_body(&provider_request.body)
    }

    async fn send_with_retry_provider(
        &self,
        endpoint: &Url,
        body: &Value,
        on_event: &mut impl FnMut(ProviderStreamEvent),
    ) -> AdapterResult<reqwest::Response> {
        // Keep this check at the final send boundary so retries and the
        // non-streaming fallback cannot bypass the same preflight.
        self.validate_provider_body(body)?;
        let mut retries = 0_u8;
        loop {
            let result = self.post_json_request(endpoint.clone(), body).send().await;
            match result {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => {
                    let status = response.status();
                    let response_body = response.text().await.unwrap_or_default();
                    if retryable_model_status(status) && retries < MODEL_MAX_RETRIES {
                        retries += 1;
                        let delay_ms = 1_000_u64 << (retries - 1);
                        on_event(ProviderStreamEvent::Retrying {
                            attempt: retries,
                            delay_ms,
                            message: format!("model provider returned {status}"),
                        });
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                    let preview = response_body.chars().take(1_200).collect::<String>();
                    return Err(AdapterError::Llm(format!("{status}: {preview}")));
                }
                Err(error) if retries < MODEL_MAX_RETRIES => {
                    retries += 1;
                    let delay_ms = 1_000_u64 << (retries - 1);
                    on_event(ProviderStreamEvent::Retrying {
                        attempt: retries,
                        delay_ms,
                        message: format!("model provider connection failed: {error}"),
                    });
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                Err(error) => {
                    return Err(AdapterError::Llm(format!("{endpoint}: {error}")));
                }
            }
        }
    }

    pub async fn stream_with_provider(
        &self,
        request: ProviderModelRequest,
        mut on_event: impl FnMut(ProviderStreamEvent),
    ) -> AdapterResult<()> {
        let provider_request = self.build_provider_request(&request)?;
        self.validate_provider_body(&provider_request.body)?;
        let response = self
            .send_with_retry_provider(
                &provider_request.endpoint,
                &provider_request.body,
                &mut on_event,
            )
            .await?;
        let mut decoder = ProviderToolStreamDecoder::for_request(self.protocol, &request);
        let mut bytes = response.bytes_stream();
        let mut emitted_event = false;
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(stream_error) if !emitted_event => {
                    let mut fallback_body = provider_request.body.clone();
                    fallback_body["stream"] = Value::Bool(false);
                    let fallback = self
                        .send_with_retry_provider(
                            &provider_request.endpoint,
                            &fallback_body,
                            &mut on_event,
                        )
                        .await
                        .map_err(|fallback_error| {
                            AdapterError::Llm(format!(
                                "{} stream failed ({stream_error}); non-streaming fallback failed: {fallback_error}",
                                provider_request.endpoint
                            ))
                        })?;
                    let body = fallback.text().await.map_err(|fallback_error| {
                        AdapterError::Llm(format!(
                            "{} stream failed ({stream_error}); error reading non-streaming fallback: {fallback_error}",
                            provider_request.endpoint
                        ))
                    })?;
                    let value: Value = serde_json::from_str(&body).map_err(|fallback_error| {
                        AdapterError::Llm(format!(
                            "{} stream failed ({stream_error}); non-streaming fallback returned invalid JSON: {fallback_error}",
                            provider_request.endpoint
                        ))
                    })?;
                    for event in
                        parse_provider_tool_response_for_request(self.protocol, &value, &request)?
                    {
                        on_event(event);
                    }
                    return Ok(());
                }
                Err(error) => {
                    return Err(AdapterError::Llm(format!(
                        "{} stream ended after partial output: {error}",
                        provider_request.endpoint
                    )));
                }
            };
            for event in decoder.push(&chunk)? {
                emitted_event = true;
                on_event(event);
            }
        }
        for event in decoder.finish()? {
            on_event(event);
        }
        Ok(())
    }

    /// Execute exactly one streaming provider request.
    ///
    /// Side-chat is an accepted durable operation with its own request ID, so
    /// it must never inherit the normal model retry or non-streaming fallback
    /// policy.  Keep the method separate from `stream_with_provider` so the
    /// established Agent callers retain their existing retry behavior.
    pub async fn stream_with_provider_once(
        &self,
        request: ProviderModelRequest,
        mut on_event: impl FnMut(ProviderStreamEvent),
    ) -> AdapterResult<()> {
        let provider_request = self.build_provider_request(&request)?;
        self.validate_provider_body(&provider_request.body)?;
        let response = self
            .post_json_request(provider_request.endpoint.clone(), &provider_request.body)
            .send()
            .await
            .map_err(|_| {
                AdapterError::Llm(format!(
                    "{}: side-chat provider request failed",
                    provider_request.endpoint
                ))
            })?;
        if !response.status().is_success() {
            return Err(AdapterError::Llm(format!(
                "{} returned {}",
                provider_request.endpoint,
                response.status()
            )));
        }

        let mut decoder = ProviderToolStreamDecoder::for_request(self.protocol, &request);
        let mut bytes = response.bytes_stream();
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|_| {
                AdapterError::Llm(format!(
                    "{}: side-chat provider stream failed",
                    provider_request.endpoint
                ))
            })?;
            for event in decoder.push(&chunk)? {
                on_event(event);
            }
        }
        for event in decoder.finish()? {
            on_event(event);
        }
        Ok(())
    }

    fn probe_body(&self) -> AdapterResult<Value> {
        let mut body = match self.protocol {
            ProviderProtocol::OpenAiCompatible => json!({
                "model": self.model,
                "stream": false,
                "max_tokens": MODEL_PROBE_OUTPUT_TOKENS,
                "messages": [{"role":"user", "content":"Reply with OK."}]
            }),
            ProviderProtocol::Anthropic => json!({
                "model": self.model,
                "stream": false,
                "max_tokens": MODEL_PROBE_OUTPUT_TOKENS,
                "messages": [{"role":"user", "content":"Reply with OK."}]
            }),
            ProviderProtocol::Ollama => json!({
                "model": self.model,
                "stream": false,
                "options": {"num_predict": MODEL_PROBE_OUTPUT_TOKENS},
                "messages": [{"role":"user", "content":"Reply with OK."}]
            }),
        };
        self.apply_reasoning_effort(&mut body);
        self.apply_fast_mode(&mut body)?;
        if self.protocol == ProviderProtocol::OpenAiCompatible
            && is_official_openai_endpoint(&self.base_url)
        {
            body.as_object_mut().unwrap().remove("max_tokens");
            body["max_completion_tokens"] = json!(MODEL_PROBE_OUTPUT_TOKENS);
        }
        if let Some(budget) = self.request_budget {
            let probe_budget = RequestBudget {
                reserved_output_tokens: budget
                    .reserved_output_tokens
                    .min(MODEL_PROBE_OUTPUT_TOKENS),
                ..budget
            };
            match self.protocol {
                ProviderProtocol::OpenAiCompatible
                    if is_official_openai_endpoint(&self.base_url) =>
                {
                    body.as_object_mut().unwrap().remove("max_tokens");
                    body["max_completion_tokens"] = json!(probe_budget.reserved_output_tokens);
                }
                ProviderProtocol::OpenAiCompatible | ProviderProtocol::Anthropic => {
                    body["max_tokens"] = json!(probe_budget.reserved_output_tokens);
                }
                ProviderProtocol::Ollama => {
                    body["options"] = json!({"num_predict": probe_budget.reserved_output_tokens});
                }
            }
            probe_budget.validate_provider_json(&body)?;
        }
        Ok(body)
    }

    pub async fn probe(&self) -> AdapterResult<ModelProbeResult> {
        let endpoint = provider_endpoint(self.protocol, self.base_url.clone())?;
        let body = self.probe_body()?;
        let builder = self.post_json_request(endpoint.clone(), &body);
        let started = Instant::now();
        let response = builder
            .send()
            .await
            .map_err(|error| AdapterError::Llm(format!("{}: {error}", endpoint)))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| AdapterError::Llm(format!("{}: {error}", endpoint)))?;
        if !status.is_success() {
            let preview: String = text.chars().take(1200).collect();
            return Err(AdapterError::Llm(format!(
                "{} returned {status}: {preview}",
                endpoint
            )));
        }
        let value: Value = serde_json::from_str(&text).map_err(|error| {
            AdapterError::Llm(format!("{} returned invalid JSON: {error}", endpoint))
        })?;
        validate_probe_response_end(self.protocol, &value)?;
        let response_text = probe_response_text(self.protocol, &value).ok_or_else(|| {
            AdapterError::Llm(format!(
                "{} succeeded but the response did not match the selected provider protocol",
                endpoint
            ))
        })?;
        Ok(ModelProbeResult {
            endpoint: endpoint.to_string(),
            protocol: format!("{:?}", self.protocol),
            model: self.model.clone(),
            latency_ms: started.elapsed().as_millis(),
            response_preview: response_text.chars().take(160).collect(),
        })
    }

    pub async fn list_models(&self) -> AdapterResult<Vec<String>> {
        let endpoint = provider_models_endpoint(self.protocol, self.base_url.clone())?;
        let builder = self.get_request(endpoint.clone());
        let response = builder
            .send()
            .await
            .map_err(|error| AdapterError::Llm(format!("{}: {error}", endpoint)))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(AdapterError::Llm(format!("{} returned {status}", endpoint)));
        }
        let value: Value = serde_json::from_str(&text).map_err(|error| {
            AdapterError::Llm(format!("{} returned invalid JSON: {error}", endpoint))
        })?;
        let entries = if self.protocol == ProviderProtocol::Ollama {
            value.get("models").and_then(Value::as_array)
        } else {
            value.get("data").and_then(Value::as_array)
        }
        .ok_or_else(|| AdapterError::Llm("model list response had no model array".into()))?;
        let mut models: Vec<String> = entries
            .iter()
            .filter_map(|entry| {
                if self.protocol == ProviderProtocol::Ollama {
                    entry.get("name")
                } else {
                    entry.get("id")
                }
            })
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        models.sort();
        models.dedup();
        Ok(models)
    }
}

#[async_trait]
impl Provider for UnifiedModelClient {
    fn profile_id(&self) -> Uuid {
        self.profile_id
    }

    async fn stream_provider(
        &self,
        request: ProviderModelRequest,
    ) -> AgentResult<Vec<ProviderStreamEvent>> {
        let mut events = Vec::new();
        self.stream_with_provider(request, |event| events.push(event))
            .await
            .map_err(|error| AgentError::Model(error.to_string()))?;
        Ok(events)
    }
}

pub fn parse_tool_call_response(response: &Value, expected_tool: &str) -> AdapterResult<Value> {
    let calls = response
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
        .ok_or_else(|| AdapterError::Llm("response did not contain a tool call".into()))?;
    let function = calls
        .iter()
        .filter_map(|call| call.get("function"))
        .find(|function| function.get("name").and_then(Value::as_str) == Some(expected_tool))
        .ok_or_else(|| AdapterError::Llm(format!("missing tool call {expected_tool}")))?;
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .ok_or_else(|| AdapterError::Llm("tool arguments were not a JSON string".into()))?;
    serde_json::from_str(arguments).map_err(AdapterError::from)
}

#[derive(Debug, Clone)]
pub struct SseToolCallAccumulator {
    expected_tool: String,
    tool_name: String,
    arguments: String,
}

impl SseToolCallAccumulator {
    pub fn new(expected_tool: impl Into<String>) -> Self {
        Self {
            expected_tool: expected_tool.into(),
            tool_name: String::new(),
            arguments: String::new(),
        }
    }

    pub fn push_data(&mut self, data: &str) -> AdapterResult<()> {
        if data.trim() == "[DONE]" {
            return Ok(());
        }
        let value: Value = serde_json::from_str(data)?;
        let Some(calls) = value
            .pointer("/choices/0/delta/tool_calls")
            .and_then(Value::as_array)
        else {
            return Ok(());
        };
        for call in calls {
            if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                self.tool_name.push_str(name);
            }
            if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str) {
                self.arguments.push_str(arguments);
            }
        }
        Ok(())
    }

    pub fn finish(self) -> AdapterResult<Value> {
        if self.tool_name != self.expected_tool {
            return Err(AdapterError::Llm(format!(
                "expected tool {}, received {}",
                self.expected_tool, self.tool_name
            )));
        }
        serde_json::from_str(&self.arguments).map_err(AdapterError::from)
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleClient {
    http: reqwest::Client,
    base_url: Url,
    model: String,
    api_key: String,
}

impl OpenAiCompatibleClient {
    pub fn new(base_url: Url, model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            model: model.into(),
            api_key: api_key.into(),
        }
    }

    pub async fn call_tool<T>(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tool_name: &str,
    ) -> AdapterResult<T>
    where
        T: DeserializeOwned + JsonSchema,
    {
        let endpoint = self
            .base_url
            .join("v1/chat/completions")
            .map_err(|error| AdapterError::Llm(error.to_string()))?;
        let schema = serde_json::to_value(schema_for!(T))?;
        let body = json!({
            "model": self.model,
            "stream": true,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": "Submit a schema-valid OmicsOps control object.",
                    "strict": true,
                    "parameters": schema
                }
            }],
            "tool_choice": {
                "type": "function",
                "function": {"name": tool_name}
            }
        });
        let response = self
            .http
            .post(endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| AdapterError::Llm(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AdapterError::Llm(format!("{status}: {body}")));
        }

        let mut stream = response.bytes_stream();
        let mut pending = Vec::new();
        let mut accumulator = SseToolCallAccumulator::new(tool_name);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| AdapterError::Llm(error.to_string()))?;
            pending.extend_from_slice(&chunk);
            while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                let line_bytes = pending.drain(..=newline).collect::<Vec<_>>();
                let line = std::str::from_utf8(&line_bytes[..newline])
                    .map_err(|error| {
                        AdapterError::Llm(format!("model stream returned invalid UTF-8: {error}"))
                    })?
                    .trim()
                    .to_owned();
                if let Some(data) = line.strip_prefix("data:") {
                    accumulator.push_data(data.trim())?;
                }
            }
        }
        let value = accumulator.finish()?;
        serde_json::from_value(value).map_err(AdapterError::from)
    }

    pub async fn probe_tool_calling(&self) -> AdapterResult<()> {
        #[derive(Debug, Serialize, serde::Deserialize, JsonSchema)]
        struct Probe {
            ok: bool,
        }
        let probe: Probe = self
            .call_tool(
                "Return the requested tool call and nothing else.",
                "Submit {\"ok\": true}.",
                "submit_capability_probe",
            )
            .await?;
        if probe.ok {
            Ok(())
        } else {
            Err(AdapterError::Llm(
                "model returned an invalid capability probe".into(),
            ))
        }
    }
}

#[cfg(test)]
mod reasoning_effort_tests {
    use super::*;

    fn client(model: &str) -> UnifiedModelClient {
        UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::OpenAiCompatible,
            Url::parse("https://api.openai.com/v1").unwrap(),
            model,
            Some("test-only".into()),
        )
        .unwrap()
    }

    fn request() -> ProviderModelRequest {
        ProviderModelRequest {
            system: "test".into(),
            messages: vec![],
            tools: vec![],
            require_strict_json_fallback: false,
        }
    }

    #[test]
    fn explicit_effort_is_isolated_and_survives_probe_and_fallback_shapes() {
        let main = client("main")
            .with_reasoning_effort(Some("low".into()))
            .unwrap();
        let child = client("child")
            .with_reasoning_effort(Some("max".into()))
            .unwrap();
        for (client, expected) in [(&main, "low"), (&child, "max")] {
            let built = client.build_provider_request(&request()).unwrap();
            assert_eq!(built.body["reasoning_effort"], expected);
            let mut fallback = built.body.clone();
            fallback["stream"] = json!(false);
            assert_eq!(fallback["reasoning_effort"], expected);
            let probe = client.probe_body().unwrap();
            assert_eq!(probe["reasoning_effort"], expected);
            assert_eq!(probe["max_completion_tokens"], 4096);
            assert!(probe.get("max_tokens").is_none());
        }
        let inherited = client("legacy");
        assert!(
            inherited
                .build_provider_request(&request())
                .unwrap()
                .body
                .get("reasoning_effort")
                .is_none()
        );
        assert!(
            inherited
                .probe_body()
                .unwrap()
                .get("reasoning_effort")
                .is_none()
        );
        let reset = child.with_reasoning_effort(None).unwrap();
        assert!(
            reset
                .build_provider_request(&request())
                .unwrap()
                .body
                .get("reasoning_effort")
                .is_none()
        );
    }

    #[test]
    fn rejects_unknown_values_and_other_protocols_without_network() {
        for value in ["", "MAX", " max", "arbitrary"] {
            assert!(
                client("exact")
                    .with_reasoning_effort(Some(value.into()))
                    .is_err()
            );
        }
        for protocol in [ProviderProtocol::Anthropic, ProviderProtocol::Ollama] {
            let client = UnifiedModelClient::new(
                Uuid::new_v4(),
                protocol,
                Url::parse("http://127.0.0.1:1").unwrap(),
                "exact",
                Some("test-only".into()),
            )
            .unwrap();
            assert!(client.with_reasoning_effort(Some("max".into())).is_err());
        }
    }

    #[test]
    fn budget_includes_effort_and_probe_uses_the_same_output_reservation() {
        let plain = client("exact").with_request_budget(RequestBudget {
            context_window_tokens: 10000,
            reserved_output_tokens: 1,
            safety_margin_tokens: 0,
        });
        let bytes = serde_json::to_vec(&plain.build_provider_request(&request()).unwrap().body)
            .unwrap()
            .len() as u32;
        let budget = RequestBudget {
            context_window_tokens: bytes + 1,
            reserved_output_tokens: 1,
            safety_margin_tokens: 0,
        };
        let plain = plain.with_request_budget(budget);
        plain.validate_request(&request()).unwrap();
        let configured = plain
            .with_reasoning_effort(Some("max".into()))
            .unwrap()
            .with_request_budget(budget);
        assert!(
            configured
                .validate_request(&request())
                .unwrap_err()
                .to_string()
                .contains("request budget:")
        );
        let configured = configured.with_request_budget(RequestBudget {
            context_window_tokens: 10000,
            reserved_output_tokens: 100,
            safety_margin_tokens: 1,
        });
        let probe = configured.probe_body().unwrap();
        assert_eq!(probe["max_completion_tokens"], 100);
        assert_eq!(probe["reasoning_effort"], "max");
    }
}

#[cfg(test)]
mod fast_mode_tests {
    use super::*;

    fn client(model: &str, base_url: &str) -> UnifiedModelClient {
        UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::OpenAiCompatible,
            Url::parse(base_url).unwrap(),
            model,
            Some("test-only".into()),
        )
        .unwrap()
    }

    fn request() -> ProviderModelRequest {
        ProviderModelRequest {
            system: "test".into(),
            messages: vec![],
            tools: vec![],
            require_strict_json_fallback: false,
        }
    }

    #[test]
    fn fast_mode_uses_exact_wire_values_in_request_probe_and_fallback() {
        for (fast_mode, expected) in [(Some(true), "priority"), (Some(false), "default")] {
            let client =
                client("gpt-5.6-luna", "https://api.openai.com/v1").with_fast_mode(fast_mode);
            let built = client.build_provider_request(&request()).unwrap();
            assert_eq!(built.body["service_tier"], expected);
            let mut fallback = built.body.clone();
            fallback["stream"] = json!(false);
            assert_eq!(fallback["service_tier"], expected);
            assert_eq!(client.probe_body().unwrap()["service_tier"], expected);
        }
    }

    #[test]
    fn absent_or_unreviewed_fast_mode_does_not_leak_service_tier() {
        let inherited = client("gpt-5.6-luna", "https://api.openai.com/v1");
        assert!(
            inherited
                .build_provider_request(&request())
                .unwrap()
                .body
                .get("service_tier")
                .is_none()
        );
        assert!(
            inherited
                .probe_body()
                .unwrap()
                .get("service_tier")
                .is_none()
        );

        for base_url in ["https://proxy.example/v1", "http://api.openai.com/v1"] {
            let standard = client("gpt-5.6-luna", base_url).with_fast_mode(Some(false));
            assert!(
                standard
                    .build_provider_request(&request())
                    .unwrap()
                    .body
                    .get("service_tier")
                    .is_none()
            );
            assert!(standard.probe_body().unwrap().get("service_tier").is_none());

            let unsupported = client("gpt-5.6-luna", base_url).with_fast_mode(Some(true));
            assert!(unsupported.build_provider_request(&request()).is_err());
            assert!(unsupported.probe_body().is_err());
        }
    }
}

#[cfg(test)]
mod image_budget_tests {
    use super::*;

    fn client(model: &str, base_url: &str) -> UnifiedModelClient {
        UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::OpenAiCompatible,
            Url::parse(base_url).unwrap(),
            model,
            Some("test-only".into()),
        )
        .unwrap()
    }

    fn request_with_images(count: usize) -> ProviderModelRequest {
        let mut parts = vec![ModelContentPart::Text {
            text: "inspect these images".into(),
        }];
        parts.extend((0..count).map(|_| ModelContentPart::Image {
            media_type: "image/png".into(),
            data_base64: "aW1hZ2U=".into(),
        }));
        ProviderModelRequest {
            system: "You are a test model.".into(),
            messages: vec![omicsops_agent::ModelMessage {
                role: "user".into(),
                content: ModelMessageContent::Parts(parts),
            }],
            tools: vec![],
            require_strict_json_fallback: false,
        }
    }

    fn request() -> ProviderModelRequest {
        ProviderModelRequest {
            system: "test".into(),
            messages: vec![],
            tools: vec![],
            require_strict_json_fallback: false,
        }
    }

    #[test]
    fn image_budget_requires_exact_official_host_path_protocol_and_model() {
        for model in [
            "gpt-6-astra",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.4-mini",
            "gpt-5.4-nano",
            "gpt-5.2",
            "gpt-4.1-mini",
            "gpt-4.1-mini-2025-04-14",
            "gpt-4o",
            "gpt-4.1",
            "gpt-4.1-2025-04-14",
            "gpt-4o-mini",
            "gpt-5.1",
        ] {
            assert!(
                client(model, "https://api.openai.com/v1").has_image_budget(),
                "{model}"
            );
        }
        for model in [
            "gpt-5.6-luna-preview",
            "gpt-5.6-luna:latest",
            "gpt-4.1-mini-2025-04-14-extra",
            "GPT-5.6-luna",
        ] {
            assert!(
                !client(model, "https://api.openai.com/v1").has_image_budget(),
                "{model}"
            );
        }
        for base_url in [
            "https://api.openai.com/v2",
            "https://api.openai.com:8443/v1",
            "http://api.openai.com/v1",
            "https://proxy.example/v1",
        ] {
            assert!(
                !client("gpt-5.6-luna", base_url).has_image_budget(),
                "{base_url}"
            );
        }
        for base_url in [
            "https://api.openai.com",
            "https://api.openai.com/",
            "https://api.openai.com/v1",
            "https://api.openai.com/v1/",
        ] {
            assert!(
                client("gpt-5.6-luna", base_url).has_image_budget(),
                "{base_url}"
            );
        }
        let anthropic = UnifiedModelClient::new(
            Uuid::new_v4(),
            ProviderProtocol::Anthropic,
            Url::parse("https://api.openai.com/v1").unwrap(),
            "gpt-5.6-luna",
            Some("test-only".into()),
        )
        .unwrap();
        assert!(!anthropic.has_image_budget());
    }

    #[test]
    fn credential_free_capability_check_reuses_the_exact_policy() {
        let official = Url::parse("https://api.openai.com/v1").unwrap();
        assert!(supports_image_budget(
            ProviderProtocol::OpenAiCompatible,
            &official,
            "gpt-5.6-luna"
        ));
        assert!(!supports_image_budget(
            ProviderProtocol::OpenAiCompatible,
            &official,
            "gpt-5.6-luna-preview"
        ));
        assert!(!supports_image_budget(
            ProviderProtocol::Anthropic,
            &official,
            "gpt-5.6-luna"
        ));
    }

    #[test]
    fn trusted_image_request_adds_high_detail_without_mutating_payload() {
        let request = request_with_images(1);
        let client = client("gpt-5.6-luna", "https://api.openai.com/v1");
        let built = client.build_provider_request(&request).unwrap();
        let image = &built.body["messages"][1]["content"][1];
        assert_eq!(image["type"], "image_url");
        assert_eq!(image["image_url"]["detail"], "high");
        assert_eq!(image["image_url"]["url"], "data:image/png;base64,aW1hZ2U=");
        let ModelMessageContent::Parts(parts) = &request.messages[0].content else {
            panic!("test request should be multimodal");
        };
        assert!(matches!(
            &parts[1],
            ModelContentPart::Image { data_base64, .. } if data_base64 == "aW1hZ2U="
        ));
        client.validate_request(&request).unwrap();
    }

    #[test]
    fn unknown_image_shape_or_model_fails_closed() {
        let request = request_with_images(1);
        let unknown = client("gpt-5.6-luna-preview", "https://api.openai.com/v1");
        let error = unknown.validate_request(&request).unwrap_err();
        assert!(error.to_string().contains("image token cost is unknown"));

        let trusted = client("gpt-5.6-luna", "https://api.openai.com/v1");
        let mut body = trusted.build_provider_request(&request).unwrap().body;
        body["messages"][1]["content"][1]["image_url"]["url"] =
            json!("https://example.test/image.png");
        let error = trusted.validate_provider_body(&body).unwrap_err();
        assert!(error.to_string().contains("remote or malformed image URL"));
    }

    #[test]
    fn multiple_images_use_the_same_conservative_bound() {
        let request = request_with_images(2);
        let client = client("gpt-5.6-luna", "https://api.openai.com/v1");
        let provisional = client.clone().with_request_budget(RequestBudget {
            context_window_tokens: u32::MAX,
            reserved_output_tokens: 1,
            safety_margin_tokens: 0,
        });
        let body = provisional.build_provider_request(&request).unwrap().body;
        let (shape, count) = image_budget_shape(&body).unwrap();
        assert_eq!(count, 2);
        let text_tokens = serde_json::to_vec(&shape).unwrap().len() as u64;
        let per_image = trusted_image_token_budget(
            ProviderProtocol::OpenAiCompatible,
            &Url::parse("https://api.openai.com/v1").unwrap(),
            "gpt-5.6-luna",
        )
        .unwrap();
        let one_image_context = text_tokens + per_image + 1;
        let budget = RequestBudget {
            context_window_tokens: u32::try_from(one_image_context).unwrap(),
            reserved_output_tokens: 1,
            safety_margin_tokens: 0,
        };
        let error = client
            .with_request_budget(budget)
            .validate_request(&request)
            .unwrap_err();
        assert!(error.to_string().contains("request budget:"));
        assert!(error.to_string().contains("exceeding context window"));
    }

    #[tokio::test]
    async fn final_send_and_non_streaming_fallback_share_image_validation() {
        let request = request_with_images(2);
        let client = client("gpt-5.6-luna", "https://api.openai.com/v1");
        let provisional = client.clone().with_request_budget(RequestBudget {
            context_window_tokens: u32::MAX,
            reserved_output_tokens: 1,
            safety_margin_tokens: 0,
        });
        let body = provisional.build_provider_request(&request).unwrap().body;
        let (shape, _) = image_budget_shape(&body).unwrap();
        let text_tokens = serde_json::to_vec(&shape).unwrap().len() as u64;
        let one_image_context = text_tokens + 3_001 + 1;
        let client = client.with_request_budget(RequestBudget {
            context_window_tokens: u32::try_from(one_image_context).unwrap(),
            reserved_output_tokens: 1,
            safety_margin_tokens: 0,
        });
        let body = client.build_provider_request(&request).unwrap().body;
        let endpoint = Url::parse("https://api.openai.com/v1/chat/completions").unwrap();
        let mut events = Vec::new();
        let first = client
            .send_with_retry_provider(&endpoint, &body, &mut |event| events.push(event))
            .await
            .unwrap_err()
            .to_string();
        let mut fallback = body.clone();
        fallback["stream"] = Value::Bool(false);
        let second = client
            .send_with_retry_provider(&endpoint, &fallback, &mut |event| events.push(event))
            .await
            .unwrap_err()
            .to_string();
        assert!(first.contains("request budget:") && first.contains("exceeding context window"));
        assert!(second.contains("request budget:") && second.contains("exceeding context window"));
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn final_send_rejects_a_body_model_mismatch_before_network() {
        let request = request_with_images(1);
        let client = client("gpt-5.6-luna", "https://api.openai.com/v1");
        let mut body = client.build_provider_request(&request).unwrap().body;
        body["model"] = json!("gpt-5.6-luna-preview");
        let mut events = Vec::new();
        let error = client
            .send_with_retry_provider(
                &Url::parse("https://api.openai.com/v1/chat/completions").unwrap(),
                &body,
                &mut |event| events.push(event),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("does not match selected model"));
        assert!(events.is_empty());
    }

    #[test]
    fn text_budget_estimation_keeps_the_public_byte_shape() {
        let body = json!({
            "model": "gpt-5.6-luna",
            "messages": [{"role": "user", "content": "plain text"}],
        });
        let budget = RequestBudget {
            context_window_tokens: 10_000,
            reserved_output_tokens: 1,
            safety_margin_tokens: 0,
        };
        assert_eq!(
            budget.estimate_input_tokens(&body).unwrap(),
            serde_json::to_vec(&body).unwrap().len() as u64
        );
        budget.validate_provider_json(&body).unwrap();
    }

    #[test]
    fn request_metrics_keep_wire_bytes_and_image_bound_separate() {
        let request = request_with_images(2);
        let client = client("gpt-5.6-luna", "https://api.openai.com/v1");
        let body = client.build_provider_request(&request).unwrap();
        let metrics = client.measure_provider_request(&body).unwrap();
        assert_eq!(metrics, client.measure_model_request(&request).unwrap());
        assert_eq!(metrics.image_count, 2);
        assert_eq!(metrics.image_bound_tokens, Some(6_002));
        assert_eq!(
            metrics.serialized_request_bytes,
            serde_json::to_vec(&body.body).unwrap().len() as u64
        );
        assert!(metrics.image_payload_bytes > 0);
        assert_ne!(metrics.text_shape_bytes, metrics.serialized_request_bytes);
    }

    #[test]
    fn request_metrics_report_observed_zero_images_without_an_image_bound_guess() {
        let request = request();
        let client = client("gpt-5.6-luna", "https://api.openai.com/v1");
        let body = client.build_provider_request(&request).unwrap();
        let metrics = client.measure_provider_request(&body).unwrap();
        assert_eq!(metrics.image_count, 0);
        assert_eq!(metrics.image_bound_tokens, Some(0));
        assert_eq!(metrics.image_payload_bytes, 0);
        assert_eq!(metrics.text_shape_bytes, metrics.serialized_request_bytes);
    }
}

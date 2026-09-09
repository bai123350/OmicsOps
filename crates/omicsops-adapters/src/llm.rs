use async_trait::async_trait;
use futures_util::StreamExt;
use omicsops_agent::provider::{
    Provider, ProviderRequest as ProviderModelRequest, ProviderStreamEvent,
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
/// provider/model dependent, so requests containing images fail closed until a
/// model-specific estimator is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestBudget {
    pub context_window_tokens: u32,
    pub reserved_output_tokens: u32,
    pub safety_margin_tokens: u32,
}

impl RequestBudget {
    const ERROR_PREFIX: &'static str = "request budget:";

    /// Estimate the input token count for a fully shaped provider JSON body.
    ///
    /// This uses compact UTF-8 byte length as a conservative estimate, rather
    /// than pretending to know the selected model's tokenizer. The complete
    /// JSON body is measured, so system content, messages, tool schemas and
    /// provider formatting are all included.
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
    if message
        .get("images")
        .and_then(Value::as_array)
        .is_some_and(|images| !images.is_empty())
    {
        return true;
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
    if let Some((input_tokens, output_tokens, provider_json)) = usage_from_value(protocol, value) {
        events.push(ProviderStreamEvent::Usage {
            input_tokens,
            output_tokens,
            provider_json,
        });
    }
    if events.is_empty() {
        return Err(AdapterError::Llm(
            "non-streaming response contained neither text nor tool calls".into(),
        ));
    }
    events.push(ProviderStreamEvent::Completed);
    Ok(events)
}

fn usage_from_value(protocol: ProviderProtocol, value: &Value) -> Option<(u64, u64, Value)> {
    let usage = match protocol {
        ProviderProtocol::OpenAiCompatible => value.get("usage")?,
        ProviderProtocol::Anthropic => value
            .get("usage")
            .or_else(|| value.pointer("/message/usage"))?,
        ProviderProtocol::Ollama => value,
    };
    let (input, output) = match protocol {
        ProviderProtocol::OpenAiCompatible => (
            usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
        ProviderProtocol::Anthropic => (
            usage
                .get("input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            usage
                .get("output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
        ProviderProtocol::Ollama => (
            usage
                .get("prompt_eval_count")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            usage.get("eval_count").and_then(Value::as_u64).unwrap_or(0),
        ),
    };
    (input > 0 || output > 0).then(|| (input, output, safe_usage_metadata(protocol, usage)))
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
    saw_completion: bool,
}

impl ProviderToolStreamDecoder {
    pub fn new(protocol: ProviderProtocol) -> Self {
        Self {
            protocol,
            pending: Vec::new(),
            active_calls: BTreeMap::new(),
            tool_aliases: BTreeMap::new(),
            saw_completion: false,
        }
    }

    pub fn for_request(protocol: ProviderProtocol, request: &ProviderModelRequest) -> Self {
        Self {
            protocol,
            pending: Vec::new(),
            active_calls: BTreeMap::new(),
            tool_aliases: provider_tool_alias_map(request),
            saw_completion: false,
        }
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
        if let Some((input_tokens, output_tokens, provider_json)) =
            usage_from_value(self.protocol, value)
        {
            events.push(ProviderStreamEvent::Usage {
                input_tokens,
                output_tokens,
                provider_json,
            });
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
        if let Some((input_tokens, output_tokens, provider_json)) =
            usage_from_value(self.protocol, value)
        {
            events.push(ProviderStreamEvent::Usage {
                input_tokens,
                output_tokens,
                provider_json,
            });
        }
        Ok(())
    }

    fn push_ollama(
        &mut self,
        value: &Value,
        events: &mut Vec<ProviderStreamEvent>,
    ) -> AdapterResult<()> {
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
        if let Some((input_tokens, output_tokens, provider_json)) =
            usage_from_value(self.protocol, value)
        {
            events.push(ProviderStreamEvent::Usage {
                input_tokens,
                output_tokens,
                provider_json,
            });
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
    let reason = match protocol {
        ProviderProtocol::OpenAiCompatible => value.pointer("/choices/0/finish_reason"),
        ProviderProtocol::Anthropic => value
            .get("stop_reason")
            .or_else(|| value.pointer("/delta/stop_reason")),
        ProviderProtocol::Ollama => value.get("done_reason"),
    }
    .and_then(Value::as_str);
    match reason {
        Some("length" | "max_tokens" | "model_context_window_exceeded") => Err(AdapterError::Llm(
            "truncated_output: provider exhausted its output allowance; partial tool calls cannot execute".into())),
        Some("content_filter" | "refusal") => Err(AdapterError::Llm(
            "unsuccessful_model_response: provider did not finish the requested response".into())),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone)]
pub struct UnifiedModelClient {
    profile_id: Uuid,
    protocol: ProviderProtocol,
    base_url: Url,
    model: String,
    credential: Option<String>,
    request_budget: Option<RequestBudget>,
    reasoning_effort: Option<String>,
    http: reqwest::Client,
}

impl UnifiedModelClient {
    fn authenticate(&self, mut builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder = builder.header(reqwest::header::ACCEPT_ENCODING, "identity");
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
        Ok(Self {
            profile_id,
            protocol,
            base_url,
            model: model.into(),
            credential,
            request_budget: None,
            reasoning_effort: None,
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(MODEL_REQUEST_TIMEOUT)
                .build()
                .map_err(|error| AdapterError::Llm(error.to_string()))?,
        })
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

    fn apply_reasoning_effort(&self, body: &mut Value) {
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }
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
        Ok(built)
    }

    /// Validate the complete provider-shaped request before any network I/O.
    pub fn validate_request(&self, request: &ProviderModelRequest) -> AdapterResult<()> {
        let provider_request = self.build_provider_request(request)?;
        if let Some(budget) = self.request_budget {
            budget.validate_provider_json(&provider_request.body)?;
        }
        Ok(())
    }

    async fn send_with_retry_provider(
        &self,
        endpoint: &Url,
        body: &Value,
        on_event: &mut impl FnMut(ProviderStreamEvent),
    ) -> AdapterResult<reqwest::Response> {
        if let Some(budget) = self.request_budget {
            // Keep this check at the final send boundary so retries and the
            // non-streaming fallback cannot bypass the same preflight.
            budget.validate_provider_json(body)?;
        }
        let mut retries = 0_u8;
        loop {
            let result = self
                .authenticate(self.http.post(endpoint.clone()).json(body))
                .send()
                .await;
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
        if let Some(budget) = self.request_budget {
            budget.validate_provider_json(&provider_request.body)?;
        }
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

    fn probe_body(&self) -> AdapterResult<Value> {
        let mut body = match self.protocol {
            ProviderProtocol::OpenAiCompatible => json!({
                "model": self.model,
                "stream": false,
                "max_tokens": 16,
                "messages": [{"role":"user", "content":"Reply with OK."}]
            }),
            ProviderProtocol::Anthropic => json!({
                "model": self.model,
                "stream": false,
                "max_tokens": 16,
                "messages": [{"role":"user", "content":"Reply with OK."}]
            }),
            ProviderProtocol::Ollama => json!({
                "model": self.model,
                "stream": false,
                "messages": [{"role":"user", "content":"Reply with OK."}]
            }),
        };
        self.apply_reasoning_effort(&mut body);
        if self.reasoning_effort.is_some() {
            // Reasoning consumes output tokens too; the legacy 16-token
            // connection probe cannot exercise an explicit reasoning request.
            body["max_tokens"] = json!(4096);
            if is_official_openai_endpoint(&self.base_url) {
                body.as_object_mut().unwrap().remove("max_tokens");
                body["max_completion_tokens"] = json!(4096);
            }
        }
        if let Some(budget) = self.request_budget {
            match self.protocol {
                ProviderProtocol::OpenAiCompatible
                    if is_official_openai_endpoint(&self.base_url) =>
                {
                    body.as_object_mut().unwrap().remove("max_tokens");
                    body["max_completion_tokens"] = json!(budget.reserved_output_tokens);
                }
                ProviderProtocol::OpenAiCompatible | ProviderProtocol::Anthropic => {
                    body["max_tokens"] = json!(budget.reserved_output_tokens);
                }
                ProviderProtocol::Ollama => {
                    body["options"] = json!({"num_predict": budget.reserved_output_tokens});
                }
            }
            budget.validate_provider_json(&body)?;
        }
        Ok(body)
    }

    pub async fn probe(&self) -> AdapterResult<ModelProbeResult> {
        let endpoint = provider_endpoint(self.protocol, self.base_url.clone())?;
        let body = self.probe_body()?;
        let mut builder = self.http.post(endpoint.clone()).json(&body);
        match self.protocol {
            ProviderProtocol::Anthropic => {
                builder = builder
                    .header("x-api-key", self.credential.as_deref().unwrap_or_default())
                    .header("anthropic-version", "2023-06-01");
            }
            ProviderProtocol::OpenAiCompatible => {
                builder = builder.bearer_auth(self.credential.as_deref().unwrap_or_default());
            }
            ProviderProtocol::Ollama => {}
        }
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
        validate_response_end(self.protocol, &value)?;
        let response_text = match self.protocol {
            ProviderProtocol::OpenAiCompatible => value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str),
            ProviderProtocol::Anthropic => value.pointer("/content/0/text").and_then(Value::as_str),
            ProviderProtocol::Ollama => value.pointer("/message/content").and_then(Value::as_str),
        }
        .ok_or_else(|| {
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
        let mut builder = self.http.get(endpoint.clone());
        match self.protocol {
            ProviderProtocol::Anthropic => {
                builder = builder
                    .header("x-api-key", self.credential.as_deref().unwrap_or_default())
                    .header("anthropic-version", "2023-06-01");
            }
            ProviderProtocol::OpenAiCompatible => {
                builder = builder.bearer_auth(self.credential.as_deref().unwrap_or_default());
            }
            ProviderProtocol::Ollama => {}
        }
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

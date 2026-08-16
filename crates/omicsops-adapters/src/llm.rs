use async_trait::async_trait;
use futures_util::StreamExt;
use omicsops_agent::harness_v3::{ModelProviderV2, ModelRequestV2, ModelStreamEventV2};
use omicsops_agent::{AgentError, AgentResult, ModelProvider, ModelRequest, ModelStreamEvent};
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

pub fn build_provider_request(
    protocol: ProviderProtocol,
    base_url: Url,
    model: &str,
    request: &ModelRequest,
) -> AdapterResult<ProviderRequest> {
    let tool = request.tool_name.as_ref().map(|name| {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": "Submit a schema-valid OmicsOps control object.",
                "parameters": request.tool_schema.clone().unwrap_or_else(|| json!({"type":"object"}))
            }
        })
    });
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|message| json!({"role": message.role, "content": message.content}))
        .collect();

    match protocol {
        ProviderProtocol::OpenAiCompatible => {
            let mut body = json!({
                "model": model,
                "stream": true,
                "messages": std::iter::once(json!({"role":"system", "content":request.system}))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "tools": tool.into_iter().collect::<Vec<_>>()
            });
            if let Some(name) = &request.tool_name {
                body["tool_choice"] = json!({"type":"function", "function":{"name":name}});
            }
            Ok(ProviderRequest {
                endpoint: provider_endpoint(ProviderProtocol::OpenAiCompatible, base_url)?,
                body,
                requires_credential: true,
            })
        }
        ProviderProtocol::Anthropic => {
            let tools: Vec<Value> = tool
                .into_iter()
                .map(|tool| {
                    json!({
                        "name": tool["function"]["name"],
                        "description": tool["function"]["description"],
                        "input_schema": tool["function"]["parameters"]
                    })
                })
                .collect();
            let mut body = json!({
                "model": model,
                "system": request.system,
                "max_tokens": 4096,
                "stream": true,
                "messages": messages,
                "tools": tools
            });
            if let Some(name) = &request.tool_name {
                body["tool_choice"] = json!({"type":"tool", "name":name});
            }
            Ok(ProviderRequest {
                endpoint: provider_endpoint(ProviderProtocol::Anthropic, base_url)?,
                body,
                requires_credential: true,
            })
        }
        ProviderProtocol::Ollama => Ok(ProviderRequest {
            endpoint: provider_endpoint(ProviderProtocol::Ollama, base_url)?,
            body: json!({
                "model": model,
                "stream": true,
                "messages": std::iter::once(json!({"role":"system", "content":request.system}))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "tools": tool.into_iter().collect::<Vec<_>>()
            }),
            requires_credential: false,
        }),
    }
}

pub fn build_provider_request_v2(
    protocol: ProviderProtocol,
    base_url: Url,
    model: &str,
    request: &ModelRequestV2,
) -> AdapterResult<ProviderRequest> {
    let messages = request
        .messages
        .iter()
        .map(|message| json!({"role": message.role, "content": message.content}))
        .collect::<Vec<_>>();
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
        ProviderProtocol::OpenAiCompatible => Ok(ProviderRequest {
            endpoint: provider_endpoint(protocol, base_url)?,
            body: json!({
                "model": model,
                "stream": true,
                "stream_options": {"include_usage": true},
                "messages": std::iter::once(json!({"role":"system", "content":request.system}))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "tools": openai_tools
            }),
            requires_credential: true,
        }),
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
            Ok(ProviderRequest {
                endpoint: provider_endpoint(protocol, base_url)?,
                body: json!({
                    "model": model,
                    "system": request.system,
                    "max_tokens": 4096,
                    "stream": true,
                    "messages": messages,
                    "tools": tools
                }),
                requires_credential: true,
            })
        }
        ProviderProtocol::Ollama => Ok(ProviderRequest {
            endpoint: provider_endpoint(protocol, base_url)?,
            body: json!({
                "model": model,
                "stream": true,
                "messages": std::iter::once(json!({"role":"system", "content":request.system}))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "tools": openai_tools
            }),
            requires_credential: false,
        }),
    }
}

fn provider_tool_aliases(request: &ModelRequestV2) -> Vec<(String, String)> {
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
    tool: &omicsops_agent::harness_v3::ModelToolSpec,
    provider_name: &str,
) -> String {
    if provider_name == tool.id {
        tool.description.clone()
    } else {
        format!("{} [OmicsOps tool id: {}]", tool.description, tool.id)
    }
}

fn provider_tool_alias_map(request: &ModelRequestV2) -> BTreeMap<String, String> {
    provider_tool_aliases(request).into_iter().collect()
}

fn canonical_tool_id(aliases: &BTreeMap<String, String>, provider_name: &str) -> String {
    aliases
        .get(provider_name)
        .cloned()
        .unwrap_or_else(|| provider_name.to_owned())
}

pub fn parse_provider_response_v2(
    protocol: ProviderProtocol,
    value: &Value,
) -> AdapterResult<Vec<ModelStreamEventV2>> {
    parse_provider_response_v2_with_aliases(protocol, value, &BTreeMap::new())
}

pub fn parse_provider_response_v2_for_request(
    protocol: ProviderProtocol,
    value: &Value,
    request: &ModelRequestV2,
) -> AdapterResult<Vec<ModelStreamEventV2>> {
    parse_provider_response_v2_with_aliases(protocol, value, &provider_tool_alias_map(request))
}

fn parse_provider_response_v2_with_aliases(
    protocol: ProviderProtocol,
    value: &Value,
    aliases: &BTreeMap<String, String>,
) -> AdapterResult<Vec<ModelStreamEventV2>> {
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
        events.push(ModelStreamEventV2::TextDelta { text: text.into() });
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
        events.push(ModelStreamEventV2::ToolCallStarted {
            call_id: call_id.clone(),
            index,
            tool_id: canonical_tool_id(aliases, tool_id),
        });
        if let Some(arguments) = arguments {
            events.push(ModelStreamEventV2::ToolArgumentsDelta {
                call_id: call_id.clone(),
                index,
                arguments: match arguments {
                    Value::String(arguments) => arguments.clone(),
                    arguments => arguments.to_string(),
                },
            });
        }
        events.push(ModelStreamEventV2::ToolCallCompleted { call_id, index });
    }
    if let Some((input_tokens, output_tokens, provider_json)) = usage_from_value(protocol, value) {
        events.push(ModelStreamEventV2::Usage {
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
    events.push(ModelStreamEventV2::Completed);
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
    (input > 0 || output > 0).then(|| (input, output, usage.clone()))
}

#[derive(Debug, Clone)]
pub struct ProviderStreamDecoderV2 {
    protocol: ProviderProtocol,
    pending: String,
    active_calls: BTreeMap<u32, (String, String)>,
    tool_aliases: BTreeMap<String, String>,
}

impl ProviderStreamDecoderV2 {
    pub fn new(protocol: ProviderProtocol) -> Self {
        Self {
            protocol,
            pending: String::new(),
            active_calls: BTreeMap::new(),
            tool_aliases: BTreeMap::new(),
        }
    }

    pub fn for_request(protocol: ProviderProtocol, request: &ModelRequestV2) -> Self {
        Self {
            protocol,
            pending: String::new(),
            active_calls: BTreeMap::new(),
            tool_aliases: provider_tool_alias_map(request),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> AdapterResult<Vec<ModelStreamEventV2>> {
        self.pending.push_str(&String::from_utf8_lossy(chunk));
        let mut events = Vec::new();
        while let Some(newline) = self.pending.find('\n') {
            let line = self.pending[..newline]
                .trim_end_matches('\r')
                .trim()
                .to_owned();
            self.pending.drain(..=newline);
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
                for (index, (call_id, _)) in &self.active_calls {
                    events.push(ModelStreamEventV2::ToolCallCompleted {
                        call_id: call_id.clone(),
                        index: *index,
                    });
                }
                events.push(ModelStreamEventV2::Completed);
                continue;
            }
            let value: Value = serde_json::from_str(data)?;
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
        Ok(events)
    }

    fn push_openai(
        &mut self,
        value: &Value,
        events: &mut Vec<ModelStreamEventV2>,
    ) -> AdapterResult<()> {
        if let Some(text) = value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            events.push(ModelStreamEventV2::TextDelta { text: text.into() });
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
                    events.push(ModelStreamEventV2::ToolCallStarted {
                        call_id: call_id.clone(),
                        index,
                        tool_id: tool_id.clone(),
                    });
                }
                self.active_calls.insert(index, (call_id.clone(), tool_id));
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str)
                    && !arguments.is_empty()
                {
                    events.push(ModelStreamEventV2::ToolArgumentsDelta {
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
            events.push(ModelStreamEventV2::Usage {
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
                events.push(ModelStreamEventV2::ToolCallCompleted {
                    call_id: call_id.clone(),
                    index: *index,
                });
            }
            events.push(ModelStreamEventV2::Completed);
        }
        Ok(())
    }

    fn push_anthropic(
        &mut self,
        value: &Value,
        events: &mut Vec<ModelStreamEventV2>,
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
                events.push(ModelStreamEventV2::ToolCallStarted {
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
                    events.push(ModelStreamEventV2::ToolArgumentsDelta {
                        call_id: call_id.clone(),
                        index,
                        arguments: arguments.into(),
                    });
                } else if let Some(text) = value.pointer("/delta/text").and_then(Value::as_str) {
                    events.push(ModelStreamEventV2::TextDelta { text: text.into() });
                }
            }
            Some("content_block_stop") => {
                let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                if let Some((call_id, _)) = self.active_calls.get(&index) {
                    events.push(ModelStreamEventV2::ToolCallCompleted {
                        call_id: call_id.clone(),
                        index,
                    });
                }
            }
            Some("message_stop") => events.push(ModelStreamEventV2::Completed),
            _ => {}
        }
        if let Some((input_tokens, output_tokens, provider_json)) =
            usage_from_value(self.protocol, value)
        {
            events.push(ModelStreamEventV2::Usage {
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
        events: &mut Vec<ModelStreamEventV2>,
    ) -> AdapterResult<()> {
        if let Some(text) = value
            .pointer("/message/content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            events.push(ModelStreamEventV2::TextDelta { text: text.into() });
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
                    events.push(ModelStreamEventV2::ToolCallStarted {
                        call_id: call_id.clone(),
                        index,
                        tool_id: tool_id.clone(),
                    });
                }
                self.active_calls.insert(index, (call_id.clone(), tool_id));
                if let Some(arguments) = call.pointer("/function/arguments") {
                    events.push(ModelStreamEventV2::ToolArgumentsDelta {
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
            events.push(ModelStreamEventV2::Usage {
                input_tokens,
                output_tokens,
                provider_json,
            });
        }
        if value.get("done").and_then(Value::as_bool) == Some(true) {
            for (index, (call_id, _)) in &self.active_calls {
                events.push(ModelStreamEventV2::ToolCallCompleted {
                    call_id: call_id.clone(),
                    index: *index,
                });
            }
            events.push(ModelStreamEventV2::Completed);
        }
        Ok(())
    }

    pub fn finish(&mut self) -> AdapterResult<Vec<ModelStreamEventV2>> {
        if self.pending.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.pending.push('\n');
        self.push(&[])
    }
}

pub fn parse_provider_event(protocol: ProviderProtocol, value: &Value) -> Option<ModelStreamEvent> {
    if protocol == ProviderProtocol::OpenAiCompatible {
        if let Some(function) = value.pointer("/choices/0/delta/tool_calls/0/function") {
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let fragment = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !name.is_empty() || !fragment.is_empty() {
                return Some(ModelStreamEvent::ToolArgumentsDelta {
                    name: name.into(),
                    json_fragment: fragment.into(),
                });
            }
        }
    }
    if protocol == ProviderProtocol::Ollama {
        if let Some(function) = value.pointer("/message/tool_calls/0/function") {
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !name.is_empty() {
                let json_fragment = match function.get("arguments") {
                    Some(Value::String(arguments)) => arguments.clone(),
                    Some(arguments) => arguments.to_string(),
                    None => String::new(),
                };
                return Some(ModelStreamEvent::ToolArgumentsDelta {
                    name: name.into(),
                    json_fragment,
                });
            }
        }
    }
    let text = match protocol {
        ProviderProtocol::OpenAiCompatible => value.pointer("/choices/0/delta/content"),
        ProviderProtocol::Anthropic => value.pointer("/delta/text"),
        ProviderProtocol::Ollama => value.pointer("/message/content"),
    }
    .and_then(Value::as_str);
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        return Some(ModelStreamEvent::TextDelta(text.into()));
    }
    let completed = match protocol {
        ProviderProtocol::OpenAiCompatible => value
            .pointer("/choices/0/finish_reason")
            .is_some_and(|reason| !reason.is_null()),
        ProviderProtocol::Anthropic => {
            value.get("type").and_then(Value::as_str) == Some("message_stop")
        }
        ProviderProtocol::Ollama => value.get("done").and_then(Value::as_bool) == Some(true),
    };
    completed.then_some(ModelStreamEvent::Completed)
}

pub fn parse_provider_response(
    protocol: ProviderProtocol,
    value: &Value,
    expected_tool: Option<&str>,
) -> AdapterResult<Vec<ModelStreamEvent>> {
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
        events.push(ModelStreamEvent::TextDelta(text.into()));
    }

    if let Some(expected_tool) = expected_tool {
        let tool = match protocol {
            ProviderProtocol::OpenAiCompatible => value
                .pointer("/choices/0/message/tool_calls")
                .and_then(Value::as_array)
                .and_then(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| call.get("function"))
                        .find(|function| {
                            function.get("name").and_then(Value::as_str) == Some(expected_tool)
                        })
                })
                .and_then(|function| function.get("arguments"))
                .map(|arguments| match arguments {
                    Value::String(arguments) => arguments.clone(),
                    arguments => arguments.to_string(),
                }),
            ProviderProtocol::Anthropic => value
                .get("content")
                .and_then(Value::as_array)
                .and_then(|content| {
                    content.iter().find(|item| {
                        item.get("type").and_then(Value::as_str) == Some("tool_use")
                            && item.get("name").and_then(Value::as_str) == Some(expected_tool)
                    })
                })
                .and_then(|item| item.get("input"))
                .map(Value::to_string),
            ProviderProtocol::Ollama => value
                .pointer("/message/tool_calls")
                .and_then(Value::as_array)
                .and_then(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| call.get("function"))
                        .find(|function| {
                            function.get("name").and_then(Value::as_str) == Some(expected_tool)
                        })
                })
                .and_then(|function| function.get("arguments"))
                .map(|arguments| match arguments {
                    Value::String(arguments) => arguments.clone(),
                    arguments => arguments.to_string(),
                }),
        }
        .ok_or_else(|| {
            AdapterError::Llm(format!(
                "non-streaming response did not contain tool call {expected_tool}"
            ))
        })?;
        events.push(ModelStreamEvent::ToolArgumentsDelta {
            name: expected_tool.into(),
            json_fragment: tool,
        });
    }

    if events.is_empty() {
        return Err(AdapterError::Llm(
            "non-streaming response contained neither text nor the requested tool call".into(),
        ));
    }
    events.push(ModelStreamEvent::Completed);
    Ok(events)
}

#[derive(Debug, Clone)]
pub struct ProviderStreamDecoder {
    protocol: ProviderProtocol,
    pending: String,
    active_tool: Option<String>,
}

impl ProviderStreamDecoder {
    pub fn new(protocol: ProviderProtocol) -> Self {
        Self {
            protocol,
            pending: String::new(),
            active_tool: None,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> AdapterResult<Vec<ModelStreamEvent>> {
        self.pending.push_str(&String::from_utf8_lossy(chunk));
        let mut events = Vec::new();
        while let Some(newline) = self.pending.find('\n') {
            let line = self.pending[..newline]
                .trim_end_matches('\r')
                .trim()
                .to_owned();
            self.pending.drain(..=newline);
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
                events.push(ModelStreamEvent::Completed);
                continue;
            }
            let value: Value = serde_json::from_str(data)?;
            if self.protocol == ProviderProtocol::Anthropic {
                if value.get("type").and_then(Value::as_str) == Some("content_block_start")
                    && value.pointer("/content_block/type").and_then(Value::as_str)
                        == Some("tool_use")
                {
                    self.active_tool = value
                        .pointer("/content_block/name")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    continue;
                }
                if let Some(fragment) = value.pointer("/delta/partial_json").and_then(Value::as_str)
                {
                    events.push(ModelStreamEvent::ToolArgumentsDelta {
                        name: self.active_tool.clone().unwrap_or_default(),
                        json_fragment: fragment.into(),
                    });
                    continue;
                }
            }
            if let Some(event) = parse_provider_event(self.protocol, &value) {
                events.push(event);
            }
        }
        Ok(events)
    }

    pub fn finish(&mut self) -> AdapterResult<Vec<ModelStreamEvent>> {
        if self.pending.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.pending.push('\n');
        self.push(&[])
    }
}

#[derive(Debug, Clone)]
pub struct UnifiedModelClient {
    profile_id: Uuid,
    protocol: ProviderProtocol,
    base_url: Url,
    model: String,
    credential: Option<String>,
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
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(MODEL_REQUEST_TIMEOUT)
                .build()
                .map_err(|error| AdapterError::Llm(error.to_string()))?,
        })
    }

    async fn send_with_retry(
        &self,
        endpoint: &Url,
        body: &Value,
        on_event: &mut impl FnMut(ModelStreamEvent),
    ) -> AdapterResult<reqwest::Response> {
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
                        on_event(ModelStreamEvent::Retrying {
                            attempt: retries,
                            delay_ms,
                            message: format!("model provider returned {status}"),
                        });
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                    let preview: String = response_body.chars().take(1_200).collect();
                    return Err(AdapterError::Llm(format!("{status}: {preview}")));
                }
                Err(error) if retries < MODEL_MAX_RETRIES => {
                    retries += 1;
                    let delay_ms = 1_000_u64 << (retries - 1);
                    on_event(ModelStreamEvent::Retrying {
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

    async fn send_with_retry_v2(
        &self,
        endpoint: &Url,
        body: &Value,
        on_event: &mut impl FnMut(ModelStreamEventV2),
    ) -> AdapterResult<reqwest::Response> {
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
                        on_event(ModelStreamEventV2::Retrying {
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
                    on_event(ModelStreamEventV2::Retrying {
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

    pub async fn stream_with_v2(
        &self,
        request: ModelRequestV2,
        mut on_event: impl FnMut(ModelStreamEventV2),
    ) -> AdapterResult<()> {
        let provider_request =
            build_provider_request_v2(self.protocol, self.base_url.clone(), &self.model, &request)?;
        let response = self
            .send_with_retry_v2(
                &provider_request.endpoint,
                &provider_request.body,
                &mut on_event,
            )
            .await?;
        let mut decoder = ProviderStreamDecoderV2::for_request(self.protocol, &request);
        let mut bytes = response.bytes_stream();
        let mut emitted_event = false;
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(stream_error) if !emitted_event => {
                    let mut fallback_body = provider_request.body.clone();
                    fallback_body["stream"] = Value::Bool(false);
                    let fallback = self
                        .send_with_retry_v2(
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
                        parse_provider_response_v2_for_request(self.protocol, &value, &request)?
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

    pub async fn stream_with(
        &self,
        request: ModelRequest,
        mut on_event: impl FnMut(ModelStreamEvent),
    ) -> AdapterResult<()> {
        let provider_request =
            build_provider_request(self.protocol, self.base_url.clone(), &self.model, &request)?;
        let response = self
            .send_with_retry(
                &provider_request.endpoint,
                &provider_request.body,
                &mut on_event,
            )
            .await?;
        let mut decoder = ProviderStreamDecoder::new(self.protocol);
        let mut bytes = response.bytes_stream();
        let mut emitted_event = false;
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(stream_error) if !emitted_event => {
                    let mut fallback_body = provider_request.body.clone();
                    fallback_body["stream"] = Value::Bool(false);
                    let fallback = self.send_with_retry(
                        &provider_request.endpoint,
                        &fallback_body,
                        &mut on_event,
                    ).await.map_err(|fallback_error| {
                        AdapterError::Llm(format!(
                            "{} stream failed ({stream_error}); non-streaming fallback failed: {fallback_error}",
                            provider_request.endpoint
                        ))
                    })?;
                    let status = fallback.status();
                    let body = fallback.text().await.map_err(|fallback_error| {
                        AdapterError::Llm(format!(
                            "{} stream failed ({stream_error}); error reading non-streaming fallback: {fallback_error}",
                            provider_request.endpoint
                        ))
                    })?;
                    if !status.is_success() {
                        return Err(AdapterError::Llm(format!(
                            "{} stream failed ({stream_error}); non-streaming fallback returned {status}: {body}",
                            provider_request.endpoint
                        )));
                    }
                    let value: Value = serde_json::from_str(&body).map_err(|fallback_error| {
                        AdapterError::Llm(format!(
                            "{} stream failed ({stream_error}); non-streaming fallback returned invalid JSON: {fallback_error}",
                            provider_request.endpoint
                        ))
                    })?;
                    for event in parse_provider_response(
                        self.protocol,
                        &value,
                        request.tool_name.as_deref(),
                    )? {
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

    pub async fn probe(&self) -> AdapterResult<ModelProbeResult> {
        let endpoint = provider_endpoint(self.protocol, self.base_url.clone())?;
        let body = match self.protocol {
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
impl ModelProvider for UnifiedModelClient {
    fn profile_id(&self) -> Uuid {
        self.profile_id
    }

    async fn stream(&self, request: ModelRequest) -> AgentResult<Vec<ModelStreamEvent>> {
        let mut events = Vec::new();
        self.stream_with(request, |event| events.push(event))
            .await
            .map_err(|error| AgentError::Model(error.to_string()))?;
        Ok(events)
    }
}

#[async_trait]
impl ModelProviderV2 for UnifiedModelClient {
    fn profile_id(&self) -> Uuid {
        self.profile_id
    }

    async fn stream_v2(&self, request: ModelRequestV2) -> AgentResult<Vec<ModelStreamEventV2>> {
        let mut events = Vec::new();
        self.stream_with_v2(request, |event| events.push(event))
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
        let mut pending = String::new();
        let mut accumulator = SseToolCallAccumulator::new(tool_name);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| AdapterError::Llm(error.to_string()))?;
            pending.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(newline) = pending.find('\n') {
                let line = pending[..newline].trim().to_owned();
                pending.drain(..=newline);
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

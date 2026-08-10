use futures_util::StreamExt;
use omicsops_agent::{ModelRequest, ModelStreamEvent};
use schemars::{JsonSchema, schema_for};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use url::Url;

use crate::{AdapterError, AdapterResult};

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
        ProviderProtocol::OpenAiCompatible => Ok(ProviderRequest {
            endpoint: base_url
                .join("v1/chat/completions")
                .map_err(|error| AdapterError::Llm(error.to_string()))?,
            body: json!({
                "model": model,
                "stream": true,
                "messages": std::iter::once(json!({"role":"system", "content":request.system}))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "tools": tool.into_iter().collect::<Vec<_>>()
            }),
            requires_credential: true,
        }),
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
            Ok(ProviderRequest {
                endpoint: base_url
                    .join("v1/messages")
                    .map_err(|error| AdapterError::Llm(error.to_string()))?,
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
            endpoint: base_url
                .join("api/chat")
                .map_err(|error| AdapterError::Llm(error.to_string()))?,
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

pub fn parse_provider_event(protocol: ProviderProtocol, value: &Value) -> Option<ModelStreamEvent> {
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

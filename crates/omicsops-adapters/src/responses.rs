//! Stateless, host-tool-only Responses transport. No credential discovery or protocol fallback.
use crate::{
    AdapterError, AdapterResult,
    llm::{self, RequestBudget},
};
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use omicsops_agent::{
    ModelContentPart, ModelMessageContent,
    provider::{
        ProviderRequest, ProviderStreamEvent, ProviderUsageAggregation, ProviderUsageSample,
        ProviderUsageState,
    },
};
use omicsops_protocol::{ModelProviderContinuationV4, ModelReplayItemV4, ToolCallV4};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use url::Url;
use uuid::Uuid;

const MAX_LINE: usize = 1024 * 1024;
const MAX_STREAM: usize = 8 * 1024 * 1024;
const MAX_ARGUMENTS: usize = 256 * 1024;
fn invalid(code: &str) -> AdapterError {
    AdapterError::Llm(code.into())
}

// Only reviewed codes cross the host boundary. Provider messages/parameters,
// response bodies and unknown codes can contain secrets or prompt fragments.
fn provider_failure(error: &Value, fallback: &str) -> AdapterError {
    for field in ["code", "type"] {
        let code = match error[field].as_str() {
            Some("context_length_exceeded" | "context_window_exceeded") => {
                "context_length_exceeded: responses"
            }
            Some("rate_limit_exceeded") => "responses_http_429_rate_limit",
            Some(
                "insufficient_quota"
                | "organization_usage_limit_exceeded"
                | "organization_spend_limit_exceeded"
                | "project_spend_limit_exceeded"
                | "billing_not_active"
                | "billing_hard_limit_reached",
            ) => "responses_quota_exhausted",
            Some("invalid_api_key" | "authentication_error") => "responses_http_401",
            Some("permission_denied" | "permission_error" | "insufficient_permissions") => {
                "responses_http_403"
            }
            Some("server_error" | "server_is_overloaded" | "service_unavailable_error") => {
                "responses_http_503"
            }
            Some("content_filter") => "responses_content_filter",
            _ => continue,
        };
        return invalid(code);
    }
    invalid(fallback)
}

async fn http_failure(response: &mut ResponsesHttpStream) -> AdapterError {
    let fallback = format!("responses_http_{}", response.status);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.body.next().await {
        let Ok(chunk) = chunk else {
            return invalid(&fallback);
        };
        if bytes.len().saturating_add(chunk.len()) > MAX_LINE {
            return invalid(&fallback);
        }
        bytes.extend(chunk);
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return invalid(&fallback);
    };
    provider_failure(&value["error"], &fallback)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsesEndpointKind {
    CodexSubscription,
    OpenCodeGo,
}

pub fn responses_endpoint(kind: ResponsesEndpointKind, base: &Url) -> AdapterResult<Url> {
    let (host, path, endpoint) = match kind {
        ResponsesEndpointKind::CodexSubscription => (
            "chatgpt.com",
            "/backend-api",
            "https://chatgpt.com/backend-api/codex/responses",
        ),
        ResponsesEndpointKind::OpenCodeGo => (
            "opencode.ai",
            "/zen/go/v1",
            "https://opencode.ai/zen/go/v1/responses",
        ),
    };
    if base.scheme() != "https"
        || base.host_str() != Some(host)
        || base.port_or_known_default() != Some(443)
        || base.path().trim_end_matches('/') != path
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(invalid("responses_endpoint_not_allowed"));
    }
    Url::parse(endpoint).map_err(|_| invalid("responses_endpoint_invalid"))
}

fn aliases(request: &ProviderRequest) -> AdapterResult<BTreeMap<String, String>> {
    let entries = llm::provider_tool_aliases(request);
    let map: BTreeMap<_, _> = entries.iter().cloned().collect();
    let originals: BTreeSet<_> = entries.iter().map(|(_, id)| id).collect();
    if map.len() != entries.len()
        || originals.len() != entries.len()
        || entries
            .iter()
            .any(|(a, id)| a != id && originals.contains(a))
    {
        return Err(invalid("responses_tool_alias_collision"));
    }
    Ok(map)
}

pub fn build_responses_request(
    kind: ResponsesEndpointKind,
    base: &Url,
    model: &str,
    request: &ProviderRequest,
    budget: Option<RequestBudget>,
    effort: Option<&str>,
) -> AdapterResult<llm::ProviderRequest> {
    let endpoint = responses_endpoint(kind, base)?;
    if model.is_empty() || model.len() > 256 {
        return Err(invalid("responses_model_invalid"));
    }
    let names = aliases(request)?;
    let reverse: BTreeMap<_, _> = names.iter().map(|(a, id)| (id, a)).collect();
    let mut input = Vec::new();
    for message in &request.messages {
        if !matches!(message.role.as_str(), "user" | "assistant") {
            return Err(invalid("responses_message_role_invalid"));
        }
        let text = match &message.content {
            ModelMessageContent::Text(text) => text.clone(),
            ModelMessageContent::Parts(parts) => {
                let mut text = String::new();
                for part in parts {
                    match part {
                        ModelContentPart::Text { text: part } => text.push_str(part),
                        _ => return Err(invalid("responses_images_unsupported")),
                    }
                }
                text
            }
        };
        input.push(json!({"role":message.role,"content":text}));
    }
    let mut calls = BTreeSet::new();
    let mut results = BTreeSet::new();
    let mut reasoning = BTreeSet::new();
    for item in &request.replay {
        input.push(match item {
            ModelReplayItemV4::AssistantText{text} => json!({"role":"assistant","content":text}),
            ModelReplayItemV4::ToolCall{call} => {
                if call.call_id.is_empty() || call.call_id.len()>256 || !call.arguments.is_object() || !calls.insert(&call.call_id) { return Err(invalid("responses_replay_call_invalid")); }
                let name = llm::provider_tool_alias(&call.tool_id);
                json!({"type":"function_call","call_id":call.call_id,"name":name,"arguments":call.arguments.to_string()})
            },
            ModelReplayItemV4::ToolResult{call_id,output} => {
                if !calls.contains(call_id) || !results.insert(call_id) {return Err(invalid("responses_replay_result_orphan"));}
                json!({"type":"function_call_output","call_id":call_id,"output":output})
            },
            ModelReplayItemV4::ResponsesReasoning{id,encrypted_content} => {
                if id.is_empty() || id.len()>256 || encrypted_content.is_empty() || encrypted_content.len()>MAX_ARGUMENTS || !reasoning.insert(id) {return Err(invalid("responses_replay_reasoning_invalid"));}
                json!({"type":"reasoning","id":id,"encrypted_content":encrypted_content,"summary":[]})
            },
        });
    }
    if calls != results {
        return Err(invalid("responses_replay_incomplete"));
    }
    let tools:Vec<_> = request.tools.iter().map(|tool|json!({"type":"function","name":reverse[&tool.id],"description":tool.description,"parameters":tool.input_schema})).collect();
    let mut body = json!({"model":model,"instructions":request.system,"input":input,"tools":tools,"tool_choice":"auto","store":false,"stream":true,"include":["reasoning.encrypted_content"]});
    if let Some(effort) = effort {
        let provider = match kind {
            ResponsesEndpointKind::CodexSubscription => {
                omicsops_core::workspace::ModelProviderKind::OpenAiCodex
            }
            ResponsesEndpointKind::OpenCodeGo => {
                omicsops_core::workspace::ModelProviderKind::OpenAiResponses
            }
        };
        omicsops_core::workspace::validate_subscription_reasoning_effort(
            provider,
            model,
            Some(effort),
        )
        .map_err(invalid)?;
        body["reasoning"] = json!({"effort":effort});
    }
    if let Some(budget) = budget {
        if kind == ResponsesEndpointKind::OpenCodeGo {
            body["max_output_tokens"] = json!(budget.reserved_output_tokens);
        }
        budget.validate_provider_json(&body)?;
    }
    if serde_json::to_vec(&body)
        .map_err(|_| invalid("responses_request_invalid"))?
        .len()
        > MAX_STREAM
    {
        return Err(invalid("responses_request_too_large"));
    }
    Ok(llm::ProviderRequest {
        endpoint,
        body,
        requires_credential: true,
    })
}

struct PendingCall {
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
}
pub struct ResponsesStreamDecoder {
    buffer: Vec<u8>,
    data: String,
    total: usize,
    done: bool,
    aliases: BTreeMap<String, String>,
    calls: BTreeMap<u32, PendingCall>,
    text: String,
    response_id: Option<String>,
    failed: bool,
}
impl ResponsesStreamDecoder {
    pub fn for_request(request: &ProviderRequest) -> Self {
        Self {
            buffer: vec![],
            data: String::new(),
            total: 0,
            done: false,
            aliases: aliases(request).unwrap_or_default(),
            calls: BTreeMap::new(),
            text: String::new(),
            response_id: None,
            failed: false,
        }
    }
    pub fn push(&mut self, chunk: &[u8]) -> AdapterResult<Vec<ProviderStreamEvent>> {
        let result = self.push_inner(chunk);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn push_inner(&mut self, chunk: &[u8]) -> AdapterResult<Vec<ProviderStreamEvent>> {
        if self.failed {
            return Err(invalid("responses_stream_failed"));
        }
        self.total = self.total.saturating_add(chunk.len());
        if self.total > MAX_STREAM {
            return Err(invalid("responses_stream_too_large"));
        }
        self.buffer.extend_from_slice(chunk);
        let mut events = vec![];
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            if end > MAX_LINE {
                return Err(invalid("responses_line_too_large"));
            }
            let bytes: Vec<_> = self.buffer.drain(..=end).collect();
            let line = std::str::from_utf8(&bytes[..end])
                .map_err(|_| invalid("responses_utf8_invalid"))?
                .trim_end_matches('\r');
            if line.is_empty() {
                if !self.data.is_empty() {
                    let data = std::mem::take(&mut self.data);
                    if data == "[DONE]" {
                        if !self.done {
                            return Err(invalid("responses_terminal_missing"));
                        }
                    } else {
                        events.extend(self.decode(&data)?);
                    }
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(data.strip_prefix(' ').unwrap_or(data));
                if self.data.len() > MAX_LINE {
                    return Err(invalid("responses_event_too_large"));
                }
            }
        }
        if self.buffer.len() > MAX_LINE {
            return Err(invalid("responses_line_too_large"));
        }
        Ok(events)
    }
    pub fn finish(&mut self) -> AdapterResult<Vec<ProviderStreamEvent>> {
        if self.failed || !self.done || !self.buffer.is_empty() || !self.data.is_empty() {
            return Err(invalid("responses_terminal_missing"));
        }
        Ok(vec![])
    }
    fn decode(&mut self, data: &str) -> AdapterResult<Vec<ProviderStreamEvent>> {
        if self.done {
            return Err(invalid("responses_duplicate_terminal"));
        }
        let value: Value =
            serde_json::from_str(data).map_err(|_| invalid("responses_json_invalid"))?;
        let mut events = vec![];
        match value["type"]
            .as_str()
            .ok_or_else(|| invalid("responses_event_type_missing"))?
        {
            "response.created" => {
                let id = required(&value["response"], "id")?;
                if self.response_id.replace(id).is_some() {
                    return Err(invalid("responses_duplicate_created"));
                }
            }
            "response.output_text.delta" => {
                let text = required_allow_empty(&value, "delta")?;
                self.text.push_str(&text);
                events.push(ProviderStreamEvent::TextDelta { text });
            }
            "response.output_item.added" => {
                let item = &value["item"];
                match item["type"].as_str() {
                    Some("function_call") => {
                        let index = index(&value)?;
                        let call = PendingCall {
                            item_id: required(item, "id")?,
                            call_id: required(item, "call_id")?,
                            name: required(item, "name")?,
                            arguments: required_allow_empty(item, "arguments")?,
                        };
                        if self.calls.len() >= 16
                            || call.arguments.len() > MAX_ARGUMENTS
                            || self.calls.contains_key(&index)
                            || self
                                .calls
                                .values()
                                .any(|c| c.item_id == call.item_id || c.call_id == call.call_id)
                            || !self.aliases.contains_key(&call.name)
                        {
                            return Err(invalid("responses_call_invalid"));
                        }
                        self.calls.insert(index, call);
                    }
                    Some("message" | "reasoning") => {}
                    _ => return Err(invalid("responses_provider_tool_forbidden")),
                }
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                let index = index(&value)?;
                let call = self
                    .calls
                    .get_mut(&index)
                    .ok_or_else(|| invalid("responses_argument_orphan"))?;
                if value["item_id"].as_str() != Some(&call.item_id) {
                    return Err(invalid("responses_item_changed"));
                }
                if value["type"] == "response.function_call_arguments.delta" {
                    let delta = required_allow_empty(&value, "delta")?;
                    if !delta.is_empty() {
                        events.push(ProviderStreamEvent::ContentProgress {
                            bytes: delta.len().min(u32::MAX as usize) as u32,
                        });
                    }
                    call.arguments.push_str(&delta);
                } else if call.arguments != required_allow_empty(&value, "arguments")? {
                    return Err(invalid("responses_arguments_changed"));
                }
                if call.arguments.len() > MAX_ARGUMENTS {
                    return Err(invalid("responses_arguments_too_large"));
                }
            }
            "response.completed" => return self.complete(&value["response"]),
            "response.incomplete" => {
                return Err(invalid(
                    match value["response"]["incomplete_details"]["reason"].as_str() {
                        Some("max_output_tokens") => "truncated_output: responses_output_limit",
                        Some("content_filter") => "responses_content_filter",
                        _ => "responses_incomplete_unknown",
                    },
                ));
            }
            "response.failed" => {
                return Err(provider_failure(
                    &value["response"]["error"],
                    "responses_generation_failed",
                ));
            }
            "error" => {
                return Err(provider_failure(
                    value.get("error").unwrap_or(&value),
                    "responses_generation_failed",
                ));
            }
            "response.in_progress"
            | "response.output_item.done"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done" => {}
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let delta = required_allow_empty(&value, "delta")?;
                if !delta.is_empty() {
                    events.push(ProviderStreamEvent::ContentProgress {
                        bytes: delta.len().min(u32::MAX as usize) as u32,
                    });
                }
            }
            _ => return Err(invalid("responses_event_unsupported")),
        }
        Ok(events)
    }
    fn complete(&mut self, response: &Value) -> AdapterResult<Vec<ProviderStreamEvent>> {
        if response["status"] != "completed" {
            return Err(invalid("responses_status_incomplete"));
        }
        let id = required(response, "id")?;
        if self.response_id.as_ref().is_some_and(|old| old != &id) {
            return Err(invalid("responses_response_changed"));
        }
        let output = response["output"]
            .as_array()
            .ok_or_else(|| invalid("responses_output_missing"))?;
        let mut items = vec![];
        let mut seen = BTreeSet::new();
        let mut seen_items = BTreeSet::new();
        let mut text = String::new();
        let mut events = vec![];
        for (position, item) in output.iter().enumerate() {
            if let Some(id) = item["id"].as_str() {
                if !seen_items.insert(id) {
                    return Err(invalid("responses_duplicate_item"));
                }
            }
            match item["type"].as_str() {
                Some("function_call") => {
                    let call_id = required(item, "call_id")?;
                    if !seen.insert(call_id.clone()) || seen.len() > 16 {
                        return Err(invalid("responses_duplicate_call"));
                    }
                    let name = required(item, "name")?;
                    let tool_id = self
                        .aliases
                        .get(&name)
                        .ok_or_else(|| invalid("responses_tool_not_granted"))?
                        .clone();
                    let args = required_allow_empty(item, "arguments")?;
                    if args.len() > MAX_ARGUMENTS {
                        return Err(invalid("responses_arguments_too_large"));
                    }
                    let arguments: Value = serde_json::from_str(&args)
                        .map_err(|_| invalid("responses_arguments_invalid"))?;
                    if !arguments.is_object() {
                        return Err(invalid("responses_arguments_invalid"));
                    }
                    let mut index = position as u32;
                    if let Some((i, old)) = self.calls.iter().find(|(_, c)| c.call_id == call_id) {
                        if item["id"].as_str() != Some(&old.item_id)
                            || old.name != name
                            || old.arguments != args
                        {
                            return Err(invalid("responses_call_changed"));
                        }
                        index = *i;
                    }
                    events.push(ProviderStreamEvent::ToolCallStarted {
                        call_id: call_id.clone(),
                        index,
                        tool_id: tool_id.clone(),
                    });
                    events.push(ProviderStreamEvent::ToolArgumentsDelta {
                        call_id: call_id.clone(),
                        index,
                        arguments: args,
                    });
                    events.push(ProviderStreamEvent::ToolCallCompleted {
                        call_id: call_id.clone(),
                        index,
                    });
                    items.push(ModelReplayItemV4::ToolCall {
                        call: ToolCallV4 {
                            call_id,
                            tool_id,
                            arguments,
                        },
                    });
                }
                Some("reasoning") => {
                    if let Some(encrypted_content) = item["encrypted_content"].as_str() {
                        items.push(ModelReplayItemV4::ResponsesReasoning {
                            id: required(item, "id")?,
                            encrypted_content: encrypted_content.into(),
                        });
                    }
                }
                Some("message") => {
                    if item["role"] != "assistant" {
                        return Err(invalid("responses_output_role_invalid"));
                    }
                    for part in item["content"]
                        .as_array()
                        .ok_or_else(|| invalid("responses_content_missing"))?
                    {
                        if part["type"] != "output_text" {
                            return Err(invalid("responses_content_unsupported"));
                        }
                        text.push_str(&required_allow_empty(part, "text")?);
                    }
                }
                _ => return Err(invalid("responses_provider_tool_forbidden")),
            }
        }
        if self.calls.values().any(|c| !seen.contains(&c.call_id)) || !text.starts_with(&self.text)
        {
            return Err(invalid("responses_output_truncated"));
        }
        if text.len() > self.text.len() {
            events.insert(
                0,
                ProviderStreamEvent::TextDelta {
                    text: text[self.text.len()..].into(),
                },
            );
        }
        if !text.is_empty() {
            items.push(ModelReplayItemV4::AssistantText { text });
        }
        let continuation = ModelProviderContinuationV4 { items };
        continuation
            .validate()
            .map_err(|_| invalid("responses_continuation_invalid"))?;
        events.push(ProviderStreamEvent::Continuation { continuation });
        if let Some(usage) = response.get("usage").filter(|v| v.is_object()) {
            let counter = |key: &str| usage.get(key).and_then(Value::as_u64);
            events.push(ProviderStreamEvent::UsageObserved {
                sample: ProviderUsageSample {
                    sample_index: 0,
                    aggregation: ProviderUsageAggregation::Cumulative,
                    state: ProviderUsageState::Final,
                    input_tokens: counter("input_tokens"),
                    context_tokens: counter("input_tokens"),
                    output_tokens: counter("output_tokens"),
                    reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"].as_u64(),
                    cache_read_input_tokens: usage["input_tokens_details"]["cached_tokens"]
                        .as_u64(),
                    cache_creation_input_tokens: None,
                    reported_total_tokens: counter("total_tokens"),
                },
            });
        }
        events.push(ProviderStreamEvent::Completed);
        self.done = true;
        Ok(events)
    }
}
fn required(value: &Value, key: &str) -> AdapterResult<String> {
    let s = required_allow_empty(value, key)?;
    if s.is_empty() || s.len() > 256 {
        Err(invalid("responses_identifier_invalid"))
    } else {
        Ok(s)
    }
}
fn required_allow_empty(value: &Value, key: &str) -> AdapterResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid("responses_field_missing"))
}
fn index(value: &Value) -> AdapterResult<u32> {
    value["output_index"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| invalid("responses_index_invalid"))
}

pub struct ResponsesAuthorization {
    token: String,
    account_id: Option<String>,
}
impl ResponsesAuthorization {
    pub fn bearer(token: String, account_id: Option<String>) -> Self {
        Self { token, account_id }
    }
}
pub struct ResponsesHttpStream {
    pub status: u16,
    pub body: Pin<Box<dyn Stream<Item = AdapterResult<Vec<u8>>> + Send>>,
}
#[async_trait]
pub trait ResponsesTransport: Send + Sync {
    async fn get(
        &self,
        _endpoint: Url,
        _headers: HeaderMap,
        _timeout: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        Err(invalid("responses_discovery_unavailable"))
    }
    async fn post(
        &self,
        endpoint: Url,
        headers: HeaderMap,
        body: Value,
        timeout: Duration,
    ) -> AdapterResult<ResponsesHttpStream>;
}
struct ReqwestTransport {
    client: reqwest::Client,
}
#[async_trait]
impl ResponsesTransport for ReqwestTransport {
    async fn get(
        &self,
        endpoint: Url,
        headers: HeaderMap,
        timeout: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        let response = self
            .client
            .get(endpoint)
            .headers(headers)
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| invalid("responses_discovery_network_failed"))?;
        Ok(ResponsesHttpStream {
            status: response.status().as_u16(),
            body: Box::pin(response.bytes_stream().map(|r| {
                r.map(|b| b.to_vec())
                    .map_err(|_| invalid("responses_discovery_network_failed"))
            })),
        })
    }
    async fn post(
        &self,
        endpoint: Url,
        headers: HeaderMap,
        body: Value,
        timeout: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        let response = self
            .client
            .post(endpoint)
            .headers(headers)
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| invalid("responses_network_failed"))?;
        Ok(ResponsesHttpStream {
            status: response.status().as_u16(),
            body: Box::pin(response.bytes_stream().map(|r| {
                r.map(|b| b.to_vec())
                    .map_err(|_| invalid("responses_stream_network_failed"))
            })),
        })
    }
}
#[derive(Clone)]
pub struct ResponsesHttpClient {
    kind: ResponsesEndpointKind,
    base: Url,
    model: String,
    budget: Option<RequestBudget>,
    session_id: Uuid,
    transport: Arc<dyn ResponsesTransport>,
    timeout: Duration,
    effort: Option<String>,
}
impl ResponsesHttpClient {
    pub fn with_session_id(mut self, id: Uuid) -> Self {
        self.session_id = id;
        self
    }
    pub fn with_request_budget(mut self, budget: RequestBudget) -> Self {
        self.budget = Some(budget);
        self
    }
    pub fn request_budget(&self) -> Option<RequestBudget> {
        self.budget
    }
    pub async fn list_go_models(
        &self,
        authorization: ResponsesAuthorization,
    ) -> AdapterResult<Vec<String>> {
        if self.kind != ResponsesEndpointKind::OpenCodeGo || authorization.account_id.is_some() {
            return Err(invalid("responses_discovery_not_allowed"));
        }
        responses_endpoint(self.kind, &self.base)?;
        let endpoint = Url::parse("https://opencode.ai/zen/go/v1/models")
            .map_err(|_| invalid("responses_endpoint_invalid"))?;
        let mut headers = HeaderMap::new();
        let mut bearer = HeaderValue::from_str(&format!("Bearer {}", authorization.token))
            .map_err(|_| invalid("responses_authorization_invalid"))?;
        bearer.set_sensitive(true);
        headers.insert("authorization", bearer);
        headers.insert(
            "user-agent",
            HeaderValue::from_static(concat!("OmicsOps/", env!("CARGO_PKG_VERSION"))),
        );
        headers.insert(
            "x-opencode-session",
            HeaderValue::from_str(&self.session_id.to_string())
                .map_err(|_| invalid("go_session_invalid"))?,
        );
        tokio::time::timeout(Duration::from_secs(15), async {
            let mut response = self
                .transport
                .get(endpoint, headers, Duration::from_secs(15))
                .await?;
            if response.status != 200 {
                return Err(invalid(&format!(
                    "responses_discovery_http_{}",
                    response.status
                )));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.body.next().await {
                let chunk = chunk?;
                if bytes.len().saturating_add(chunk.len()) > MAX_LINE {
                    return Err(invalid("responses_discovery_too_large"));
                }
                bytes.extend(chunk);
            }
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| invalid("responses_discovery_invalid"))?;
            let entries = value
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("responses_discovery_invalid"))?;
            let mut models = Vec::new();
            for entry in entries {
                let id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty() && id.len() <= 256)
                    .ok_or_else(|| invalid("responses_discovery_invalid"))?;
                models.push(id.to_owned());
            }
            models.sort();
            models.dedup();
            Ok(models)
        })
        .await
        .map_err(|_| invalid("responses_discovery_timeout"))?
    }
    pub fn new(
        kind: ResponsesEndpointKind,
        base: Url,
        model: String,
        budget: Option<RequestBudget>,
        session_id: Uuid,
    ) -> AdapterResult<Self> {
        responses_endpoint(kind, &base)?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .build()
            .map_err(|_| invalid("responses_client_failed"))?;
        Ok(Self {
            kind,
            base,
            model,
            budget,
            session_id,
            transport: Arc::new(ReqwestTransport { client }),
            timeout: Duration::from_secs(180),
            effort: None,
        })
    }
    pub fn with_transport(mut self, transport: Arc<dyn ResponsesTransport>) -> Self {
        self.transport = transport;
        self
    }
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.effort = effort;
        self
    }
    pub fn shape(&self, request: &ProviderRequest) -> AdapterResult<llm::ProviderRequest> {
        build_responses_request(
            self.kind,
            &self.base,
            &self.model,
            request,
            self.budget,
            self.effort.as_deref(),
        )
    }
    pub async fn stream_once(
        &self,
        request: ProviderRequest,
        authorization: ResponsesAuthorization,
        mut on_event: impl FnMut(ProviderStreamEvent) + Send,
    ) -> AdapterResult<()> {
        let wire = self.shape(&request)?;
        let mut headers = HeaderMap::new();
        let mut bearer = HeaderValue::from_str(&format!("Bearer {}", authorization.token))
            .map_err(|_| invalid("responses_authorization_invalid"))?;
        bearer.set_sensitive(true);
        headers.insert("authorization", bearer);
        headers.insert("accept", HeaderValue::from_static("text/event-stream"));
        headers.insert("user-agent", HeaderValue::from_static("OmicsOps/0.1"));
        match self.kind {
            ResponsesEndpointKind::CodexSubscription => {
                let account = authorization
                    .account_id
                    .ok_or_else(|| invalid("codex_account_missing"))?;
                let mut header = HeaderValue::from_str(&account)
                    .map_err(|_| invalid("codex_account_invalid"))?;
                header.set_sensitive(true);
                headers.insert("chatgpt-account-id", header);
                headers.insert("originator", HeaderValue::from_static("omicsops"));
            }
            ResponsesEndpointKind::OpenCodeGo => {
                if authorization.account_id.is_some() {
                    return Err(invalid("go_account_header_forbidden"));
                }
                headers.insert(
                    "x-opencode-session",
                    HeaderValue::from_str(&self.session_id.to_string())
                        .map_err(|_| invalid("go_session_invalid"))?,
                );
            }
        }
        let timeout = self.timeout;
        tokio::time::timeout(timeout, async {
            let mut response = self
                .transport
                .post(wire.endpoint, headers, wire.body, timeout)
                .await?;
            if response.status != 200 {
                return Err(http_failure(&mut response).await);
            }
            let mut decoder = ResponsesStreamDecoder::for_request(&request);
            while let Some(chunk) = response.body.next().await {
                for event in decoder.push(&chunk?)? {
                    on_event(event);
                }
            }
            for event in decoder.finish()? {
                on_event(event);
            }
            Ok(())
        })
        .await
        .map_err(|_| invalid("responses_timeout"))?
    }
}

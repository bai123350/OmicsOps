//! Explicit model backend routing. Secrets are acquired at the subscription send boundary.
use crate::{
    AdapterError, AdapterResult,
    claude_code::{ClaudeCodeClient, ClaudeProcessRunner, resolve_claude_executable},
    codex_auth::CodexCredentialCoordinator,
    credentials::CredentialVault,
    llm::{
        ModelProbeResult, ProviderProtocol, RequestBudget, RequestBudgetMetrics, UnifiedModelClient,
    },
    responses::{
        ResponsesAuthorization, ResponsesEndpointKind, ResponsesHttpClient, ResponsesTransport,
    },
};
use omicsops_agent::{
    ModelMessage, ModelMessageContent,
    provider::{ProviderRequest, ProviderStreamEvent},
};
use omicsops_core::workspace::{
    ModelProfile, ModelProviderKind, validate_subscription_profile_fields,
};
use omicsops_dto::{ModelDiscoveryResult, ModelDiscoverySource};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use url::Url;
use uuid::Uuid;

fn error(code: &str) -> AdapterError {
    AdapterError::Llm(code.into())
}
pub struct ModelClientServices {
    pub vault: Arc<dyn CredentialVault>,
    pub codex: Arc<CodexCredentialCoordinator>,
    pub claude: Arc<dyn ClaudeProcessRunner>,
}
#[derive(Clone)]
pub struct CodexModelClient {
    transport: ResponsesHttpClient,
    profile: ModelProfile,
    services: Arc<ModelClientServices>,
    fast: Option<bool>,
}
#[derive(Clone)]
pub struct OpenCodeGoModelClient {
    transport: ResponsesHttpClient,
    profile: ModelProfile,
    services: Arc<ModelClientServices>,
    fast: Option<bool>,
}
#[derive(Clone)]
pub enum ModelClient {
    Http(UnifiedModelClient),
    Codex(CodexModelClient),
    Responses(OpenCodeGoModelClient),
    Claude(ClaudeCodeClient),
}
impl From<UnifiedModelClient> for ModelClient {
    fn from(client: UnifiedModelClient) -> Self {
        Self::Http(client)
    }
}
impl ModelClient {
    pub fn supports_native_replay(&self) -> bool {
        !matches!(self, Self::Http(_))
    }
    pub fn from_profile(
        profile: &ModelProfile,
        services: Arc<ModelClientServices>,
    ) -> AdapterResult<Self> {
        validate_subscription_profile_fields(profile).map_err(|code| error(&code))?;
        if profile.fast_mode == Some(true)
            && !omicsops_core::workspace::supports_fast_mode(
                profile.provider,
                &profile.base_url,
                &profile.model,
            )
        {
            return Err(error("subscription_fast_mode_unsupported"));
        }
        let base = Url::parse(&profile.base_url).map_err(|_| error("model_endpoint_invalid"))?;
        let client = match profile.provider {
            ModelProviderKind::OpenAiCompatible
            | ModelProviderKind::Anthropic
            | ModelProviderKind::Ollama => {
                let protocol = match profile.provider {
                    ModelProviderKind::OpenAiCompatible => ProviderProtocol::OpenAiCompatible,
                    ModelProviderKind::Anthropic => ProviderProtocol::Anthropic,
                    _ => ProviderProtocol::Ollama,
                };
                let credential = profile
                    .credential_reference
                    .as_deref()
                    .map(|r| services.vault.get(r))
                    .transpose()?
                    .flatten();
                Self::Http(UnifiedModelClient::new(
                    profile.id,
                    protocol,
                    base,
                    profile.model.clone(),
                    credential,
                )?)
            }
            ModelProviderKind::OpenAiCodex => Self::Codex(CodexModelClient {
                transport: ResponsesHttpClient::new(
                    ResponsesEndpointKind::CodexSubscription,
                    base,
                    profile.model.clone(),
                    None,
                    profile.id,
                )?,
                profile: profile.clone(),
                services,
                fast: None,
            }),
            ModelProviderKind::OpenAiResponses => Self::Responses(OpenCodeGoModelClient {
                transport: ResponsesHttpClient::new(
                    ResponsesEndpointKind::OpenCodeGo,
                    base,
                    profile.model.clone(),
                    None,
                    profile.id,
                )?,
                profile: profile.clone(),
                services,
                fast: None,
            }),
            ModelProviderKind::ClaudeCode => Self::Claude(ClaudeCodeClient::new(
                profile.id,
                resolve_claude_executable(profile.cli_executable.as_deref())?,
                profile.model.clone(),
                services.claude.clone(),
            )?),
        };
        client
            .with_reasoning_effort(profile.reasoning_effort.clone())
            .map(|client| client.with_fast_mode(profile.fast_mode))
    }
    pub fn with_responses_transport(mut self, transport: Arc<dyn ResponsesTransport>) -> Self {
        match &mut self {
            Self::Codex(c) => c.transport = c.transport.clone().with_transport(transport),
            Self::Responses(c) => c.transport = c.transport.clone().with_transport(transport),
            _ => {}
        }
        self
    }
    pub fn with_session_id(mut self, id: Uuid) -> Self {
        match &mut self {
            Self::Http(c) => *c = c.clone().with_session_id(id),
            Self::Codex(c) => c.transport = c.transport.clone().with_session_id(id),
            Self::Responses(c) => c.transport = c.transport.clone().with_session_id(id),
            Self::Claude(_) => {}
        }
        self
    }
    pub fn with_request_budget(mut self, budget: RequestBudget) -> Self {
        match &mut self {
            Self::Http(c) => *c = c.clone().with_request_budget(budget),
            Self::Codex(c) => c.transport = c.transport.clone().with_request_budget(budget),
            Self::Responses(c) => c.transport = c.transport.clone().with_request_budget(budget),
            Self::Claude(c) => *c = c.clone().with_request_budget(budget),
        }
        self
    }
    pub fn request_budget(&self) -> Option<RequestBudget> {
        match self {
            Self::Http(c) => c.request_budget(),
            Self::Codex(c) => c.transport.request_budget(),
            Self::Responses(c) => c.transport.request_budget(),
            Self::Claude(c) => c.request_budget(),
        }
    }
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> AdapterResult<Self> {
        match &mut self {
            Self::Http(c) => *c = c.clone().with_reasoning_effort(effort)?,
            Self::Claude(_) if effort.is_some() => {
                return Err(error("claude_reasoning_effort_unsupported"));
            }
            Self::Claude(_) => {}
            Self::Codex(c) => {
                validate_responses_effort(effort.as_deref())?;
                c.transport = c.transport.clone().with_reasoning_effort(effort);
            }
            Self::Responses(c) => {
                validate_responses_effort(effort.as_deref())?;
                c.transport = c.transport.clone().with_reasoning_effort(effort);
            }
        }
        Ok(self)
    }
    pub fn with_fast_mode(mut self, fast: Option<bool>) -> Self {
        match &mut self {
            Self::Http(c) => *c = c.clone().with_fast_mode(fast),
            Self::Codex(c) => c.fast = fast,
            Self::Responses(c) => c.fast = fast,
            Self::Claude(c) => *c = c.clone().with_fast_mode(fast),
        }
        self
    }
    pub fn has_image_budget(&self) -> bool {
        matches!(self, Self::Http(c) if c.has_image_budget())
    }
    pub fn validate_request(&self, request: &ProviderRequest) -> AdapterResult<()> {
        self.measure_model_request(request).map(|_| ())
    }
    pub fn measure_model_request(
        &self,
        request: &ProviderRequest,
    ) -> AdapterResult<RequestBudgetMetrics> {
        let wire = match self {
            Self::Http(c) => return c.measure_model_request(request),
            Self::Claude(c) => return c.measure_model_request(request),
            Self::Codex(c) => {
                if c.fast == Some(true) {
                    return Err(error("subscription_fast_mode_unsupported"));
                }
                c.transport.shape(request)?
            }
            Self::Responses(c) => {
                if c.fast == Some(true) {
                    return Err(error("subscription_fast_mode_unsupported"));
                }
                c.transport.shape(request)?
            }
        };
        let bytes = serde_json::to_vec(&wire.body)
            .map_err(|_| error("model_request_invalid"))?
            .len() as u64;
        Ok(RequestBudgetMetrics {
            serialized_request_bytes: bytes,
            text_shape_bytes: bytes,
            image_payload_bytes: 0,
            image_count: 0,
            image_bound_tokens: None,
        })
    }
    pub async fn stream_with_provider(
        &self,
        request: ProviderRequest,
        on_event: impl FnMut(ProviderStreamEvent) + Send,
    ) -> AdapterResult<()> {
        self.stream(request, on_event, false, false).await
    }
    pub async fn stream_with_provider_v4(
        &self,
        request: ProviderRequest,
        on_event: impl FnMut(ProviderStreamEvent) + Send,
    ) -> AdapterResult<()> {
        self.stream(request, on_event, true, false).await
    }
    pub async fn stream_with_provider_once(
        &self,
        request: ProviderRequest,
        on_event: impl FnMut(ProviderStreamEvent) + Send,
    ) -> AdapterResult<()> {
        self.stream(request, on_event, false, true).await
    }
    async fn stream(
        &self,
        request: ProviderRequest,
        mut on_event: impl FnMut(ProviderStreamEvent) + Send,
        v4: bool,
        once: bool,
    ) -> AdapterResult<()> {
        self.validate_request(&request)?;
        match self {
            Self::Http(c) => {
                if once {
                    c.stream_with_provider_once(request, on_event).await
                } else if v4 {
                    c.stream_with_provider_v4(request, on_event).await
                } else {
                    c.stream_with_provider(request, on_event).await
                }
            }
            Self::Claude(c) => c.stream_once(request, on_event).await,
            Self::Responses(c) => {
                response_transport(&c.transport, v4)
                    .stream_once(request, go_authorization(c)?, on_event)
                    .await
            }
            Self::Codex(c) => {
                let reference = c
                    .profile
                    .credential_reference
                    .as_deref()
                    .ok_or_else(|| error("codex_login_required"))?;
                let binding = c
                    .profile
                    .subscription_account_ref
                    .ok_or_else(|| error("codex_login_required"))?;
                let snapshot = c.services.codex.access_snapshot(reference, binding).await?;
                let transport = response_transport(&c.transport, v4);
                let mut observed = false;
                let result = transport
                    .stream_once(request.clone(), snapshot.authorization(), |event| {
                        observed = true;
                        on_event(event);
                    })
                    .await;
                if !once
                    && !observed
                    && matches!(&result,Err(AdapterError::Llm(code)) if code=="responses_http_401")
                {
                    let refreshed = c
                        .services
                        .codex
                        .refresh_after_unauthorized(reference, binding, &snapshot)
                        .await?;
                    transport
                        .stream_once(request, refreshed.into_responses_authorization(), on_event)
                        .await
                } else {
                    result
                }
            }
        }
    }
    pub async fn probe(&self) -> AdapterResult<ModelProbeResult> {
        if let Self::Http(c) = self {
            return c.probe().await;
        }
        let started = Instant::now();
        let request = ProviderRequest {
            system: "Connection probe. Reply with OK.".into(),
            messages: vec![ModelMessage {
                role: "user".into(),
                content: ModelMessageContent::Text("OK".into()),
            }],
            tools: vec![],
            require_strict_json_fallback: false,
            replay: vec![],
        };
        let mut text = String::new();
        self.stream_with_provider_once(request, |event| {
            if let ProviderStreamEvent::TextDelta { text: delta } = event {
                if text.len() < 640 {
                    text.extend(delta.chars().take(160));
                }
            }
        })
        .await?;
        if text.trim().is_empty() {
            return Err(error("model_probe_empty_response"));
        }
        let (endpoint, protocol, model) = match self {
            Self::Codex(c) => (
                "https://chatgpt.com/backend-api/codex/responses",
                "open_ai_codex",
                c.profile.model.clone(),
            ),
            Self::Responses(c) => (
                "https://opencode.ai/zen/go/v1/responses",
                "open_ai_responses",
                c.profile.model.clone(),
            ),
            Self::Claude(c) => (
                "claude-code://local",
                "claude_code",
                c.configured_model().into(),
            ),
            Self::Http(_) => unreachable!(),
        };
        Ok(ModelProbeResult {
            endpoint: endpoint.into(),
            protocol: protocol.into(),
            model,
            latency_ms: started.elapsed().as_millis(),
            response_preview: text.chars().take(160).collect(),
        })
    }
    pub async fn list_models(&self) -> AdapterResult<Vec<String>> {
        self.discover_models()
            .await
            .map(|discovery| discovery.models)
    }
    pub async fn discover_models(&self) -> AdapterResult<ModelDiscoveryResult> {
        let (models, source, can_refresh) = match self {
            Self::Http(c) => (c.list_models().await?, ModelDiscoverySource::Provider, true),
            Self::Responses(c) => (
                c.transport.list_go_models(go_authorization(c)?).await?,
                ModelDiscoverySource::Provider,
                true,
            ),
            Self::Codex(c) => (
                vec![c.profile.model.clone()],
                ModelDiscoverySource::ConfiguredOnly,
                false,
            ),
            Self::Claude(c) => (
                vec![c.configured_model().into()],
                ModelDiscoverySource::ConfiguredOnly,
                false,
            ),
        };
        Ok(ModelDiscoveryResult {
            models,
            source,
            can_refresh,
        })
    }
}
fn validate_responses_effort(effort: Option<&str>) -> AdapterResult<()> {
    if effort
        .is_some_and(|e| !matches!(e, "none" | "minimal" | "low" | "medium" | "high" | "xhigh"))
    {
        return Err(error("responses_effort_invalid"));
    }
    Ok(())
}
fn response_transport(client: &ResponsesHttpClient, v4: bool) -> ResponsesHttpClient {
    if v4 {
        client
            .clone()
            .with_request_timeout(Duration::from_secs(900))
    } else {
        client.clone()
    }
}
fn go_authorization(client: &OpenCodeGoModelClient) -> AdapterResult<ResponsesAuthorization> {
    let reference = client
        .profile
        .credential_reference
        .as_deref()
        .ok_or_else(|| error("go_credential_missing"))?;
    let token = client
        .services
        .vault
        .get(reference)
        .map_err(|_| error("go_credential_unavailable"))?
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| error("go_credential_missing"))?;
    Ok(ResponsesAuthorization::bearer(token, None))
}

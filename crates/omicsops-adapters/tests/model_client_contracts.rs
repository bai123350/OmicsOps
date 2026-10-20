use async_trait::async_trait;
use futures_util::stream;
use omicsops_adapters::{
    AdapterError, AdapterResult,
    claude_code::SystemClaudeProcessRunner,
    codex_auth::*,
    credentials::{CredentialVault, MemoryCredentialVault},
    model_client::{ModelClient, ModelClientServices, ModelDiscoveryClient},
    responses::{ResponsesHttpStream, ResponsesTransport},
};
use omicsops_agent::{
    ModelMessage, ModelMessageContent,
    provider::{ProviderRequest, ProviderStreamEvent},
};
use omicsops_core::workspace::{ModelProfile, ModelProviderKind};
use omicsops_dto::ModelDiscoverySource;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use url::Url;
use uuid::Uuid;

struct Clock;
impl CodexAuthClock for Clock {
    fn now_ms(&self) -> i64 {
        1000
    }
}
struct Auth {
    refreshes: AtomicUsize,
}
#[async_trait]
impl CodexAuthTransport for Auth {
    async fn begin_device(&self) -> AdapterResult<CodexDeviceChallenge> {
        unreachable!()
    }
    async fn poll_device(
        &self,
        _: &CodexDeviceChallenge,
        _: Arc<AtomicBool>,
    ) -> AdapterResult<CodexDevicePoll> {
        unreachable!()
    }
    async fn refresh(&self, old: &CodexCredentialBundle) -> AdapterResult<CodexCredentialBundle> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Ok(bundle(old.account_ref(), 1_000_000))
    }
}
fn bundle(id: Uuid, expiry: i64) -> CodexCredentialBundle {
    CodexCredentialBundle::from_vault_json(&json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","expires_at_ms":expiry,"account_id":"fixture-account","account_ref":id}).to_string()).unwrap()
}
fn profile(kind: ModelProviderKind) -> ModelProfile {
    ModelProfile {
        id: Uuid::new_v4(),
        label: "fixture".into(),
        provider: kind,
        base_url: match kind {
            ModelProviderKind::OpenAiCodex => "https://chatgpt.com/backend-api",
            ModelProviderKind::OpenAiResponses => "https://opencode.ai/zen/go/v1",
            ModelProviderKind::ClaudeCode => "claude-code://local",
            ModelProviderKind::Ollama => "http://127.0.0.1:1",
            _ => "https://fixture.example/v1",
        }
        .into(),
        model: if kind == ModelProviderKind::ClaudeCode {
            "sonnet"
        } else {
            "grok-4.7"
        }
        .into(),
        credential_reference: if matches!(
            kind,
            ModelProviderKind::ClaudeCode | ModelProviderKind::Ollama
        ) {
            None
        } else {
            Some("model/fixture".into())
        },
        supports_tools: true,
        supports_vision: false,
        context_window_tokens: None,
        catalog_capabilities: None,
        reasoning_effort: None,
        fast_mode: None,
        delegated_model_profile_id: None,
        cli_executable: (kind == ModelProviderKind::ClaudeCode)
            .then(|| "C:\\fixture\\claude.exe".into()),
        subscription_account_ref: (kind == ModelProviderKind::OpenAiCodex).then(Uuid::new_v4),
    }
}
fn services() -> (
    Arc<ModelClientServices>,
    Arc<MemoryCredentialVault>,
    Arc<Auth>,
) {
    let vault = Arc::new(MemoryCredentialVault::default());
    let auth = Arc::new(Auth {
        refreshes: AtomicUsize::new(0),
    });
    let codex = Arc::new(CodexCredentialCoordinator::new(
        vault.clone(),
        auth.clone(),
        Arc::new(Clock),
    ));
    (
        Arc::new(ModelClientServices {
            vault: vault.clone(),
            codex,
            claude: Arc::new(SystemClaudeProcessRunner),
        }),
        vault,
        auth,
    )
}
fn request() -> ProviderRequest {
    ProviderRequest {
        system: "Host policy".into(),
        messages: vec![ModelMessage {
            role: "user".into(),
            content: ModelMessageContent::Text("OK".into()),
        }],
        tools: vec![],
        require_strict_json_fallback: false,
        replay: vec![],
    }
}
struct Transport {
    calls: AtomicUsize,
    status: u16,
    disconnected: bool,
}
#[async_trait]
impl ResponsesTransport for Transport {
    async fn post(
        &self,
        url: Url,
        headers: reqwest::header::HeaderMap,
        _: Value,
        _: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(url.scheme(), "https");
        assert!(headers["authorization"].is_sensitive());
        let chunk = if self.disconnected {
            Err(AdapterError::Llm("fixture_disconnect".into()))
        } else {
            Ok(format!("data: {}\n\n", json!({"type":"response.completed","response":{"id":"resp_fixture","status":"completed","output":[{"id":"msg_fixture","type":"message","role":"assistant","content":[{"type":"output_text","text":"OK"}]}]}})).into_bytes())
        };
        Ok(ResponsesHttpStream {
            status: self.status,
            body: Box::pin(stream::iter([chunk])),
        })
    }
}
#[tokio::test]
async fn model_client_routes_without_credential_or_protocol_inference() {
    let (s, vault, _) = services();
    vault.set("model/fixture", "fixture-key").unwrap();
    for kind in [
        ModelProviderKind::OpenAiCompatible,
        ModelProviderKind::Anthropic,
        ModelProviderKind::Ollama,
    ] {
        assert!(matches!(
            ModelClient::from_profile(&profile(kind), s.clone()).unwrap(),
            ModelClient::Http(_)
        ));
    }
    for kind in [
        ModelProviderKind::OpenAiCodex,
        ModelProviderKind::OpenAiResponses,
    ] {
        let p = profile(kind);
        let transport = Arc::new(Transport {
            calls: AtomicUsize::new(0),
            status: 200,
            disconnected: false,
        });
        let client = ModelClient::from_profile(&p, s.clone())
            .unwrap()
            .with_responses_transport(transport.clone());
        assert!(matches!(
            (&client, kind),
            (ModelClient::Codex(_), ModelProviderKind::OpenAiCodex)
                | (
                    ModelClient::Responses(_),
                    ModelProviderKind::OpenAiResponses
                )
        ));
        client.validate_request(&request()).unwrap();
        assert!(
            client
                .measure_model_request(&request())
                .unwrap()
                .serialized_request_bytes
                > 0
        );
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert!(
            client
                .clone()
                .with_fast_mode(Some(true))
                .validate_request(&request())
                .is_err()
        );
        assert!(!client.has_image_budget());
        let mut image_request = request();
        image_request.messages[0].content =
            ModelMessageContent::Parts(vec![omicsops_agent::ModelContentPart::Image {
                media_type: "image/png".into(),
                data_base64: "fixture".into(),
            }]);
        assert!(client.validate_request(&image_request).is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    }
    let mut invalid = profile(ModelProviderKind::OpenAiCodex);
    invalid.base_url = "https://api.openai.com/v1".into();
    assert!(ModelClient::from_profile(&invalid, s).is_err());
}
#[tokio::test]
async fn model_client_once_does_not_retry_accepted_request() {
    for (status, disconnected, expiry) in [
        (401, false, 1_000_000),
        (200, true, 1_000_000),
        (200, false, 1000),
    ] {
        let (s, vault, auth) = services();
        let p = profile(ModelProviderKind::OpenAiCodex);
        vault
            .set(
                "model/fixture",
                &bundle(p.subscription_account_ref.unwrap(), expiry)
                    .to_vault_json()
                    .unwrap(),
            )
            .unwrap();
        let transport = Arc::new(Transport {
            calls: AtomicUsize::new(0),
            status,
            disconnected,
        });
        let client = ModelClient::from_profile(&p, s)
            .unwrap()
            .with_responses_transport(transport.clone());
        let mut events = vec![];
        let result = client
            .stream_with_provider_once(request(), |event| events.push(event))
            .await;
        assert_eq!(result.is_ok(), status == 200 && !disconnected);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            auth.refreshes.load(Ordering::SeqCst),
            usize::from(expiry == 1000)
        );
        if result.is_err() {
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, ProviderStreamEvent::Completed))
            );
        }
    }
}
#[tokio::test]
async fn model_client_probe_and_discovery_preserve_source() {
    let (s, vault, _) = services();
    vault.set("model/fixture", "fixture-key").unwrap();
    let client = ModelClient::from_profile(&profile(ModelProviderKind::OpenAiResponses), s.clone())
        .unwrap()
        .with_responses_transport(Arc::new(Transport {
            calls: AtomicUsize::new(0),
            status: 200,
            disconnected: false,
        }));
    let probe = client.probe().await.unwrap();
    assert_eq!(probe.response_preview, "OK");
    assert!(client.discover_models().await.is_err());
    #[cfg(windows)]
    {
        let client =
            ModelClient::from_profile(&profile(ModelProviderKind::ClaudeCode), s.clone()).unwrap();
        let discovery = client.discover_models().await.unwrap();
        assert_eq!(discovery.source, ModelDiscoverySource::ConfiguredOnly);
        assert!(!discovery.can_refresh);
        assert_eq!(discovery.models, ["sonnet"]);
    }
    let client = ModelClient::from_profile(&profile(ModelProviderKind::OpenAiCodex), s).unwrap();
    let discovery = client.discover_models().await.unwrap();
    assert_eq!(discovery.source, ModelDiscoverySource::ConfiguredOnly);
    assert!(!discovery.can_refresh);
}

#[tokio::test]
async fn model_client_ordinary_auth_retry_is_bounded_and_does_not_fallback() {
    let (services, vault, auth) = services();
    let profile = profile(ModelProviderKind::OpenAiCodex);
    vault
        .set(
            "model/fixture",
            &bundle(profile.subscription_account_ref.unwrap(), 1_000_000)
                .to_vault_json()
                .unwrap(),
        )
        .unwrap();
    let transport = Arc::new(Transport {
        calls: AtomicUsize::new(0),
        status: 401,
        disconnected: false,
    });
    let client = ModelClient::from_profile(&profile, services)
        .unwrap()
        .with_responses_transport(transport.clone());
    let mut events = vec![];
    assert!(
        client
            .stream_with_provider_v4(request(), |event| events.push(event))
            .await
            .is_err()
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    assert_eq!(auth.refreshes.load(Ordering::SeqCst), 1);
    assert!(events.is_empty());
}
struct DiscoveryTransport {
    status: u16,
    value: Value,
    gets: AtomicUsize,
}
#[async_trait]
impl ResponsesTransport for DiscoveryTransport {
    async fn post(
        &self,
        _: Url,
        _: reqwest::header::HeaderMap,
        _: Value,
        _: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        panic!("discovery must not generate")
    }
    async fn get(
        &self,
        endpoint: Url,
        headers: reqwest::header::HeaderMap,
        _: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        assert_eq!(endpoint.as_str(), "https://opencode.ai/zen/go/v1/models");
        assert!(
            !headers.contains_key("authorization"),
            "the public Go model list must not depend on a valid API key"
        );
        assert!(!headers.contains_key("x-api-key"));
        assert_eq!(
            headers["user-agent"],
            concat!("OmicsOps/", env!("CARGO_PKG_VERSION"))
        );
        assert!(headers.contains_key("x-opencode-session"));
        self.gets.fetch_add(1, Ordering::SeqCst);
        Ok(ResponsesHttpStream {
            status: self.status,
            body: Box::pin(stream::iter([Ok(self.value.to_string().into_bytes())])),
        })
    }
}
#[tokio::test]
async fn model_client_discovery_reports_only_valid_provider_data() {
    for (status, value, success) in [
        (
            200,
            json!({"data":[{"id":"grok-4.7"},{"id":"grok-4.7"}]}),
            true,
        ),
        (307, json!({"data":[]}), false),
        (200, json!({"data":[{"id":null}]}), false),
    ] {
        let (services, vault, _) = services();
        vault.set("model/fixture", "fixture-key").unwrap();
        let transport = Arc::new(DiscoveryTransport {
            status,
            value,
            gets: AtomicUsize::new(0),
        });
        let client =
            ModelClient::from_profile(&profile(ModelProviderKind::OpenAiResponses), services)
                .unwrap()
                .with_responses_transport(transport.clone());
        let result = client.discover_models().await;
        assert_eq!(result.is_ok(), success);
        if let Ok(result) = result {
            assert_eq!(result.models, ["grok-4.7"]);
            assert_eq!(result.source, ModelDiscoverySource::Provider);
            assert!(result.can_refresh);
        }
        assert_eq!(transport.gets.load(Ordering::SeqCst), 1);
    }
}

struct UnavailableVault;
impl CredentialVault for UnavailableVault {
    fn get(&self, _: &str) -> AdapterResult<Option<String>> {
        panic!("public Go discovery must not read credentials")
    }
    fn set(&self, _: &str, _: &str) -> AdapterResult<()> {
        panic!("discovery must not write credentials")
    }
    fn delete(&self, _: &str) -> AdapterResult<()> {
        panic!("discovery must not delete credentials")
    }
}

#[tokio::test]
async fn public_go_discovery_is_independent_of_credentials_and_inference_options() {
    for kind in [
        ModelProviderKind::OpenAiCompatible,
        ModelProviderKind::Anthropic,
        ModelProviderKind::OpenAiResponses,
    ] {
        for invalid_options in [false, true] {
            let (services, _, _) = services();
            let services = Arc::new(ModelClientServices {
                vault: Arc::new(UnavailableVault),
                codex: services.codex.clone(),
                claude: services.claude.clone(),
            });
            let mut profile = profile(kind);
            profile.base_url = "https://opencode.ai:443/zen/go/v1/".into();
            if invalid_options {
                profile.fast_mode = Some(true);
                profile.reasoning_effort = Some("not-a-supported-effort".into());
            } else {
                profile.credential_reference = None;
            }
            let transport = Arc::new(DiscoveryTransport {
                status: 200,
                value: json!({"object":"list","data":[
                    {"id":"minimax-m3","object":"model","created":0,"owned_by":"opencode"},
                    {"id":"glm-5.3-flash","object":"model","created":0,"owned_by":"opencode"},
                    {"id":"glm-5.3-flash","object":"model","created":0,"owned_by":"opencode"}
                ]}),
                gets: AtomicUsize::new(0),
            });
            let result = ModelDiscoveryClient::from_profile(&profile, services.clone())
                .unwrap()
                .with_responses_transport(transport.clone())
                .discover_models()
                .await
                .unwrap();
            assert_eq!(result.models, ["glm-5.3-flash", "minimax-m3"]);
            assert_eq!(result.source, ModelDiscoverySource::Provider);
            assert!(result.can_refresh);
            assert_eq!(transport.gets.load(Ordering::SeqCst), 1);
            if invalid_options || kind != ModelProviderKind::OpenAiResponses {
                assert!(ModelClient::from_profile(&profile, services).is_err());
            } else {
                let error = ModelClient::from_profile(&profile, services)
                    .unwrap()
                    .with_responses_transport(transport)
                    .probe()
                    .await
                    .unwrap_err();
                assert!(error.to_string().contains("go_credential_missing"));
            }
        }
    }
}

#[test]
fn public_go_discovery_does_not_bypass_other_endpoints_or_providers() {
    for base in [
        "http://opencode.ai/zen/go/v1",
        "https://opencode.ai:8443/zen/go/v1",
        "https://opencode.ai.example/zen/go/v1",
        "https://opencode.ai/zen/go/v10",
        "https://opencode.ai/zen/v1",
        "https://opencode.ai/zen/go/v1?forward=1",
        "https://opencode.ai/zen/go/v1#fragment",
        "https://user@opencode.ai/zen/go/v1",
    ] {
        let (services, _, _) = services();
        let mut profile = profile(ModelProviderKind::OpenAiCompatible);
        profile.base_url = base.into();
        profile.credential_reference = None;
        assert!(
            ModelDiscoveryClient::from_profile(&profile, services).is_err(),
            "{base}"
        );
    }
    for kind in [
        ModelProviderKind::OpenAiCodex,
        ModelProviderKind::ClaudeCode,
        ModelProviderKind::Ollama,
    ] {
        let (services, _, _) = services();
        let mut profile = profile(kind);
        profile.base_url = "https://opencode.ai/zen/go/v1".into();
        profile.fast_mode = Some(true);
        assert!(ModelDiscoveryClient::from_profile(&profile, services).is_err());
    }
}

#[tokio::test]
async fn public_go_discovery_preserves_empty_lists_and_rejects_failed_or_malformed_responses() {
    for (status, value, expected) in [
        (200, json!({"data":[]}), Some(Vec::<String>::new())),
        (401, json!({"error":"fixture"}), None),
        (500, json!({"data":[]}), None),
        (307, json!({"data":[]}), None),
        (200, json!({"models":[]}), None),
        (200, json!({"data":[{"id":""}]}), None),
        (200, json!({"data":[{"id":"x".repeat(257)}]}), None),
        (200, json!({"data":[{"id":"x".repeat(1024 * 1024)}]}), None),
    ] {
        let (services, _, _) = services();
        let mut profile = profile(ModelProviderKind::OpenAiCompatible);
        profile.base_url = "https://opencode.ai/zen/go/v1".into();
        let result = ModelDiscoveryClient::from_profile(&profile, services)
            .unwrap()
            .with_responses_transport(Arc::new(DiscoveryTransport {
                status,
                value,
                gets: AtomicUsize::new(0),
            }))
            .discover_models()
            .await;
        match expected {
            Some(models) => assert_eq!(result.unwrap().models, models),
            None => assert!(result.is_err()),
        }
    }
}

#[tokio::test]
#[ignore = "requires OMICSOPS_LIVE_GO_DISCOVERY=1 and public OpenCode network access; no credentials or inference"]
async fn live_public_go_discovery_without_credentials() {
    assert_eq!(
        std::env::var("OMICSOPS_LIVE_GO_DISCOVERY").as_deref(),
        Ok("1")
    );
    for kind in [
        ModelProviderKind::OpenAiCompatible,
        ModelProviderKind::Anthropic,
        ModelProviderKind::OpenAiResponses,
    ] {
        let (services, _, _) = services();
        let services = Arc::new(ModelClientServices {
            vault: Arc::new(UnavailableVault),
            codex: services.codex.clone(),
            claude: services.claude.clone(),
        });
        let mut profile = profile(kind);
        profile.base_url = "https://opencode.ai/zen/go/v1".into();
        profile.credential_reference = None;
        let result = ModelDiscoveryClient::from_profile(&profile, services)
            .unwrap()
            .discover_models()
            .await
            .unwrap();
        assert_eq!(result.source, ModelDiscoverySource::Provider);
        assert!(result.can_refresh);
        assert!(!result.models.is_empty());
        println!("{kind:?}: {} public models", result.models.len());
    }
}

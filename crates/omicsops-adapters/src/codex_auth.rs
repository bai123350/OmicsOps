//! Fixed public-client device flow; credentials are serialized only for the OS vault.
//! Endpoint/client constants verified against OpenAI Codex and Wisp Science
//! (AGPL-3.0, commit ade254989da5d395c6334c893637f32c3ef61ba7,
//! https://github.com/xuzhougeng/wisp-science/blob/ade254989da5d395c6334c893637f32c3ef61ba7/crates/wisp-llm/src/codex_auth.rs).
use crate::{
    AdapterError, AdapterResult, credentials::CredentialVault, responses::ResponsesAuthorization,
};
use async_trait::async_trait;
use base64::Engine;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use url::Url;
use uuid::Uuid;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH_BASE: &str = "https://auth.openai.com";
const CALLBACK: &str = "https://auth.openai.com/deviceauth/callback";
fn error(code: &str) -> AdapterError {
    AdapterError::Llm(code.into())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexCredentialBundle {
    access_token: String,
    refresh_token: String,
    expires_at_ms: i64,
    account_id: String,
    account_ref: Uuid,
}
impl CodexCredentialBundle {
    pub fn from_vault_json(raw: &str) -> AdapterResult<Self> {
        if raw.len() > 64 * 1024 {
            return Err(error("codex_credentials_invalid"));
        }
        let bundle: Self =
            serde_json::from_str(raw).map_err(|_| error("codex_credentials_invalid"))?;
        if bundle.access_token.is_empty()
            || bundle.refresh_token.is_empty()
            || bundle.account_id.is_empty()
            || bundle.account_id.len() > 256
            || bundle.expires_at_ms <= 0
            || bundle.account_ref.is_nil()
        {
            return Err(error("codex_credentials_invalid"));
        }
        Ok(bundle)
    }
    pub fn to_vault_json(&self) -> AdapterResult<String> {
        serde_json::to_string(self).map_err(|_| error("codex_credentials_invalid"))
    }
    pub fn account_ref(&self) -> Uuid {
        self.account_ref
    }
    pub fn with_account_ref(mut self, reference: Uuid) -> Self {
        self.account_ref = reference;
        self
    }
    pub fn same_account_as(&self, other: &Self) -> bool {
        self.account_id == other.account_id
    }
    pub fn masked_account_label(&self) -> String {
        format!(
            "账户 {}",
            &hex::encode(Sha256::digest(self.account_id.as_bytes()))[..8]
        )
    }
    fn fingerprint(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.access_token.as_bytes());
        digest.update([0]);
        digest.update(self.refresh_token.as_bytes());
        hex::encode(digest.finalize())
    }
    fn snapshot(&self, generation: u64, coordinator_id: Uuid) -> CodexAccessSnapshot {
        CodexAccessSnapshot {
            token: self.access_token.clone(),
            account_id: self.account_id.clone(),
            account_ref: self.account_ref,
            generation,
            coordinator_id,
        }
    }
}
#[derive(Clone)]
pub struct CodexAccessSnapshot {
    token: String,
    account_id: String,
    account_ref: Uuid,
    generation: u64,
    coordinator_id: Uuid,
}
impl CodexAccessSnapshot {
    pub fn authorization(&self) -> ResponsesAuthorization {
        ResponsesAuthorization::bearer(self.token.clone(), Some(self.account_id.clone()))
    }
    pub fn into_responses_authorization(self) -> ResponsesAuthorization {
        ResponsesAuthorization::bearer(self.token, Some(self.account_id))
    }
    pub fn masked_account_label(&self) -> String {
        format!(
            "账户 {}",
            &hex::encode(Sha256::digest(self.account_id.as_bytes()))[..8]
        )
    }
}
pub struct CodexDeviceChallenge {
    pub device_auth_id: String,
    pub user_code: String,
    pub verification_uri: Url,
    pub interval: Duration,
    pub expires_at_ms: i64,
}
pub enum CodexDevicePoll {
    Pending { next_poll_after: Duration },
    Authorized { bundle: CodexCredentialBundle },
}
pub trait CodexAuthClock: Send + Sync {
    fn now_ms(&self) -> i64;
}
pub struct SystemCodexAuthClock;
impl CodexAuthClock for SystemCodexAuthClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}
#[async_trait]
pub trait CodexAuthTransport: Send + Sync {
    async fn begin_device(&self) -> AdapterResult<CodexDeviceChallenge>;
    async fn poll_device(
        &self,
        challenge: &CodexDeviceChallenge,
        cancelled: Arc<AtomicBool>,
    ) -> AdapterResult<CodexDevicePoll>;
    async fn refresh(&self, bundle: &CodexCredentialBundle)
    -> AdapterResult<CodexCredentialBundle>;
}
// No Debug implementation: form and JSON may contain codes, verifiers or tokens.
pub enum CodexAuthHttpBody {
    Json(Value),
    Form(Vec<(String, String)>),
}
pub struct CodexAuthHttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}
#[async_trait]
pub trait CodexAuthHttpTransport: Send + Sync {
    async fn post(&self, url: Url, body: CodexAuthHttpBody)
    -> AdapterResult<CodexAuthHttpResponse>;
}
struct Http {
    client: reqwest::Client,
}
#[async_trait]
impl CodexAuthHttpTransport for Http {
    async fn post(
        &self,
        url: Url,
        body: CodexAuthHttpBody,
    ) -> AdapterResult<CodexAuthHttpResponse> {
        if url.scheme() != "https"
            || url.host_str() != Some("auth.openai.com")
            || url.port_or_known_default() != Some(443)
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || !matches!(
                url.path(),
                "/api/accounts/deviceauth/usercode"
                    | "/api/accounts/deviceauth/token"
                    | "/oauth/token"
            )
        {
            return Err(error("codex_auth_endpoint_invalid"));
        }
        let request = self
            .client
            .post(url)
            .header("user-agent", "OmicsOps/0.1")
            .header("originator", "omicsops");
        let request = match body {
            CodexAuthHttpBody::Json(value) => request.json(&value),
            CodexAuthHttpBody::Form(pairs) => request.form(&pairs),
        };
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status().as_u16();
        let mut stream = response.bytes_stream();
        let mut body = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(transport_error)?;
            if body.len() + chunk.len() > 64 * 1024 {
                return Err(error("codex_auth_response_too_large"));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(CodexAuthHttpResponse { status, body })
    }
}
pub struct CodexDeviceAuth {
    http: Arc<dyn CodexAuthHttpTransport>,
    clock: Arc<dyn CodexAuthClock>,
}
impl CodexDeviceAuth {
    pub fn new(clock: Arc<dyn CodexAuthClock>) -> AdapterResult<Self> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| error("codex_auth_client_failed"))?;
        Ok(Self {
            http: Arc::new(Http { client }),
            clock,
        })
    }
    pub fn with_http_transport(mut self, http: Arc<dyn CodexAuthHttpTransport>) -> Self {
        self.http = http;
        self
    }
    async fn post(
        &self,
        path: &str,
        body: CodexAuthHttpBody,
    ) -> AdapterResult<CodexAuthHttpResponse> {
        self.http
            .post(
                Url::parse(&format!("{AUTH_BASE}{path}"))
                    .map_err(|_| error("codex_auth_endpoint_invalid"))?,
                body,
            )
            .await
    }
    async fn exchange(
        &self,
        pairs: Vec<(String, String)>,
        old: Option<&CodexCredentialBundle>,
    ) -> AdapterResult<CodexCredentialBundle> {
        let response = self
            .post("/oauth/token", CodexAuthHttpBody::Form(pairs))
            .await?;
        if let Some(code) = response_error_code(&response) {
            return Err(error(code));
        }
        if response.status != 200 {
            return Err(error(if response.status == 400 || response.status == 401 {
                "codex_reauthentication_required"
            } else if response.status == 403 {
                "codex_device_access_denied"
            } else {
                "codex_token_exchange_failed"
            }));
        }
        parse_token_bundle(&response.body, old, self.clock.now_ms())
    }
}
#[async_trait]
impl CodexAuthTransport for CodexDeviceAuth {
    async fn begin_device(&self) -> AdapterResult<CodexDeviceChallenge> {
        let started = self.clock.now_ms();
        let response = self
            .post(
                "/api/accounts/deviceauth/usercode",
                CodexAuthHttpBody::Json(json!({"client_id":CLIENT_ID})),
            )
            .await?;
        if let Some(code) = response_error_code(&response) {
            return Err(error(code));
        }
        if response.status != 200 {
            return Err(error(if matches!(response.status, 401 | 403) {
                "codex_device_access_denied"
            } else {
                "codex_device_login_unavailable"
            }));
        }
        let value = json_body(&response.body)?;
        let interval = value["interval"]
            .as_u64()
            .or_else(|| value["interval"].as_str().and_then(|v| v.parse().ok()))
            .unwrap_or(5);
        if interval == 0 || interval > 900 {
            return Err(error("codex_device_interval_invalid"));
        }
        let lifetime = value["expires_in"]
            .as_i64()
            .filter(|v| *v > 0)
            .unwrap_or(900)
            .min(900);
        Ok(CodexDeviceChallenge {
            device_auth_id: field(&value, "device_auth_id")?,
            user_code: field(
                &value,
                if value.get("user_code").is_some() {
                    "user_code"
                } else {
                    "usercode"
                },
            )?,
            verification_uri: Url::parse("https://auth.openai.com/codex/device")
                .map_err(|_| error("codex_auth_endpoint_invalid"))?,
            interval: Duration::from_secs(interval),
            expires_at_ms: started.saturating_add(lifetime * 1000),
        })
    }
    async fn poll_device(
        &self,
        challenge: &CodexDeviceChallenge,
        cancelled: Arc<AtomicBool>,
    ) -> AdapterResult<CodexDevicePoll> {
        check_login(challenge, &cancelled, self.clock.now_ms())?;
        let response=self.post("/api/accounts/deviceauth/token",CodexAuthHttpBody::Json(json!({"device_auth_id":challenge.device_auth_id,"user_code":challenge.user_code}))).await?;
        check_login(challenge, &cancelled, self.clock.now_ms())?;
        if let Some(code) = response_error_code(&response) {
            return Err(error(code));
        }
        if response.status == 200 {
            let value = json_body(&response.body)?;
            let code = field(&value, "authorization_code")?;
            let verifier = field(&value, "code_verifier")?;
            check_login(challenge, &cancelled, self.clock.now_ms())?;
            let bundle = self
                .exchange(
                    form(&[
                        ("grant_type", "authorization_code"),
                        ("client_id", CLIENT_ID),
                        ("code", &code),
                        ("code_verifier", &verifier),
                        ("redirect_uri", CALLBACK),
                    ]),
                    None,
                )
                .await?;
            check_login(challenge, &cancelled, self.clock.now_ms())?;
            return Ok(CodexDevicePoll::Authorized { bundle });
        }
        let value = if response.body.is_empty() {
            json!({})
        } else {
            json_body(&response.body)?
        };
        let code = value["error"]
            .as_str()
            .or_else(|| value["error"]["code"].as_str());
        let next = match code {
            Some("deviceauth_authorization_pending" | "authorization_pending") => {
                challenge.interval
            }
            Some("slow_down") => challenge.interval.saturating_add(Duration::from_secs(5)),
            None if matches!(response.status, 403 | 404) && value.get("error").is_none() => {
                challenge.interval
            }
            _ if matches!(response.status, 401 | 403) => {
                return Err(error("codex_device_access_denied"));
            }
            _ => return Err(error("codex_device_authorization_failed")),
        };
        Ok(CodexDevicePoll::Pending {
            next_poll_after: next,
        })
    }
    async fn refresh(
        &self,
        bundle: &CodexCredentialBundle,
    ) -> AdapterResult<CodexCredentialBundle> {
        self.exchange(
            form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", &bundle.refresh_token),
            ]),
            Some(bundle),
        )
        .await
    }
}
fn transport_error(cause: reqwest::Error) -> AdapterError {
    error(if cause.is_timeout() {
        "codex_auth_timeout"
    } else if cause.is_connect() {
        "codex_auth_connection_failed"
    } else {
        "codex_auth_network_uncertain"
    })
}
// Recognize fixed categories only. OAuth bodies, arbitrary provider messages,
// proxy addresses and HTML never cross this boundary.
fn response_error_code(response: &CodexAuthHttpResponse) -> Option<&'static str> {
    let prefix = String::from_utf8_lossy(&response.body[..response.body.len().min(256)])
        .trim_start()
        .to_ascii_lowercase();
    if prefix.starts_with("<!doctype html")
        || prefix.starts_with("<html")
        || prefix.contains("<head>")
    {
        return Some("codex_device_web_verification_required");
    }
    let value: Option<Value> = serde_json::from_slice(&response.body).ok();
    if value.as_ref().is_some_and(|v| {
        v["error"]["code"].as_str().or_else(|| v["error"].as_str())
            == Some("unsupported_country_region_territory")
    }) {
        return Some("codex_device_region_unsupported");
    }
    (response.status == 429).then_some("codex_device_rate_limited")
}
fn check_login(
    challenge: &CodexDeviceChallenge,
    cancel: &AtomicBool,
    now: i64,
) -> AdapterResult<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(error("codex_login_cancelled"))
    } else if now >= challenge.expires_at_ms {
        Err(error("codex_login_expired"))
    } else {
        Ok(())
    }
}
fn form(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn json_body(body: &[u8]) -> AdapterResult<Value> {
    if body.len() > 64 * 1024 {
        return Err(error("codex_auth_response_too_large"));
    }
    serde_json::from_slice(body).map_err(|_| error("codex_auth_response_invalid"))
}
fn field(value: &Value, key: &str) -> AdapterResult<String> {
    value[key]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 16 * 1024)
        .map(str::to_owned)
        .ok_or_else(|| error("codex_auth_field_invalid"))
}
fn parse_token_bundle(
    body: &[u8],
    old: Option<&CodexCredentialBundle>,
    now: i64,
) -> AdapterResult<CodexCredentialBundle> {
    let value = json_body(body)?;
    let access_token = field(&value, "access_token")?;
    let refresh_token = value["refresh_token"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .or_else(|| old.map(|b| b.refresh_token.clone()))
        .ok_or_else(|| error("codex_refresh_token_missing"))?;
    let payload = access_token
        .split('.')
        .nth(1)
        .ok_or_else(|| error("codex_token_claims_invalid"))?;
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| error("codex_token_claims_invalid"))?,
    )
    .map_err(|_| error("codex_token_claims_invalid"))?;
    let account_id = field(&claims["https://api.openai.com/auth"], "chatgpt_account_id")?;
    let relative = value["expires_in"]
        .as_i64()
        .filter(|v| *v > 0)
        .map(|v| now.saturating_add(v.saturating_mul(1000)));
    let absolute = claims["exp"]
        .as_i64()
        .filter(|v| *v > 0)
        .map(|v| v.saturating_mul(1000));
    let expires_at_ms = match (relative, absolute) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        _ => return Err(error("codex_token_expiry_missing")),
    };
    if expires_at_ms <= now || old.is_some_and(|b| b.account_id != account_id) {
        return Err(error("codex_account_or_expiry_changed"));
    }
    Ok(CodexCredentialBundle {
        access_token,
        refresh_token,
        expires_at_ms,
        account_id,
        account_ref: old.map(|b| b.account_ref).unwrap_or_else(Uuid::new_v4),
    })
}

#[derive(Default)]
struct RefreshState {
    poisoned_fingerprint: Option<String>,
    generation: u64,
}
pub struct CodexCredentialCoordinator {
    coordinator_id: Uuid,
    vault: Arc<dyn CredentialVault>,
    transport: Arc<dyn CodexAuthTransport>,
    clock: Arc<dyn CodexAuthClock>,
    locks: Mutex<HashMap<(String, Uuid), Arc<tokio::sync::Mutex<RefreshState>>>>,
    mutation_lock: Arc<tokio::sync::Mutex<()>>,
}
impl CodexCredentialCoordinator {
    pub fn new(
        vault: Arc<dyn CredentialVault>,
        transport: Arc<dyn CodexAuthTransport>,
        clock: Arc<dyn CodexAuthClock>,
    ) -> Self {
        Self {
            coordinator_id: Uuid::new_v4(),
            vault,
            transport,
            clock,
            locks: Mutex::new(HashMap::new()),
            mutation_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
    /// Share the desktop credential mutation lock with login, disconnect and deletion.
    pub fn with_mutation_lock(mut self, lock: Arc<tokio::sync::Mutex<()>>) -> Self {
        self.mutation_lock = lock;
        self
    }
    fn read(&self, reference: &str, account_ref: Uuid) -> AdapterResult<CodexCredentialBundle> {
        let raw = self
            .vault
            .get(reference)
            .map_err(|_| error("codex_vault_read_failed"))?
            .ok_or_else(|| error("codex_reauthentication_required"))?;
        let bundle = CodexCredentialBundle::from_vault_json(&raw)?;
        if bundle.account_ref != account_ref {
            return Err(error("codex_account_binding_changed"));
        }
        Ok(bundle)
    }
    fn lock_for(
        &self,
        reference: &str,
        account_ref: Uuid,
    ) -> AdapterResult<Arc<tokio::sync::Mutex<RefreshState>>> {
        Ok(self
            .locks
            .lock()
            .map_err(|_| error("codex_refresh_lock_failed"))?
            .entry((reference.into(), account_ref))
            .or_default()
            .clone())
    }
    pub async fn access_snapshot(
        &self,
        reference: &str,
        account_ref: Uuid,
    ) -> AdapterResult<CodexAccessSnapshot> {
        self.access(reference, account_ref, None).await
    }
    pub async fn refresh_after_unauthorized(
        &self,
        reference: &str,
        account_ref: Uuid,
        rejected: &CodexAccessSnapshot,
    ) -> AdapterResult<CodexAccessSnapshot> {
        if rejected.account_ref != account_ref || rejected.coordinator_id != self.coordinator_id {
            return Err(error("codex_account_binding_changed"));
        }
        self.access(reference, account_ref, Some(rejected)).await
    }
    async fn access(
        &self,
        reference: &str,
        account_ref: Uuid,
        rejected: Option<&CodexAccessSnapshot>,
    ) -> AdapterResult<CodexAccessSnapshot> {
        let lock = self.lock_for(reference, account_ref)?;
        let mut state = lock.lock().await;
        let bundle = self.read(reference, account_ref)?;
        let fingerprint = bundle.fingerprint();
        if state.poisoned_fingerprint.as_ref() == Some(&fingerprint) {
            return Err(error("codex_reauthentication_required"));
        }
        state.poisoned_fingerprint = None;
        let rejected_current = rejected.is_some_and(|s| {
            s.token == bundle.access_token
                && s.account_id == bundle.account_id
                && s.generation == state.generation
        });
        if !rejected_current && self.clock.now_ms().saturating_add(300_000) < bundle.expires_at_ms {
            return Ok(bundle.snapshot(state.generation, self.coordinator_id));
        }
        // Mark before awaiting: cancellation of an uncertain rotating refresh also poisons this snapshot.
        state.poisoned_fingerprint = Some(fingerprint.clone());
        let outcome = self.transport.refresh(&bundle).await;
        let _mutation_guard = self.mutation_lock.lock().await;
        let current = self.read(reference, account_ref)?;
        if current.fingerprint() != fingerprint {
            if current.same_account_as(&bundle) && current.expires_at_ms > self.clock.now_ms() {
                state.poisoned_fingerprint = None;
                state.generation = state.generation.saturating_add(1);
                return Ok(current.snapshot(state.generation, self.coordinator_id));
            }
            return Err(error("codex_account_binding_changed"));
        }
        let refreshed = match outcome {
            Ok(updated)
                if updated.account_ref == account_ref
                    && updated.same_account_as(&bundle)
                    && updated.expires_at_ms > self.clock.now_ms() =>
            {
                updated
            }
            _ => {
                if let Ok(updated) = self.read(reference, account_ref) {
                    if updated.fingerprint() != fingerprint
                        && updated.same_account_as(&bundle)
                        && updated.expires_at_ms > self.clock.now_ms()
                    {
                        state.poisoned_fingerprint = None;
                        state.generation = state.generation.saturating_add(1);
                        return Ok(updated.snapshot(state.generation, self.coordinator_id));
                    }
                }
                return Err(error("codex_reauthentication_required"));
            }
        };
        self.vault
            .set(reference, &refreshed.to_vault_json()?)
            .map_err(|_| error("codex_vault_write_failed_reauthentication_required"))?;
        state.poisoned_fingerprint = None;
        state.generation = state.generation.saturating_add(1);
        Ok(refreshed.snapshot(state.generation, self.coordinator_id))
    }
}

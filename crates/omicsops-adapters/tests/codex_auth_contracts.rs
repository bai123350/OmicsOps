use async_trait::async_trait;
use omicsops_adapters::{
    AdapterError, AdapterResult,
    codex_auth::*,
    credentials::{CredentialVault, MemoryCredentialVault},
};
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

struct Clock;
impl CodexAuthClock for Clock {
    fn now_ms(&self) -> i64 {
        1000
    }
}

// Complete wire fixtures: requests are checked against the official device flow,
// rather than accepting any body as the earlier HTTP fake did.
struct DeviceWire {
    replies: std::sync::Mutex<VecDeque<(&'static str, serde_json::Value, u16, Vec<u8>)>>,
}
#[async_trait]
impl CodexAuthHttpTransport for DeviceWire {
    async fn post(
        &self,
        url: url::Url,
        body: CodexAuthHttpBody,
    ) -> AdapterResult<CodexAuthHttpResponse> {
        let (path, expected, status, bytes) = self.replies.lock().unwrap().pop_front().unwrap();
        assert_eq!(url.as_str(), format!("https://auth.openai.com{path}"));
        let actual = match body {
            CodexAuthHttpBody::Json(value) => value,
            CodexAuthHttpBody::Form(pairs) => serde_json::to_value(
                pairs
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>(),
            )
            .unwrap(),
        };
        assert_eq!(actual, expected);
        Ok(CodexAuthHttpResponse {
            status,
            body: bytes,
        })
    }
}
fn device_auth(replies: Vec<(&'static str, serde_json::Value, u16, Vec<u8>)>) -> CodexDeviceAuth {
    CodexDeviceAuth::new(Arc::new(Clock))
        .unwrap()
        .with_http_transport(Arc::new(DeviceWire {
            replies: std::sync::Mutex::new(replies.into()),
        }))
}
fn begin_request() -> serde_json::Value {
    json!({"client_id":"app_EMoamEEZ73f0CkXaXp7hrann"})
}
#[tokio::test]
async fn codex_device_login_accepts_the_official_usercode_alias() {
    let auth = device_auth(vec![(
        "/api/accounts/deviceauth/usercode",
        begin_request(),
        200,
        br#"{"device_auth_id":"dev","usercode":"ABCD-1234","interval":"5"}"#.to_vec(),
    )]);
    let challenge = auth
        .begin_device()
        .await
        .expect("official alias must yield a usable challenge");
    assert_eq!(challenge.user_code, "ABCD-1234");
    assert_eq!(challenge.interval, Duration::from_secs(5));
}
#[tokio::test]
async fn codex_device_login_preserves_only_safe_http_failure_categories() {
    for (status, body, expected) in [
        (
            403,
            r#"{"error":{"code":"unsupported_country_region_territory","message":"SECRET_TOKEN_SENTINEL"}}"#,
            "codex_device_region_unsupported",
        ),
        (
            403,
            r#"{"error":"unsupported_country_region_territory","access_token":"SECRET_TOKEN_SENTINEL"}"#,
            "codex_device_region_unsupported",
        ),
        (
            403,
            "<html><head>SECRET_TOKEN_SENTINEL</head></html>",
            "codex_device_web_verification_required",
        ),
        (
            200,
            "<!DOCTYPE html><html>SECRET_TOKEN_SENTINEL</html>",
            "codex_device_web_verification_required",
        ),
        (
            401,
            r#"{"error":{"code":"SECRET_TOKEN_SENTINEL"}}"#,
            "codex_device_access_denied",
        ),
        (
            429,
            r#"{"detail":"SECRET_TOKEN_SENTINEL"}"#,
            "codex_device_rate_limited",
        ),
        (
            404,
            r#"{"detail":"SECRET_TOKEN_SENTINEL"}"#,
            "codex_device_login_unavailable",
        ),
        (
            503,
            "SECRET_TOKEN_SENTINEL",
            "codex_device_login_unavailable",
        ),
        (200, "SECRET_TOKEN_SENTINEL", "codex_auth_response_invalid"),
    ] {
        let auth = device_auth(vec![(
            "/api/accounts/deviceauth/usercode",
            begin_request(),
            status,
            body.as_bytes().to_vec(),
        )]);
        let failure = auth.begin_device().await.err().expect("must fail safely");
        assert!(
            matches!(failure, AdapterError::Llm(ref code) if code == expected),
            "wrong safe category: {failure}"
        );
        assert!(!failure.to_string().contains("SECRET_TOKEN_SENTINEL"));
    }
}
#[tokio::test]
async fn codex_device_poll_rejects_real_denials_without_mistaking_them_for_pending() {
    for (status, body, expected) in [
        (
            403,
            r#"{"error":{"code":"unsupported_country_region_territory","message":"SECRET_TOKEN_SENTINEL"}}"#,
            "codex_device_region_unsupported",
        ),
        (
            403,
            "<html><head>SECRET_TOKEN_SENTINEL</head></html>",
            "codex_device_web_verification_required",
        ),
        (
            429,
            r#"{"error":"SECRET_TOKEN_SENTINEL"}"#,
            "codex_device_rate_limited",
        ),
        (
            403,
            r#"{"error":"access_denied","message":"SECRET_TOKEN_SENTINEL"}"#,
            "codex_device_access_denied",
        ),
    ] {
        let auth = device_auth(vec![
            (
                "/api/accounts/deviceauth/usercode",
                begin_request(),
                200,
                br#"{"device_auth_id":"dev","user_code":"ABCD-1234","interval":"5"}"#.to_vec(),
            ),
            (
                "/api/accounts/deviceauth/token",
                json!({"device_auth_id":"dev","user_code":"ABCD-1234"}),
                status,
                body.as_bytes().to_vec(),
            ),
        ]);
        let challenge = auth.begin_device().await.unwrap();
        let failure = auth
            .poll_device(&challenge, Arc::new(AtomicBool::new(false)))
            .await
            .err()
            .expect("must fail safely");
        assert!(
            matches!(failure, AdapterError::Llm(ref code) if code == expected),
            "wrong safe category: {failure}"
        );
        assert!(!failure.to_string().contains("SECRET_TOKEN_SENTINEL"));
    }
}
#[tokio::test]
async fn codex_device_wire_preserves_pending_and_exchanges_the_granted_code_once() {
    use base64::Engine;
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        r#"{"exp":7200,"https://api.openai.com/auth":{"chatgpt_account_id":"acct-fixture"}}"#,
    );
    let auth = device_auth(vec![
        ("/api/accounts/deviceauth/usercode", begin_request(), 200, br#"{"device_auth_id":"dev","user_code":"ABCD-1234","interval":"5"}"#.to_vec()),
        ("/api/accounts/deviceauth/token", json!({"device_auth_id":"dev","user_code":"ABCD-1234"}), 404, vec![]),
        ("/api/accounts/deviceauth/token", json!({"device_auth_id":"dev","user_code":"ABCD-1234"}), 403, br#"{"detail":"Authorization pending"}"#.to_vec()),
        ("/api/accounts/deviceauth/token", json!({"device_auth_id":"dev","user_code":"ABCD-1234"}), 200, br#"{"authorization_code":"fixture-code","code_verifier":"fixture-verifier"}"#.to_vec()),
        ("/oauth/token", json!({"grant_type":"authorization_code","client_id":"app_EMoamEEZ73f0CkXaXp7hrann","code":"fixture-code","code_verifier":"fixture-verifier","redirect_uri":"https://auth.openai.com/deviceauth/callback"}), 200, json!({"access_token":format!("e30.{claims}.sig"),"refresh_token":"fixture-refresh","expires_in":3600}).to_string().into_bytes()),
    ]);
    let challenge = auth.begin_device().await.unwrap();
    for _ in 0..2 {
        assert!(
            matches!(auth.poll_device(&challenge, Arc::new(AtomicBool::new(false))).await.unwrap(), CodexDevicePoll::Pending { next_poll_after } if next_poll_after == Duration::from_secs(5))
        );
    }
    let CodexDevicePoll::Authorized { bundle } = auth
        .poll_device(&challenge, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap()
    else {
        panic!("must authorize")
    };
    assert!(!bundle.account_ref().is_nil());
    let saved: serde_json::Value = serde_json::from_str(&bundle.to_vault_json().unwrap()).unwrap();
    assert_eq!(saved["account_id"], "acct-fixture");
    assert_eq!(saved["expires_at_ms"], 3_601_000);
}

struct WriteFailVault {
    inner: MemoryCredentialVault,
}
impl CredentialVault for WriteFailVault {
    fn set(&self, _: &str, _: &str) -> AdapterResult<()> {
        Err(AdapterError::Credential("SECRET_TOKEN_SENTINEL".into()))
    }
    fn get(&self, reference: &str) -> AdapterResult<Option<String>> {
        self.inner.get(reference)
    }
    fn delete(&self, reference: &str) -> AdapterResult<()> {
        self.inner.delete(reference)
    }
}
#[tokio::test]
async fn codex_vault_failure_requires_login_without_repeating_rotating_refresh() {
    let inner = MemoryCredentialVault::default();
    let id = Uuid::new_v4();
    inner
        .set(
            "model/test",
            &bundle(id, "old", 1000).to_vault_json().unwrap(),
        )
        .unwrap();
    let auth = Arc::new(Auth {
        count: AtomicUsize::new(0),
        fail: false,
        changed: false,
    });
    let c = CodexCredentialCoordinator::new(
        Arc::new(WriteFailVault { inner }),
        auth.clone(),
        Arc::new(Clock),
    );
    for _ in 0..2 {
        let e = c.access_snapshot("model/test", id).await.err().unwrap();
        assert!(!e.to_string().contains("SECRET_TOKEN_SENTINEL"));
    }
    assert_eq!(auth.count.load(Ordering::SeqCst), 1);
}
fn bundle(reference: Uuid, token: &str, expiry: i64) -> CodexCredentialBundle {
    CodexCredentialBundle::from_vault_json(&json!({"access_token":token,"refresh_token":"SECRET_TOKEN_SENTINEL","expires_at_ms":expiry,"account_id":"acct-fixture","account_ref":reference}).to_string()).unwrap()
}
struct Auth {
    count: AtomicUsize,
    fail: bool,
    changed: bool,
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
        self.count.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        if self.fail {
            return Err(AdapterError::Llm("SECRET_TOKEN_SENTINEL".into()));
        }
        if self.changed {
            return CodexCredentialBundle::from_vault_json(&json!({"access_token":"new","refresh_token":"new-refresh","expires_at_ms":1_000_000,"account_id":"other-account","account_ref":old.account_ref()}).to_string());
        }
        Ok(bundle(old.account_ref(), "new", 1_000_000))
    }
}
#[tokio::test]
async fn codex_refresh_is_serialized_and_keeps_account_binding() {
    let vault = Arc::new(MemoryCredentialVault::default());
    let id = Uuid::new_v4();
    vault
        .set(
            "model/test",
            &bundle(id, "old", 1000).to_vault_json().unwrap(),
        )
        .unwrap();
    let auth = Arc::new(Auth {
        count: AtomicUsize::new(0),
        fail: false,
        changed: false,
    });
    let c = CodexCredentialCoordinator::new(vault.clone(), auth.clone(), Arc::new(Clock));
    let (a, b) = tokio::join!(
        c.access_snapshot("model/test", id),
        c.access_snapshot("model/test", id)
    );
    assert!(a.is_ok() && b.is_ok());
    assert_eq!(auth.count.load(Ordering::SeqCst), 1);
    let snap = a.unwrap();
    c.refresh_after_unauthorized("model/test", id, &snap)
        .await
        .unwrap();
    assert_eq!(auth.count.load(Ordering::SeqCst), 2);
    assert!(
        c.access_snapshot("model/test", Uuid::new_v4())
            .await
            .is_err()
    );
    let changed = Arc::new(Auth {
        count: AtomicUsize::new(0),
        fail: false,
        changed: true,
    });
    let c = CodexCredentialCoordinator::new(vault.clone(), changed, Arc::new(Clock));
    assert!(
        c.refresh_after_unauthorized("model/test", id, &snap)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn codex_auth_unknown_exchange_is_not_replayed() {
    let vault = Arc::new(MemoryCredentialVault::default());
    let id = Uuid::new_v4();
    vault
        .set(
            "model/test",
            &bundle(id, "old", 1000).to_vault_json().unwrap(),
        )
        .unwrap();
    let auth = Arc::new(Auth {
        count: AtomicUsize::new(0),
        fail: true,
        changed: false,
    });
    let c = CodexCredentialCoordinator::new(vault, auth.clone(), Arc::new(Clock));
    for _ in 0..2 {
        let err = c.access_snapshot("model/test", id).await.err().unwrap();
        assert!(!err.to_string().contains("SECRET_TOKEN_SENTINEL"));
    }
    assert_eq!(auth.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn codex_concurrent_401_refreshes_once_even_if_access_token_is_unchanged() {
    let vault = Arc::new(MemoryCredentialVault::default());
    let id = Uuid::new_v4();
    vault
        .set(
            "model/test",
            &bundle(id, "new", 1_000_000).to_vault_json().unwrap(),
        )
        .unwrap();
    let auth = Arc::new(Auth {
        count: AtomicUsize::new(0),
        fail: false,
        changed: false,
    });
    let c = CodexCredentialCoordinator::new(vault, auth.clone(), Arc::new(Clock));
    let rejected = c.access_snapshot("model/test", id).await.unwrap();
    let (a, b) = tokio::join!(
        c.refresh_after_unauthorized("model/test", id, &rejected),
        c.refresh_after_unauthorized("model/test", id, &rejected)
    );
    assert!(a.is_ok() && b.is_ok());
    assert_eq!(auth.count.load(Ordering::SeqCst), 1);
}

struct Http {
    calls: AtomicUsize,
    cancel: Arc<AtomicBool>,
    mode: u8,
}

struct DisconnectDuringRefresh {
    vault: Arc<MemoryCredentialVault>,
}
#[async_trait]
impl CodexAuthTransport for DisconnectDuringRefresh {
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
        self.vault.delete("model/test")?;
        Ok(bundle(old.account_ref(), "new", 1_000_000))
    }
}
#[tokio::test]
async fn codex_refresh_cannot_restore_credentials_removed_during_network_wait() {
    let vault = Arc::new(MemoryCredentialVault::default());
    let id = Uuid::new_v4();
    vault
        .set(
            "model/test",
            &bundle(id, "old", 1000).to_vault_json().unwrap(),
        )
        .unwrap();
    let c = CodexCredentialCoordinator::new(
        vault.clone(),
        Arc::new(DisconnectDuringRefresh {
            vault: vault.clone(),
        }),
        Arc::new(Clock),
    );
    assert!(c.access_snapshot("model/test", id).await.is_err());
    assert!(vault.get("model/test").unwrap().is_none());
}
#[async_trait]
impl CodexAuthHttpTransport for Http {
    async fn post(
        &self,
        url: url::Url,
        _body: CodexAuthHttpBody,
    ) -> AdapterResult<CodexAuthHttpResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (status, body) = match url.path() {
            "/api/accounts/deviceauth/usercode" => (
                200,
                json!({"device_auth_id":"dev","user_code":"ABCD-1234","interval":"5"}).to_string(),
            ),
            "/api/accounts/deviceauth/token" => {
                if self.mode == 1 {
                    self.cancel.store(true, Ordering::SeqCst);
                }
                (if self.mode==2{400}else{200},if self.mode==2{json!({"error":"slow_down"})}else{json!({"authorization_code":"SECRET_CODE","code_verifier":"SECRET_VERIFIER"})}.to_string())
            }
            _ => panic!("token exchange must not happen after cancel"),
        };
        Ok(CodexAuthHttpResponse {
            status,
            body: body.into_bytes(),
        })
    }
}
#[tokio::test]
async fn codex_device_poll_obeys_interval_expiry_and_cancel() {
    let cancel = Arc::new(AtomicBool::new(false));
    let http = Arc::new(Http {
        calls: AtomicUsize::new(0),
        cancel: cancel.clone(),
        mode: 1,
    });
    let auth = CodexDeviceAuth::new(Arc::new(Clock))
        .unwrap()
        .with_http_transport(http.clone());
    let challenge = auth.begin_device().await.unwrap();
    assert_eq!(
        challenge.verification_uri.as_str(),
        "https://auth.openai.com/codex/device"
    );
    assert!(challenge.expires_at_ms - 1000 <= 15 * 60 * 1000);
    assert!(auth.poll_device(&challenge, cancel).await.is_err());
    assert_eq!(http.calls.load(Ordering::SeqCst), 2);
    let http = Arc::new(Http {
        calls: AtomicUsize::new(0),
        cancel: Arc::new(AtomicBool::new(false)),
        mode: 2,
    });
    let auth = CodexDeviceAuth::new(Arc::new(Clock))
        .unwrap()
        .with_http_transport(http);
    let mut challenge = auth.begin_device().await.unwrap();
    assert!(
        matches!(auth.poll_device(&challenge,Arc::new(AtomicBool::new(false))).await.unwrap(),CodexDevicePoll::Pending{next_poll_after} if next_poll_after==Duration::from_secs(10))
    );
    challenge.expires_at_ms = 1000;
    assert!(
        auth.poll_device(&challenge, Arc::new(AtomicBool::new(false)))
            .await
            .is_err()
    );
}

use async_trait::async_trait;
use omicsops_adapters::{
    AdapterError, AdapterResult,
    codex_auth::*,
    credentials::{CredentialVault, MemoryCredentialVault},
};
use serde_json::json;
use std::{
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

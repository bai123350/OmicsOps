use super::*;
use async_trait::async_trait;
use omicsops_adapters::{AdapterResult, credentials::MemoryCredentialVault};
use omicsops_dto::SaveModelProfileRequest;
use serde_json::json;
use std::sync::atomic::AtomicI64;
#[test]
fn subscription_resource_opening_accepts_only_fixed_official_pages() {
    for (resource, expected) in [
        ("codex_login", "https://auth.openai.com/codex/device"),
        ("claude_setup", "https://code.claude.com/docs/en/setup"),
        ("go_privacy", "https://opencode.ai/docs/go/#privacy"),
    ] {
        let parsed: omicsops_dto::SubscriptionResource =
            serde_json::from_value(json!(resource)).unwrap();
        let mut opened = vec![];
        open_subscription_resource_with(parsed, |url| {
            opened.push(url);
            Ok(())
        })
        .unwrap();
        assert_eq!(opened, vec![expected]);
        assert!(open_subscription_resource_with(parsed, |_| Err("open_failed".into())).is_err());
    }
    for invalid in [
        "https://malicious.test",
        "file:///secret",
        "cmd.exe",
        "codex_login?token=secret",
    ] {
        assert!(
            serde_json::from_value::<omicsops_dto::SubscriptionResource>(json!(invalid)).is_err()
        );
    }
}
struct Clock(AtomicI64);
impl CodexAuthClock for Clock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
struct Auth {
    clock: Arc<Clock>,
    expired: bool,
}
#[async_trait]
impl CodexAuthTransport for Auth {
    async fn begin_device(&self) -> AdapterResult<CodexDeviceChallenge> {
        Ok(CodexDeviceChallenge {
            device_auth_id: "dev".into(),
            user_code: "ABCD-1234".into(),
            verification_uri: url::Url::parse("https://auth.openai.com/codex/device").unwrap(),
            interval: std::time::Duration::from_secs(60),
            expires_at_ms: self.clock.now_ms() + if self.expired { 0 } else { 900_000 },
        })
    }
    async fn poll_device(
        &self,
        c: &CodexDeviceChallenge,
        _: Arc<AtomicBool>,
    ) -> AdapterResult<CodexDevicePoll> {
        Ok(CodexDevicePoll::Pending {
            next_poll_after: c.interval,
        })
    }
    async fn refresh(&self, _: &CodexCredentialBundle) -> AdapterResult<CodexCredentialBundle> {
        unreachable!()
    }
}
fn fixture(account: &str) -> CodexCredentialBundle {
    CodexCredentialBundle::from_vault_json(&json!({"access_token":"SECRET_ACCESS_SENTINEL","refresh_token":"SECRET_REFRESH_SENTINEL","expires_at_ms":1_000_000,"account_id":account,"account_ref":Uuid::new_v4()}).to_string()).unwrap()
}
fn request(id: Option<Uuid>) -> SaveModelProfileRequest {
    serde_json::from_value(json!({"id":id,"label":"Codex","provider":"open_ai_codex","base_url":"https://chatgpt.com/backend-api","model":"gpt-5.5","context_window_tokens":65_536})).unwrap()
}
fn manager() -> (
    Arc<SubscriptionLoginManager>,
    Arc<MemoryCredentialVault>,
    Arc<Clock>,
    CredentialMutationState,
) {
    let vault = Arc::new(MemoryCredentialVault::default());
    let clock = Arc::new(Clock(AtomicI64::new(1000)));
    let mutation = CredentialMutationState::default();
    (
        Arc::new(SubscriptionLoginManager::new(
            vault.clone(),
            Arc::new(Auth {
                clock: clock.clone(),
                expired: false,
            }),
            clock.clone(),
            mutation.lock.clone(),
        )),
        vault,
        clock,
        mutation,
    )
}

struct FailingAuth {
    clock: Arc<Clock>,
    code: String,
    fail_begin: bool,
    llm_error: bool,
}
impl FailingAuth {
    fn error(&self) -> omicsops_adapters::AdapterError {
        if self.llm_error {
            omicsops_adapters::AdapterError::Llm(self.code.clone())
        } else {
            omicsops_adapters::AdapterError::Credential(self.code.clone())
        }
    }
}
#[async_trait]
impl CodexAuthTransport for FailingAuth {
    async fn begin_device(&self) -> AdapterResult<CodexDeviceChallenge> {
        if self.fail_begin {
            return Err(self.error());
        }
        Ok(CodexDeviceChallenge {
            device_auth_id: "device-fixture".into(),
            user_code: "ABCD-1234".into(),
            verification_uri: url::Url::parse("https://auth.openai.com/codex/device").unwrap(),
            interval: Duration::from_millis(1),
            expires_at_ms: self.clock.now_ms() + 900_000,
        })
    }
    async fn poll_device(
        &self,
        _: &CodexDeviceChallenge,
        _: Arc<AtomicBool>,
    ) -> AdapterResult<CodexDevicePoll> {
        Err(self.error())
    }
    async fn refresh(&self, _: &CodexCredentialBundle) -> AdapterResult<CodexCredentialBundle> {
        unreachable!()
    }
}
fn failing_manager(code: &str, fail_begin: bool, llm_error: bool) -> Arc<SubscriptionLoginManager> {
    let clock = Arc::new(Clock(AtomicI64::new(1000)));
    Arc::new(SubscriptionLoginManager::new(
        Arc::new(MemoryCredentialVault::default()),
        Arc::new(FailingAuth {
            clock: clock.clone(),
            code: code.into(),
            fail_begin,
            llm_error,
        }),
        clock,
        Default::default(),
    ))
}
#[tokio::test]
async fn subscription_login_begin_preserves_only_exact_allowlisted_auth_errors() {
    for code in [
        "codex_auth_network_uncertain",
        "codex_auth_timeout",
        "codex_auth_connection_failed",
        "codex_device_region_unsupported",
        "codex_device_access_denied",
        "codex_device_web_verification_required",
        "codex_device_rate_limited",
        "codex_device_login_unavailable",
        "codex_auth_response_invalid",
        "codex_auth_field_invalid",
        "codex_device_interval_invalid",
        "codex_device_authorization_failed",
        "codex_token_exchange_failed",
        "codex_reauthentication_required",
        "codex_login_expired",
    ] {
        let m = failing_manager(code, true, true);
        assert_eq!(m.begin(None).await.unwrap_err(), code);
        let entries = m.entries.lock().await;
        let login = entries.values().next().unwrap();
        assert_eq!(login.state, CodexLoginState::Failed);
        assert_eq!(login.error_code.as_deref(), Some(code));
        assert!(login.bundle.is_none());
    }
    for (code, llm_error) in [
        ("SECRET_AUTH_BODY_SENTINEL", true),
        (
            "codex_device_region_unsupported SECRET_AUTH_BODY_SENTINEL",
            true,
        ),
        ("codex_device_region_unsupported", false),
    ] {
        let m = failing_manager(code, true, llm_error);
        assert_eq!(
            m.begin(None).await.unwrap_err(),
            "codex_device_login_failed"
        );
        let entries = m.entries.lock().await;
        assert_eq!(
            entries.values().next().unwrap().error_code.as_deref(),
            Some("codex_device_login_failed")
        );
    }
}
#[tokio::test]
async fn subscription_login_poll_exposes_safe_errors_without_transport_payloads() {
    for (code, llm_error, expected) in [
        ("codex_auth_timeout", true, "codex_auth_timeout"),
        (
            "codex_device_access_denied",
            true,
            "codex_device_access_denied",
        ),
        (
            "codex_token_exchange_failed",
            true,
            "codex_token_exchange_failed",
        ),
        ("codex_login_expired", true, "codex_login_expired"),
        (
            "SECRET_AUTH_BODY_SENTINEL",
            true,
            "codex_device_authorization_failed",
        ),
        (
            "codex_auth_timeout",
            false,
            "codex_device_authorization_failed",
        ),
    ] {
        let m = failing_manager(code, false, llm_error);
        let challenge = m.begin(None).await.unwrap();
        let state = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let state = m.poll(challenge.login_id).await.unwrap();
                if state.state != CodexLoginState::Pending {
                    break state;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("owned poller must finish the login");
        assert_eq!(state.state, CodexLoginState::Failed);
        assert_eq!(state.error_code.as_deref(), Some(expected));
        assert!(!serde_json::to_string(&state).unwrap().contains("SECRET_"));
    }
}
#[tokio::test]
async fn subscription_login_expiration_has_an_explicit_retry_code() {
    let (m, _, clock, _) = manager();
    let challenge = m.begin(None).await.unwrap();
    clock.0.store(901_000, Ordering::SeqCst);
    let state = m.poll(challenge.login_id).await.unwrap();
    assert_eq!(state.state, CodexLoginState::Expired);
    assert_eq!(state.error_code.as_deref(), Some("codex_login_expired"));
}
#[tokio::test]
async fn subscription_login_rejects_an_expired_challenge() {
    let clock = Arc::new(Clock(AtomicI64::new(1000)));
    let m = Arc::new(SubscriptionLoginManager::new(
        Arc::new(MemoryCredentialVault::default()),
        Arc::new(Auth {
            clock: clock.clone(),
            expired: true,
        }),
        clock,
        Default::default(),
    ));
    assert!(m.begin(None).await.is_err());
}
#[tokio::test]
async fn subscription_login_drop_stops_owned_pollers() {
    let (m, _, _, _) = manager();
    let begin = m.begin(None).await.unwrap();
    let handle = m.jobs.lock().unwrap().get(&begin.login_id).unwrap().clone();
    drop(m);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert!(handle.is_finished());
}
struct RestoreFailVault {
    inner: MemoryCredentialVault,
}
impl CredentialVault for RestoreFailVault {
    fn get(&self, r: &str) -> AdapterResult<Option<String>> {
        self.inner.get(r)
    }
    fn set(&self, r: &str, s: &str) -> AdapterResult<()> {
        self.inner.set(r, s)
    }
    fn delete(&self, _: &str) -> AdapterResult<()> {
        Err(omicsops_adapters::AdapterError::Credential(
            "SECRET_ACCESS_SENTINEL".into(),
        ))
    }
}
#[tokio::test]
async fn subscription_save_restore_failure_is_explicit_and_drops_temporary_bundle() {
    let vault = Arc::new(RestoreFailVault {
        inner: Default::default(),
    });
    let clock = Arc::new(Clock(AtomicI64::new(1000)));
    let c = CredentialMutationState::default();
    let m = Arc::new(SubscriptionLoginManager::new(
        vault,
        Arc::new(Auth {
            clock: clock.clone(),
            expired: false,
        }),
        clock,
        c.lock.clone(),
    ));
    let s = Store::open_in_memory().await.unwrap();
    sqlx::query("CREATE TRIGGER reject_subscription BEFORE INSERT ON model_profiles BEGIN SELECT RAISE(FAIL, 'test'); END").execute(s.pool()).await.unwrap();
    let id = login(&m, None, "one").await;
    let error = finish(&m, &s, &c, id, None).await.unwrap_err();
    assert!(error.contains("restore_failed"));
    assert!(!error.contains("SECRET_"));
    assert_eq!(m.poll(id).await.unwrap().state, CodexLoginState::Failed);
    assert!(m.entries.lock().await.get(&id).unwrap().bundle.is_none());
}
async fn login(m: &Arc<SubscriptionLoginManager>, id: Option<Uuid>, account: &str) -> Uuid {
    let b = m.begin(id).await.unwrap();
    m.authorize(b.login_id, fixture(account)).await;
    b.login_id
}
async fn finish(
    m: &SubscriptionLoginManager,
    s: &Store,
    c: &CredentialMutationState,
    login_id: Uuid,
    id: Option<Uuid>,
) -> Result<ModelProfile, String> {
    m.finish(
        s,
        c,
        &[],
        FinishCodexLoginRequest {
            login_id,
            profile: request(id),
        },
    )
    .await
}
#[tokio::test]
async fn subscription_login_cancel_wins_over_late_authorization() {
    let (m, v, clock, _) = manager();
    let b = m.begin(None).await.unwrap();
    m.cancel(b.login_id).await.unwrap();
    m.authorize(b.login_id, fixture("one")).await;
    assert_eq!(
        m.poll(b.login_id).await.unwrap().state,
        CodexLoginState::Cancelled
    );
    assert!(v.accounts().is_empty());
    for _ in 0..8 {
        m.begin(None).await.unwrap();
    }
    assert!(m.begin(None).await.is_err());
    clock.0.store(901_000, Ordering::SeqCst);
    assert!(m.begin(None).await.is_ok());
}
#[tokio::test]
async fn subscription_login_cannot_cross_profile_or_replace_active_account() {
    let (m, v, _, c) = manager();
    let s = Store::open_in_memory().await.unwrap();
    let l = login(&m, None, "one").await;
    assert!(finish(&m, &s, &c, l, Some(Uuid::new_v4())).await.is_err());
    let original = finish(&m, &s, &c, l, None).await.unwrap();
    assert!(finish(&m, &s, &c, l, None).await.is_err());
    let l = login(&m, Some(original.id), "one").await;
    let same = finish(&m, &s, &c, l, Some(original.id)).await.unwrap();
    assert_eq!(
        same.subscription_account_ref,
        original.subscription_account_ref
    );
    let l = login(&m, Some(original.id), "two").await;
    let changed = finish(&m, &s, &c, l, Some(original.id)).await.unwrap();
    assert_ne!(changed.id, original.id);
    assert_ne!(
        changed.subscription_account_ref,
        original.subscription_account_ref
    );
    assert_eq!(
        s.get_model_profile(original.id)
            .await
            .unwrap()
            .unwrap()
            .execution_configuration_hash(),
        original.execution_configuration_hash()
    );
    let stored = sqlx::query_scalar::<_, String>("SELECT value_json FROM model_profiles")
        .fetch_all(s.pool())
        .await
        .unwrap()
        .join("");
    assert!(!stored.contains("SECRET_"));
    assert_eq!(v.accounts().len(), 2);
}
#[tokio::test]
async fn subscription_save_compensates_keyring_database_failures() {
    let (m, v, _, c) = manager();
    let s = Store::open_in_memory().await.unwrap();
    sqlx::query("CREATE TRIGGER reject_subscription BEFORE INSERT ON model_profiles BEGIN SELECT RAISE(FAIL, 'test'); END").execute(s.pool()).await.unwrap();
    let l = login(&m, None, "one").await;
    assert!(finish(&m, &s, &c, l, None).await.is_err());
    assert!(v.accounts().is_empty());
    sqlx::query("DROP TRIGGER reject_subscription")
        .execute(s.pool())
        .await
        .unwrap();
    let saved = finish(&m, &s, &c, l, None).await.unwrap();
    let reference = saved.credential_reference.as_deref().unwrap();
    let before = v.get(reference).unwrap();
    sqlx::query("CREATE TRIGGER reject_subscription BEFORE INSERT ON model_profiles BEGIN SELECT RAISE(FAIL, 'test'); END").execute(s.pool()).await.unwrap();
    let l = login(&m, Some(saved.id), "one").await;
    assert!(finish(&m, &s, &c, l, Some(saved.id)).await.is_err());
    assert_eq!(v.get(reference).unwrap(), before);
}
#[tokio::test]
async fn subscription_login_disconnect_is_scoped_and_guarded() {
    let (m, v, _, c) = manager();
    let s = Store::open_in_memory().await.unwrap();
    let l = login(&m, None, "one").await;
    let saved = finish(&m, &s, &c, l, None).await.unwrap();
    v.set("model/other", "other fixture").unwrap();
    let now = chrono::Utc::now();
    let project = omicsops_core::workspace::Project::new(
        Uuid::new_v4(),
        "fixture",
        "C:\\omicsops-test",
        omicsops_core::workspace::ProjectTemplate::Blank,
        now,
    );
    s.save_project(&project).await.unwrap();
    let conversation =
        omicsops_core::workspace::Conversation::new(Uuid::new_v4(), project.id, "fixture", now);
    s.save_conversation(&conversation).await.unwrap();
    sqlx::query("INSERT INTO side_chat_turns_v4(request_id,conversation_id,project_id,request_hash,status,value_json) VALUES(?1,?2,?3,?4,'running',?5)").bind(Uuid::new_v4().to_string()).bind(conversation.id.to_string()).bind(project.id.to_string()).bind("a".repeat(64)).bind(json!({"model_profile_id":saved.id}).to_string()).execute(s.pool()).await.unwrap();
    assert!(m.disconnect(&s, &c, &[], saved.id).await.is_err());
    assert!(
        v.get(saved.credential_reference.as_deref().unwrap())
            .unwrap()
            .is_some()
    );
    sqlx::query("DELETE FROM side_chat_turns_v4")
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_disconnect BEFORE INSERT ON model_profiles BEGIN SELECT RAISE(FAIL, 'test'); END").execute(s.pool()).await.unwrap();
    assert!(m.disconnect(&s, &c, &[], saved.id).await.is_err());
    assert!(
        v.get(saved.credential_reference.as_deref().unwrap())
            .unwrap()
            .is_some()
    );
    sqlx::query("DROP TRIGGER reject_disconnect")
        .execute(s.pool())
        .await
        .unwrap();
    let result = m.disconnect(&s, &c, &[], saved.id).await.unwrap();
    assert!(result.subscription_account_ref.is_none());
    assert!(
        v.get(saved.credential_reference.as_deref().unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        v.get("model/other").unwrap().as_deref(),
        Some("other fixture")
    );
}

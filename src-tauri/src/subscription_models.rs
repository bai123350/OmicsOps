//! Device login owns temporary credentials; UI receives only states and public device codes.
use crate::{
    commands::AppState,
    credential_settings::CredentialMutationState,
    model_commands::{merge_existing_profile, model_profile_from_request},
};
use omicsops_adapters::{
    codex_auth::*,
    credentials::{CredentialVault, SystemCredentialVault, credential_account},
};
use omicsops_core::workspace::{ModelProfile, ModelProviderKind};
use omicsops_dto::{
    BeginCodexLoginResponse, CodexLoginState, CodexLoginStateResponse, FinishCodexLoginRequest,
    SubscriptionModelStatus,
};
use omicsops_store::Store;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::State;
use uuid::Uuid;

struct Login {
    profile_id: Option<Uuid>,
    state: CodexLoginState,
    expires_at_ms: i64,
    cancel: Arc<AtomicBool>,
    bundle: Option<CodexCredentialBundle>,
    error_code: Option<String>,
}
pub struct SubscriptionLoginManager {
    entries: tokio::sync::Mutex<HashMap<Uuid, Login>>,
    jobs: Mutex<HashMap<Uuid, tokio::task::AbortHandle>>,
    vault: Arc<dyn CredentialVault>,
    auth: Arc<dyn CodexAuthTransport>,
    clock: Arc<dyn CodexAuthClock>,
    pub(crate) coordinator: Arc<CodexCredentialCoordinator>,
}
impl SubscriptionLoginManager {
    pub(crate) fn model_services(
        &self,
    ) -> Arc<omicsops_adapters::model_client::ModelClientServices> {
        Arc::new(omicsops_adapters::model_client::ModelClientServices {
            vault: self.vault.clone(),
            codex: self.coordinator.clone(),
            claude: Arc::new(omicsops_adapters::claude_code::SystemClaudeProcessRunner),
        })
    }
    pub fn new(
        vault: Arc<dyn CredentialVault>,
        auth: Arc<dyn CodexAuthTransport>,
        clock: Arc<dyn CodexAuthClock>,
        mutation_lock: Arc<tokio::sync::Mutex<()>>,
    ) -> Self {
        let coordinator = Arc::new(
            CodexCredentialCoordinator::new(vault.clone(), auth.clone(), clock.clone())
                .with_mutation_lock(mutation_lock),
        );
        Self {
            entries: Default::default(),
            jobs: Default::default(),
            vault,
            auth,
            clock,
            coordinator,
        }
    }
    pub(crate) fn system(mutation_lock: Arc<tokio::sync::Mutex<()>>) -> Result<Self, String> {
        let clock = Arc::new(SystemCodexAuthClock);
        let auth =
            Arc::new(CodexDeviceAuth::new(clock.clone()).map_err(|_| "codex_auth_client_failed")?);
        Ok(Self::new(
            Arc::new(SystemCredentialVault),
            auth,
            clock,
            mutation_lock,
        ))
    }
    fn clean(entries: &mut HashMap<Uuid, Login>, now: i64) {
        for entry in entries.values_mut() {
            if now >= entry.expires_at_ms
                && matches!(
                    entry.state,
                    CodexLoginState::Pending | CodexLoginState::Authorized
                )
            {
                entry.state = CodexLoginState::Expired;
                entry.cancel.store(true, Ordering::SeqCst);
                entry.bundle = None;
            }
        }
        entries.retain(|_, entry| now < entry.expires_at_ms.saturating_add(900_000));
    }
    pub async fn begin(
        self: &Arc<Self>,
        profile_id: Option<Uuid>,
    ) -> Result<BeginCodexLoginResponse, String> {
        let login_id = Uuid::new_v4();
        let cancel = Arc::new(AtomicBool::new(false));
        let now = self.clock.now_ms();
        {
            let mut entries = self.entries.lock().await;
            Self::clean(&mut entries, now);
            if entries
                .values()
                .filter(|e| {
                    matches!(
                        e.state,
                        CodexLoginState::Pending | CodexLoginState::Authorized
                    )
                })
                .count()
                >= 8
                || entries.len() >= 64
            {
                return Err("codex_login_capacity_reached".into());
            }
            entries.insert(
                login_id,
                Login {
                    profile_id,
                    state: CodexLoginState::Pending,
                    expires_at_ms: now.saturating_add(900_000),
                    cancel: cancel.clone(),
                    bundle: None,
                    error_code: None,
                },
            );
        }
        let mut challenge = match self.auth.begin_device().await {
            Ok(c) => c,
            Err(_) => {
                self.fail(login_id, "codex_device_login_failed").await;
                return Err("codex_device_login_failed".into());
            }
        };
        if challenge.verification_uri.as_str() != "https://auth.openai.com/codex/device"
            || challenge.user_code.is_empty()
            || challenge.user_code.len() > 64
            || challenge.interval.is_zero()
            || challenge.interval > Duration::from_secs(900)
            || challenge.expires_at_ms <= self.clock.now_ms()
        {
            self.fail(login_id, "codex_device_challenge_invalid").await;
            return Err("codex_device_challenge_invalid".into());
        }
        challenge.expires_at_ms = challenge.expires_at_ms.min(now.saturating_add(900_000));
        let response = BeginCodexLoginResponse {
            login_id,
            verification_uri: challenge.verification_uri.to_string(),
            user_code: challenge.user_code.clone(),
            expires_at: date(challenge.expires_at_ms)?,
        };
        {
            let mut entries = self.entries.lock().await;
            let entry = entries.get_mut(&login_id).ok_or("codex_login_missing")?;
            entry.expires_at_ms = challenge.expires_at_ms;
        }
        let weak = Arc::downgrade(self);
        let auth = self.auth.clone();
        let clock = self.clock.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(challenge.interval).await;
                if cancel.load(Ordering::SeqCst) || clock.now_ms() >= challenge.expires_at_ms {
                    break;
                }
                match auth.poll_device(&challenge, cancel.clone()).await {
                    Ok(CodexDevicePoll::Pending { next_poll_after }) => {
                        challenge.interval = next_poll_after.max(Duration::from_secs(1));
                    }
                    Ok(CodexDevicePoll::Authorized { bundle }) => {
                        if let Some(manager) = weak.upgrade() {
                            manager.authorize(login_id, bundle).await;
                        }
                        break;
                    }
                    Err(_) => {
                        if let Some(manager) = weak.upgrade() {
                            manager
                                .fail(login_id, "codex_device_authorization_failed")
                                .await;
                        }
                        break;
                    }
                }
            }
            if let Some(manager) = weak.upgrade() {
                manager
                    .jobs
                    .lock()
                    .ok()
                    .map(|mut jobs| jobs.remove(&login_id));
            }
        });
        self.jobs
            .lock()
            .map_err(|_| "codex_login_lock_failed")?
            .insert(login_id, task.abort_handle());
        Ok(response)
    }
    async fn authorize(&self, id: Uuid, bundle: CodexCredentialBundle) {
        let mut entries = self.entries.lock().await;
        Self::clean(&mut entries, self.clock.now_ms());
        if let Some(entry) = entries.get_mut(&id) {
            if entry.state == CodexLoginState::Pending && !entry.cancel.load(Ordering::SeqCst) {
                entry.bundle = Some(bundle);
                entry.state = CodexLoginState::Authorized;
            }
        }
    }
    async fn fail(&self, id: Uuid, code: &str) {
        let mut entries = self.entries.lock().await;
        Self::clean(&mut entries, self.clock.now_ms());
        if let Some(entry) = entries.get_mut(&id) {
            if entry.state == CodexLoginState::Pending {
                entry.state = CodexLoginState::Failed;
                entry.error_code = Some(code.into());
                entry.bundle = None;
            }
        }
    }
    pub async fn poll(&self, id: Uuid) -> Result<CodexLoginStateResponse, String> {
        let mut entries = self.entries.lock().await;
        Self::clean(&mut entries, self.clock.now_ms());
        state_response(id, entries.get(&id).ok_or("codex_login_missing")?)
    }
    pub async fn cancel(&self, id: Uuid) -> Result<CodexLoginStateResponse, String> {
        let mut entries = self.entries.lock().await;
        Self::clean(&mut entries, self.clock.now_ms());
        let entry = entries.get_mut(&id).ok_or("codex_login_missing")?;
        if matches!(
            entry.state,
            CodexLoginState::Pending | CodexLoginState::Authorized
        ) {
            entry.state = CodexLoginState::Cancelled;
            entry.cancel.store(true, Ordering::SeqCst);
            entry.bundle = None;
        }
        if let Some(task) = self
            .jobs
            .lock()
            .map_err(|_| "codex_login_lock_failed")?
            .remove(&id)
        {
            task.abort();
        }
        state_response(id, entry)
    }
    pub async fn finish(
        &self,
        store: &Store,
        mutations: &CredentialMutationState,
        active_ids: &[Uuid],
        request: FinishCodexLoginRequest,
    ) -> Result<ModelProfile, String> {
        let _mutation = mutations.lock.lock().await;
        let mut entries = self.entries.lock().await;
        Self::clean(&mut entries, self.clock.now_ms());
        let entry = entries
            .get_mut(&request.login_id)
            .ok_or("codex_login_missing")?;
        if entry.state != CodexLoginState::Authorized {
            return Err("codex_login_not_authorized".into());
        }
        if request.profile.id != entry.profile_id || request.profile.provider != "open_ai_codex" {
            return Err("codex_login_profile_mismatch".into());
        }
        let input = request.profile;
        let old = if let Some(id) = entry.profile_id {
            Some(
                store
                    .get_model_profile(id)
                    .await
                    .map_err(|_| "subscription_store_failed")?
                    .ok_or("codex_login_profile_missing")?,
            )
        } else {
            None
        };
        if old
            .as_ref()
            .is_some_and(|p| p.provider != ModelProviderKind::OpenAiCodex)
        {
            return Err("codex_login_profile_mismatch".into());
        }
        let mut profile = model_profile_from_request(input.clone())?;
        merge_existing_profile(
            &mut profile,
            old.as_ref(),
            input.delegated_model_profile_id.is_none(),
            input.context_window_tokens.is_none(),
            input.reasoning_effort.is_none(),
            input.fast_mode.is_none(),
            input.refresh_catalog,
            input.cli_executable.is_none(),
        );
        let mut bundle = entry
            .bundle
            .as_ref()
            .ok_or("codex_login_not_authorized")?
            .clone();
        let previous =
            if let Some(reference) = old.as_ref().and_then(|p| p.credential_reference.as_deref()) {
                self.vault
                    .get(reference)
                    .map_err(|_| "subscription_vault_read_failed")?
                    .and_then(|raw| CodexCredentialBundle::from_vault_json(&raw).ok())
            } else {
                None
            };
        let same_account = previous
            .as_ref()
            .is_some_and(|b| b.same_account_as(&bundle));
        if same_account {
            bundle = bundle.with_account_ref(
                old.as_ref()
                    .and_then(|p| p.subscription_account_ref)
                    .unwrap_or_else(|| previous.as_ref().unwrap().account_ref()),
            );
        } else if old
            .as_ref()
            .is_some_and(|p| p.subscription_account_ref.is_some())
        {
            profile.id = Uuid::new_v4();
            profile.credential_reference = Some(credential_account("model", profile.id));
        }
        profile.subscription_account_ref = Some(bundle.account_ref());
        omicsops_core::workspace::validate_subscription_profile_fields(&profile)?;
        crate::model_commands::validate_profile_capabilities(&profile)?;
        if let Some(child) = profile.delegated_model_profile_id {
            if !store
                .get_model_profile(child)
                .await
                .map_err(|_| "subscription_store_failed")?
                .is_some_and(|p| p.supports_tools)
            {
                return Err("delegated model must support tools".into());
            }
        }
        let expected = old
            .as_ref()
            .filter(|p| p.id == profile.id)
            .map(ModelProfile::execution_configuration_hash);
        let saved = save_with_compensation(
            store,
            self.vault.as_ref(),
            &profile,
            expected.as_deref(),
            active_ids,
            Some(
                bundle
                    .to_vault_json()
                    .map_err(|_| "subscription_bundle_invalid")?,
            ),
        )
        .await;
        if let Err(error) = saved {
            if error.contains("restore_failed") {
                entry.state = CodexLoginState::Failed;
                entry.bundle = None;
                entry.cancel.store(true, Ordering::SeqCst);
                entry.error_code = Some(error.clone());
            }
            return Err(error);
        }
        entry.state = CodexLoginState::Saved;
        entry.bundle = None;
        entry.cancel.store(true, Ordering::SeqCst);
        Ok(profile)
    }
    pub async fn disconnect(
        &self,
        store: &Store,
        mutations: &CredentialMutationState,
        active_ids: &[Uuid],
        id: Uuid,
    ) -> Result<ModelProfile, String> {
        let _mutation = mutations.lock.lock().await;
        let mut profile = store
            .get_model_profile(id)
            .await
            .map_err(|_| "subscription_store_failed")?
            .ok_or("subscription_profile_missing")?;
        if profile.provider != ModelProviderKind::OpenAiCodex {
            return Err("subscription_provider_mismatch".into());
        }
        let hash = profile.execution_configuration_hash();
        profile.subscription_account_ref = None;
        save_with_compensation(
            store,
            self.vault.as_ref(),
            &profile,
            Some(&hash),
            active_ids,
            None,
        )
        .await?;
        Ok(profile)
    }
    pub async fn status(&self, profile: &ModelProfile) -> SubscriptionModelStatus {
        let provider = serde_json::to_value(profile.provider)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut status = SubscriptionModelStatus {
            provider,
            authenticated: false,
            masked_account_label: None,
            cli_version: None,
            error_code: None,
        };
        match profile.provider {
            ModelProviderKind::OpenAiCodex => {
                if let (Some(reference), Some(account_ref)) = (
                    &profile.credential_reference,
                    profile.subscription_account_ref,
                ) {
                    match self
                        .coordinator
                        .access_snapshot(reference, account_ref)
                        .await
                    {
                        Ok(snapshot) => {
                            status.authenticated = true;
                            status.masked_account_label = Some(snapshot.masked_account_label());
                        }
                        Err(_) => {
                            status.error_code = Some("codex_reauthentication_required".into())
                        }
                    }
                } else {
                    status.error_code = Some("codex_reauthentication_required".into());
                }
            }
            ModelProviderKind::ClaudeCode => {
                use omicsops_adapters::claude_code::{
                    ClaudeAuthKind, ClaudeProcessRunner, SystemClaudeProcessRunner,
                    resolve_claude_executable,
                };
                match resolve_claude_executable(profile.cli_executable.as_deref()) {
                    Ok(path) => match tokio::time::timeout(
                        Duration::from_secs(15),
                        SystemClaudeProcessRunner.inspect(&path),
                    )
                    .await
                    {
                        Ok(Ok(preflight)) => {
                            status.authenticated =
                                preflight.auth_kind == ClaudeAuthKind::Subscription;
                            status.cli_version = Some(preflight.version);
                            status.error_code = Some(
                                if !status.authenticated {
                                    "claude_subscription_login_required"
                                } else {
                                    "claude_policy_unverifiable"
                                }
                                .into(),
                            );
                        }
                        _ => status.error_code = Some("claude_preflight_failed".into()),
                    },
                    Err(_) => {
                        status.error_code = Some("claude_native_executable_unavailable".into())
                    }
                }
            }
            _ => {
                status.authenticated = profile
                    .credential_reference
                    .as_deref()
                    .and_then(|r| self.vault.get(r).ok().flatten())
                    .is_some_and(|s| !s.is_empty())
            }
        }
        status
    }
}
impl Drop for SubscriptionLoginManager {
    fn drop(&mut self) {
        if let Ok(jobs) = self.jobs.get_mut() {
            for task in jobs.values() {
                task.abort();
            }
        }
    }
}
fn date(ms: i64) -> Result<chrono::DateTime<chrono::Utc>, String> {
    chrono::DateTime::from_timestamp_millis(ms).ok_or_else(|| "codex_login_expiry_invalid".into())
}
fn state_response(id: Uuid, entry: &Login) -> Result<CodexLoginStateResponse, String> {
    Ok(CodexLoginStateResponse {
        login_id: id,
        state: entry.state,
        expires_at: date(entry.expires_at_ms)?,
        error_code: entry.error_code.clone(),
    })
}
async fn save_with_compensation(
    store: &Store,
    vault: &dyn CredentialVault,
    profile: &ModelProfile,
    expected: Option<&str>,
    active_ids: &[Uuid],
    secret: Option<String>,
) -> Result<(), String> {
    let reference = profile
        .credential_reference
        .as_deref()
        .ok_or("subscription_reference_missing")?;
    let previous = vault
        .get(reference)
        .map_err(|_| "subscription_vault_read_failed")?;
    let touched = AtomicBool::new(false);
    let result = store
        .save_model_profile_guarded_with(profile, expected, active_ids, || {
            touched.store(true, Ordering::SeqCst);
            match secret {
                Some(ref secret) => vault.set(reference, secret),
                None => vault.delete(reference),
            }
            .map_err(|_| "subscription_vault_write_failed".into())
        })
        .await;
    if let Err(error) = result {
        if touched.load(Ordering::SeqCst) {
            let restored = match previous {
                Some(ref previous) => vault.set(reference, previous),
                None => vault.delete(reference),
            };
            if restored.is_err() {
                return Err(
                    "subscription_save_failed_restore_failed_reauthentication_required".into(),
                );
            }
        }
        return Err(match error {
            omicsops_store::StoreError::InvalidInput(_) => "subscription_profile_in_use_or_changed",
            _ => "subscription_save_failed",
        }
        .into());
    }
    Ok(())
}
fn active_ids(state: &AppState) -> Result<Vec<Uuid>, String> {
    Ok(state
        .active_runs
        .lock()
        .map_err(|_| "subscription_active_runs_unavailable")?
        .keys()
        .copied()
        .collect())
}
#[tauri::command]
pub async fn subscription_begin_codex_login(
    state: State<'_, AppState>,
    profile_id: Option<Uuid>,
) -> Result<BeginCodexLoginResponse, String> {
    if let Some(id) = profile_id {
        let profile = state
            .repository
            .get_model_profile(id)
            .await
            .map_err(|_| "subscription_store_failed")?
            .ok_or("subscription_profile_missing")?;
        if profile.provider != ModelProviderKind::OpenAiCodex {
            return Err("subscription_provider_mismatch".into());
        }
    }
    state.subscription_models.begin(profile_id).await
}
#[tauri::command]
pub async fn subscription_poll_codex_login(
    state: State<'_, AppState>,
    login_id: Uuid,
) -> Result<CodexLoginStateResponse, String> {
    state.subscription_models.poll(login_id).await
}
#[tauri::command]
pub async fn subscription_cancel_codex_login(
    state: State<'_, AppState>,
    login_id: Uuid,
) -> Result<CodexLoginStateResponse, String> {
    state.subscription_models.cancel(login_id).await
}
#[tauri::command]
pub async fn subscription_finish_codex_login(
    state: State<'_, AppState>,
    mutations: State<'_, CredentialMutationState>,
    request: FinishCodexLoginRequest,
) -> Result<ModelProfile, String> {
    state
        .subscription_models
        .finish(&state.repository, &mutations, &active_ids(&state)?, request)
        .await
}
#[tauri::command]
pub async fn subscription_model_status(
    state: State<'_, AppState>,
    profile_id: Uuid,
) -> Result<SubscriptionModelStatus, String> {
    let profile = state
        .repository
        .get_model_profile(profile_id)
        .await
        .map_err(|_| "subscription_store_failed")?
        .ok_or("subscription_profile_missing")?;
    Ok(state.subscription_models.status(&profile).await)
}
#[tauri::command]
pub async fn subscription_disconnect_codex(
    state: State<'_, AppState>,
    mutations: State<'_, CredentialMutationState>,
    profile_id: Uuid,
) -> Result<ModelProfile, String> {
    state
        .subscription_models
        .disconnect(
            &state.repository,
            &mutations,
            &active_ids(&state)?,
            profile_id,
        )
        .await
}
#[cfg(test)]
mod tests;

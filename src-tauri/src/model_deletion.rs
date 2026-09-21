use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use omicsops_adapters::credentials::CredentialVault;
use omicsops_store::Store;
use tauri::State;
use uuid::Uuid;

use crate::{commands::AppState, credential_settings::CredentialMutationState};

#[tauri::command]
pub async fn delete_model_profile(
    state: State<'_, AppState>,
    credential_mutations: State<'_, CredentialMutationState>,
    profile_id: Uuid,
) -> Result<bool, String> {
    delete_model_profile_from(
        &state.repository,
        &state.credentials,
        &state.active_runs,
        &credential_mutations,
        profile_id,
    )
    .await
}

pub(crate) async fn delete_model_profile_from(
    repository: &Store,
    credentials: &dyn CredentialVault,
    active_runs: &Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
    credential_mutations: &CredentialMutationState,
    profile_id: Uuid,
) -> Result<bool, String> {
    let _credential_guard = credential_mutations.lock.lock().await;
    let active_run_ids = active_runs
        .lock()
        .map_err(|_| "Active model runs could not be checked.".to_owned())?
        .keys()
        .copied()
        .collect::<Vec<_>>();
    repository
        .delete_model_profile_with(profile_id, &active_run_ids, |reference| {
            credentials
                .delete(reference)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use omicsops_adapters::credentials::{CredentialVault, MemoryCredentialVault};
    use omicsops_core::workspace::{ModelProfile, ModelProviderKind};
    use omicsops_store::Store;
    use uuid::Uuid;

    use crate::credential_settings::CredentialMutationState;

    fn profile(id: Uuid) -> ModelProfile {
        ModelProfile {
            id,
            label: "delete me".into(),
            provider: ModelProviderKind::OpenAiCompatible,
            base_url: "https://example.invalid/v1".into(),
            model: "test-model".into(),
            credential_reference: Some(format!("model/{id}")),
            supports_tools: true,
            supports_vision: false,
            context_window_tokens: None,
            catalog_capabilities: None,
            reasoning_effort: None,
            fast_mode: None,
            delegated_model_profile_id: None,
        }
    }

    #[tokio::test]
    async fn command_helper_removes_the_profile_and_its_vault_entry() {
        let store = Store::open_in_memory().await.unwrap();
        let vault = MemoryCredentialVault::default();
        let id = Uuid::new_v4();
        let profile = profile(id);
        store.save_model_profile(&profile).await.unwrap();
        vault
            .set(profile.credential_reference.as_deref().unwrap(), "secret")
            .unwrap();

        let deleted = super::delete_model_profile_from(
            &store,
            &vault,
            &Arc::new(Mutex::new(HashMap::new())),
            &CredentialMutationState::default(),
            id,
        )
        .await
        .unwrap();

        assert!(deleted);
        assert!(store.get_model_profile(id).await.unwrap().is_none());
        assert!(vault.accounts().is_empty());
    }
}

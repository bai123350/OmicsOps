use std::collections::BTreeMap;

use chrono::Utc;
use omicsops_adapters::credentials::{CredentialVault, credential_account};
use omicsops_core::domain::AuthenticationMethod;
use omicsops_dto::{
    CreateCredentialResult, CreateManagedCredentialRequest, CredentialConsumer, CredentialEntry,
    CredentialPresence, CredentialTarget, CredentialValueKind, DeleteCredentialResult,
    ManagedCredentialMetadata, ReplaceCredentialRequest,
};
use omicsops_mcp::McpEnvBinding;
use omicsops_store::Store;
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::{AppState, parse_authentication_secret},
    p1_commands::{MCP_SERVER_KIND, McpServerProfile},
};

pub const MANAGED_CREDENTIAL_KIND: &str = "credential_entry_v1";
const MAX_MANAGED_CREDENTIALS: usize = 100;
const MAX_SECRET_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct CredentialMutationState {
    pub(crate) lock: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
struct Owner {
    target: CredentialTarget,
    label: String,
    value_kind: CredentialValueKind,
    priority: u8,
}

#[derive(Default)]
struct InventoryItem {
    owners: Vec<Owner>,
    consumers: Vec<CredentialConsumer>,
}

pub async fn list_credentials(
    repository: &Store,
    vault: &dyn CredentialVault,
) -> Result<Vec<CredentialEntry>, String> {
    let mut items: BTreeMap<String, InventoryItem> = BTreeMap::new();

    for profile in repository
        .list_model_profiles()
        .await
        .map_err(storage_unavailable)?
    {
        let Some(reference) = nonempty_reference(profile.credential_reference.as_deref()) else {
            continue;
        };
        let item = items.entry(reference.to_owned()).or_default();
        item.owners.push(Owner {
            target: CredentialTarget::Model { id: profile.id },
            label: profile.label.clone(),
            value_kind: CredentialValueKind::ApiKey,
            priority: 0,
        });
        item.consumers.push(CredentialConsumer {
            kind: "model".into(),
            id: profile.id,
            label: profile.label,
            binding_name: None,
        });
    }

    for profile in repository
        .list_connections()
        .await
        .map_err(storage_unavailable)?
    {
        let Some(reference) = nonempty_reference(Some(&profile.authentication_reference)) else {
            continue;
        };
        let value_kind = match profile.authentication {
            AuthenticationMethod::Password => CredentialValueKind::Password,
            AuthenticationMethod::PrivateKey => CredentialValueKind::SshPrivateKey,
        };
        let item = items.entry(reference.to_owned()).or_default();
        item.owners.push(Owner {
            target: CredentialTarget::Ssh { id: profile.id },
            label: profile.label.clone(),
            value_kind,
            priority: 0,
        });
        item.consumers.push(CredentialConsumer {
            kind: "ssh".into(),
            id: profile.id,
            label: profile.label,
            binding_name: None,
        });
    }

    for metadata in repository
        .list_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND)
        .await
        .map_err(storage_unavailable)?
    {
        let reference = credential_account("settings", metadata.id);
        items.entry(reference).or_default().owners.push(Owner {
            target: CredentialTarget::Managed { id: metadata.id },
            label: metadata.label,
            value_kind: CredentialValueKind::ApiKey,
            priority: 1,
        });
    }

    for profile in repository
        .list_json::<McpServerProfile>(MCP_SERVER_KIND)
        .await
        .map_err(storage_unavailable)?
    {
        for binding in profile.env_bindings {
            let Some(reference) = nonempty_reference(binding.credential_reference.as_deref())
            else {
                continue;
            };
            let item = items.entry(reference.to_owned()).or_default();
            item.owners.push(Owner {
                target: CredentialTarget::McpBinding {
                    server_id: profile.id,
                    name: binding.name.clone(),
                },
                label: format!("{} · {}", profile.name, binding.name),
                value_kind: CredentialValueKind::ApiKey,
                priority: 2,
            });
            item.consumers.push(CredentialConsumer {
                kind: "mcp".into(),
                id: profile.id,
                label: profile.name.clone(),
                binding_name: Some(binding.name),
            });
        }
    }

    let mut result = Vec::with_capacity(items.len());
    for (reference, mut item) in items {
        item.owners.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| target_sort_key(&left.target).cmp(&target_sort_key(&right.target)))
        });
        item.consumers.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.binding_name.cmp(&right.binding_name))
        });
        let owner = item.owners.first().expect("inventory item has an owner");
        let incompatible = item
            .owners
            .iter()
            .filter(|candidate| candidate.priority < 2)
            .any(|candidate| candidate.value_kind != owner.value_kind);
        let orphaned_owner_reference = owner.priority == 2
            && (reference.starts_with("ssh/")
                || reference.starts_with("model/")
                || reference.starts_with("settings/"));
        let can_replace = !incompatible && !orphaned_owner_reference;
        let presence = match vault.get(&reference) {
            Ok(Some(_)) => CredentialPresence::Present,
            Ok(None) => CredentialPresence::Missing,
            Err(_) => CredentialPresence::Unavailable,
        };
        let managed = matches!(owner.target, CredentialTarget::Managed { .. });
        let can_delete = managed && can_replace && item.consumers.is_empty();
        result.push(CredentialEntry {
            target: owner.target.clone(),
            reference,
            label: owner.label.clone(),
            presence,
            value_kind: owner.value_kind,
            consumers: item.consumers,
            can_replace,
            can_delete,
        });
    }
    Ok(result)
}

pub async fn create_credential(
    repository: &Store,
    vault: &dyn CredentialVault,
    request: CreateManagedCredentialRequest,
) -> Result<CreateCredentialResult, String> {
    let label = request.label.trim();
    if label.is_empty() || label.chars().count() > 100 {
        return Err("Credential label must contain 1 to 100 characters".into());
    }
    if crate::composer_references::public_text(label) != label {
        return Err("Credential label contains prohibited sensitive content".into());
    }
    validate_secret(&request.secret)?;
    if repository
        .list_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND)
        .await
        .map_err(storage_unavailable)?
        .len()
        >= MAX_MANAGED_CREDENTIALS
    {
        return Err("At most 100 managed credentials can be stored".into());
    }
    let metadata = ManagedCredentialMetadata {
        id: Uuid::new_v4(),
        label: label.to_owned(),
        created_at: Utc::now(),
    };
    repository
        .put_json(MANAGED_CREDENTIAL_KIND, &metadata.id.to_string(), &metadata)
        .await
        .map_err(storage_unavailable)?;
    let reference = credential_account("settings", metadata.id);
    let saved = vault.set(&reference, &request.secret).is_ok();
    let entry = CredentialEntry {
        target: CredentialTarget::Managed { id: metadata.id },
        reference: reference.clone(),
        label: metadata.label,
        presence: credential_presence(vault, &reference),
        value_kind: CredentialValueKind::ApiKey,
        consumers: vec![],
        can_replace: true,
        can_delete: true,
    };
    Ok(if saved {
        CreateCredentialResult::Saved { entry }
    } else {
        CreateCredentialResult::SecretSaveNotConfirmed { entry }
    })
}

pub async fn replace_credential(
    repository: &Store,
    vault: &dyn CredentialVault,
    request: ReplaceCredentialRequest,
) -> Result<CredentialEntry, String> {
    validate_secret(&request.secret)?;
    if request.expected_reference.trim().is_empty() {
        return Err("Expected credential reference is required".into());
    }
    let resolved = resolve_target_reference(repository, &request.target).await?;
    if request.expected_reference != resolved {
        return Err("Credential binding changed; refresh before retrying".into());
    }
    let mut current = credential_by_reference(repository, vault, &resolved).await?;
    if request.expected_value_kind != current.value_kind {
        return Err("Credential value type changed; refresh before retrying".into());
    }
    if !current.can_replace {
        return Err("Credential reference has conflicting or missing ownership".into());
    }
    match current.value_kind {
        CredentialValueKind::SshPrivateKey => {
            parse_authentication_secret(AuthenticationMethod::PrivateKey, &request.secret)
                .map_err(|_| "Private-key credential is invalid".to_string())?;
        }
        CredentialValueKind::Password => {
            parse_authentication_secret(AuthenticationMethod::Password, &request.secret)
                .map_err(|_| "Password credential is invalid".to_string())?;
        }
        CredentialValueKind::ApiKey => {}
    }
    vault
        .set(&resolved, &request.secret)
        .map_err(|_| "Credential secret could not be saved".to_string())?;
    current.presence = CredentialPresence::Present;
    Ok(current)
}

pub async fn delete_credential(
    repository: &Store,
    vault: &dyn CredentialVault,
    id: Uuid,
) -> Result<DeleteCredentialResult, String> {
    let metadata = repository
        .get_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND, &id.to_string())
        .await
        .map_err(storage_unavailable)?
        .ok_or_else(|| "Managed credential was not found".to_string())?;
    if metadata.id != id {
        return Err("Managed credential metadata identity is invalid".into());
    }
    let reference = credential_account("settings", metadata.id);
    let entry = credential_by_reference(repository, vault, &reference).await?;
    if !entry.consumers.is_empty() {
        return Ok(DeleteCredentialResult::InUse {
            consumers: entry.consumers,
        });
    }
    vault
        .delete(&reference)
        .map_err(|_| "Credential secret could not be deleted".to_string())?;
    repository
        .delete_json(MANAGED_CREDENTIAL_KIND, &id.to_string())
        .await
        .map_err(storage_unavailable)?;
    Ok(DeleteCredentialResult::Deleted)
}

pub async fn validate_managed_credential_references(
    repository: &Store,
    bindings: &[McpEnvBinding],
) -> Result<(), String> {
    for reference in bindings
        .iter()
        .filter_map(|binding| binding.credential_reference.as_deref())
        .filter(|reference| reference.starts_with("settings/"))
    {
        let id = reference
            .strip_prefix("settings/")
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| "Managed credential reference is invalid".to_string())?;
        if reference != credential_account("settings", id) {
            return Err("Managed credential reference is invalid".into());
        }
        if repository
            .get_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND, &id.to_string())
            .await
            .map_err(storage_unavailable)?
            .is_none_or(|metadata| metadata.id != id)
        {
            return Err("Managed credential reference was not found".into());
        }
    }
    Ok(())
}

pub(crate) async fn persist_mcp_profile_with_credential_guard(
    repository: &Store,
    mutations: &CredentialMutationState,
    profile: &McpServerProfile,
) -> Result<(), String> {
    let _guard = mutations.lock.lock().await;
    validate_managed_credential_references(repository, &profile.env_bindings).await?;
    repository
        .put_json(MCP_SERVER_KIND, &profile.id.to_string(), profile)
        .await
        .map_err(|_| "MCP configuration storage is unavailable".to_string())
}

async fn delete_credential_with_guard(
    repository: &Store,
    vault: &dyn CredentialVault,
    mutations: &CredentialMutationState,
    id: Uuid,
) -> Result<DeleteCredentialResult, String> {
    let _guard = mutations.lock.lock().await;
    delete_credential(repository, vault, id).await
}

async fn credential_by_reference(
    repository: &Store,
    vault: &dyn CredentialVault,
    reference: &str,
) -> Result<CredentialEntry, String> {
    list_credentials(repository, vault)
        .await?
        .into_iter()
        .find(|entry| entry.reference == reference)
        .ok_or_else(|| "Credential target is no longer available".to_string())
}

async fn resolve_target_reference(
    repository: &Store,
    target: &CredentialTarget,
) -> Result<String, String> {
    match target {
        CredentialTarget::Model { id } => repository
            .get_model_profile(*id)
            .await
            .map_err(storage_unavailable)?
            .and_then(|profile| profile.credential_reference)
            .and_then(|reference| nonempty_reference(Some(&reference)).map(str::to_owned))
            .ok_or_else(|| "Model credential target was not found".into()),
        CredentialTarget::Ssh { id } => repository
            .list_connections()
            .await
            .map_err(storage_unavailable)?
            .into_iter()
            .find(|profile| profile.id == *id)
            .map(|profile| profile.authentication_reference)
            .and_then(|reference| nonempty_reference(Some(&reference)).map(str::to_owned))
            .ok_or_else(|| "SSH credential target was not found".into()),
        CredentialTarget::McpBinding { server_id, name } => repository
            .get_json::<McpServerProfile>(MCP_SERVER_KIND, &server_id.to_string())
            .await
            .map_err(storage_unavailable)?
            .and_then(|profile| {
                profile
                    .env_bindings
                    .into_iter()
                    .find(|binding| binding.name == *name)
            })
            .and_then(|binding| binding.credential_reference)
            .and_then(|reference| nonempty_reference(Some(&reference)).map(str::to_owned))
            .ok_or_else(|| "MCP credential target was not found".into()),
        CredentialTarget::Managed { id } => repository
            .get_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND, &id.to_string())
            .await
            .map_err(storage_unavailable)?
            .filter(|metadata| metadata.id == *id)
            .map(|metadata| credential_account("settings", metadata.id))
            .ok_or_else(|| "Managed credential target was not found".into()),
    }
}

fn validate_secret(secret: &str) -> Result<(), String> {
    if secret.trim().is_empty() {
        return Err("Credential secret is required".into());
    }
    if secret.len() > MAX_SECRET_BYTES {
        return Err("Credential secret exceeds 64 KiB".into());
    }
    Ok(())
}

fn credential_presence(vault: &dyn CredentialVault, reference: &str) -> CredentialPresence {
    match vault.get(reference) {
        Ok(Some(_)) => CredentialPresence::Present,
        Ok(None) => CredentialPresence::Missing,
        Err(_) => CredentialPresence::Unavailable,
    }
}

fn storage_unavailable(_: impl std::fmt::Display) -> String {
    "Credential metadata storage is unavailable".into()
}

fn nonempty_reference(reference: Option<&str>) -> Option<&str> {
    reference.map(str::trim).filter(|value| !value.is_empty())
}

fn target_sort_key(target: &CredentialTarget) -> String {
    match target {
        CredentialTarget::Model { id } => format!("0/{id}"),
        CredentialTarget::Ssh { id } => format!("1/{id}"),
        CredentialTarget::Managed { id } => format!("2/{id}"),
        CredentialTarget::McpBinding { server_id, name } => format!("3/{server_id}/{name}"),
    }
}

#[tauri::command]
pub async fn settings_list_credentials(
    state: State<'_, AppState>,
) -> Result<Vec<CredentialEntry>, String> {
    list_credentials(&state.repository, &state.credentials).await
}

#[tauri::command]
pub async fn settings_create_credential(
    state: State<'_, AppState>,
    mutations: State<'_, CredentialMutationState>,
    request: CreateManagedCredentialRequest,
) -> Result<CreateCredentialResult, String> {
    let _guard = mutations.lock.lock().await;
    create_credential(&state.repository, &state.credentials, request).await
}

#[tauri::command]
pub async fn settings_replace_credential(
    state: State<'_, AppState>,
    mutations: State<'_, CredentialMutationState>,
    request: ReplaceCredentialRequest,
) -> Result<CredentialEntry, String> {
    let _guard = mutations.lock.lock().await;
    replace_credential(&state.repository, &state.credentials, request).await
}

#[tauri::command]
pub async fn settings_delete_credential(
    state: State<'_, AppState>,
    mutations: State<'_, CredentialMutationState>,
    id: Uuid,
) -> Result<DeleteCredentialResult, String> {
    delete_credential_with_guard(&state.repository, &state.credentials, &mutations, id).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use chrono::Utc;
    use omicsops_adapters::{AdapterError, AdapterResult};
    use omicsops_core::domain::{AuthenticationMethod, ConnectionProfile};
    use omicsops_mcp::McpEnvBinding;

    use super::*;

    #[derive(Clone, Default)]
    struct TestVault {
        values: Arc<Mutex<BTreeMap<String, String>>>,
        fail_get: Arc<Mutex<bool>>,
        fail_set: Arc<Mutex<bool>>,
        fail_delete: Arc<Mutex<bool>>,
        set_calls: Arc<Mutex<Vec<String>>>,
        delete_calls: Arc<Mutex<Vec<String>>>,
    }

    impl CredentialVault for TestVault {
        fn set(&self, account: &str, secret: &str) -> AdapterResult<()> {
            self.set_calls.lock().unwrap().push(account.into());
            if *self.fail_set.lock().unwrap() {
                return Err(AdapterError::Credential(format!("vault rejected {secret}")));
            }
            self.values
                .lock()
                .unwrap()
                .insert(account.into(), secret.into());
            Ok(())
        }

        fn get(&self, account: &str) -> AdapterResult<Option<String>> {
            if *self.fail_get.lock().unwrap() {
                return Err(AdapterError::Credential("raw vault failure".into()));
            }
            Ok(self.values.lock().unwrap().get(account).cloned())
        }

        fn delete(&self, account: &str) -> AdapterResult<()> {
            self.delete_calls.lock().unwrap().push(account.into());
            if *self.fail_delete.lock().unwrap() {
                return Err(AdapterError::Credential("raw delete failure".into()));
            }
            self.values.lock().unwrap().remove(account);
            Ok(())
        }
    }

    fn binding(name: &str, reference: &str) -> McpEnvBinding {
        McpEnvBinding {
            name: name.into(),
            value: None,
            credential_reference: Some(reference.into()),
        }
    }

    fn mcp(id: Uuid, name: &str, env_bindings: Vec<McpEnvBinding>) -> McpServerProfile {
        McpServerProfile {
            id,
            name: name.into(),
            command: "server".into(),
            args: vec![],
            cwd: None,
            timeout_secs: 60,
            env_bindings,
            enabled: false,
            launch_approved: false,
            approved_tools: vec![],
            tools: vec![],
            capabilities: serde_json::json!({}),
            tool_catalog_sha256: None,
            catalog_generation: 0,
            config_version: 1,
            status: "disconnected".into(),
            last_error: None,
            stderr_tail: None,
            last_inspected_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn directory_deduplicates_shared_mcp_reference_and_hides_vault_failures() {
        let store = Store::open_in_memory().await.unwrap();
        let account = "external/shared";
        store
            .put_json(
                MCP_SERVER_KIND,
                &Uuid::from_u128(1).to_string(),
                &mcp(
                    Uuid::from_u128(1),
                    "Papers",
                    vec![binding("TOKEN", account)],
                ),
            )
            .await
            .unwrap();
        store
            .put_json(
                MCP_SERVER_KIND,
                &Uuid::from_u128(2).to_string(),
                &mcp(Uuid::from_u128(2), "Data", vec![binding("KEY", account)]),
            )
            .await
            .unwrap();
        let vault = TestVault::default();

        let missing = list_credentials(&store, &vault).await.unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].presence, CredentialPresence::Missing);
        assert_eq!(missing[0].consumers.len(), 2);
        *vault.fail_get.lock().unwrap() = true;
        let unavailable = list_credentials(&store, &vault).await.unwrap();
        assert_eq!(unavailable[0].presence, CredentialPresence::Unavailable);
        assert!(
            !serde_json::to_string(&unavailable)
                .unwrap()
                .contains("raw vault failure")
        );
    }

    #[tokio::test]
    async fn mcp_alias_to_ssh_uses_private_key_validation_and_expected_reference() {
        let store = Store::open_in_memory().await.unwrap();
        let ssh_id = Uuid::from_u128(10);
        let account = credential_account("ssh", ssh_id);
        store
            .save_connection(&ConnectionProfile {
                id: ssh_id,
                label: "Cluster".into(),
                host: "cluster.example".into(),
                port: 22,
                username: "user".into(),
                authentication: AuthenticationMethod::PrivateKey,
                authentication_reference: account.clone(),
                host_key_fingerprint: None,
            })
            .await
            .unwrap();
        let server_id = Uuid::from_u128(11);
        store
            .put_json(
                MCP_SERVER_KIND,
                &server_id.to_string(),
                &mcp(server_id, "Alias", vec![binding("SSH", &account)]),
            )
            .await
            .unwrap();
        let vault = TestVault::default();
        let target = CredentialTarget::McpBinding {
            server_id,
            name: "SSH".into(),
        };
        let bad = replace_credential(
            &store,
            &vault,
            ReplaceCredentialRequest {
                target: target.clone(),
                expected_reference: account.clone(),
                expected_value_kind: CredentialValueKind::SshPrivateKey,
                secret: "generic-api-key".into(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(bad, "Private-key credential is invalid");
        assert!(vault.set_calls.lock().unwrap().is_empty());

        replace_credential(
            &store,
            &vault,
            ReplaceCredentialRequest {
                target,
                expected_reference: account.clone(),
                expected_value_kind: CredentialValueKind::SshPrivateKey,
                secret: r#"{"path":"C:\\keys\\cluster","passphrase":null}"#.into(),
            },
        )
        .await
        .unwrap();
        assert!(vault.values.lock().unwrap().contains_key(&account));
    }

    #[tokio::test]
    async fn changed_binding_rejects_replace_without_writing_either_account() {
        let store = Store::open_in_memory().await.unwrap();
        let server_id = Uuid::from_u128(20);
        let old_account = "external/old";
        let new_account = "external/new";
        store
            .put_json(
                MCP_SERVER_KIND,
                &server_id.to_string(),
                &mcp(server_id, "Server", vec![binding("TOKEN", new_account)]),
            )
            .await
            .unwrap();
        let vault = TestVault::default();
        let error = replace_credential(
            &store,
            &vault,
            ReplaceCredentialRequest {
                target: CredentialTarget::McpBinding {
                    server_id,
                    name: "TOKEN".into(),
                },
                expected_reference: old_account.into(),
                expected_value_kind: CredentialValueKind::ApiKey,
                secret: "secret-sentinel".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(error.contains("binding changed"));
        assert!(vault.values.lock().unwrap().is_empty());
        assert!(vault.set_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn changed_ssh_authentication_kind_rejects_stale_replace() {
        let store = Store::open_in_memory().await.unwrap();
        let id = Uuid::from_u128(22);
        let account = credential_account("ssh", id);
        store
            .save_connection(&ConnectionProfile {
                id,
                label: "Cluster".into(),
                host: "cluster.example".into(),
                port: 22,
                username: "user".into(),
                authentication: AuthenticationMethod::Password,
                authentication_reference: account.clone(),
                host_key_fingerprint: None,
            })
            .await
            .unwrap();
        let vault = TestVault::default();
        let error = replace_credential(
            &store,
            &vault,
            ReplaceCredentialRequest {
                target: CredentialTarget::Ssh { id },
                expected_reference: account,
                expected_value_kind: CredentialValueKind::SshPrivateKey,
                secret: r#"{"path":"C:\\keys\\old","passphrase":null}"#.into(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            "Credential value type changed; refresh before retrying"
        );
        assert!(vault.set_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_failure_leaves_non_secret_missing_metadata_for_retry() {
        let store = Store::open_in_memory().await.unwrap();
        let vault = TestVault::default();
        *vault.fail_set.lock().unwrap() = true;
        let secret = "secret-sentinel";
        let result = create_credential(
            &store,
            &vault,
            CreateManagedCredentialRequest {
                label: " Research token ".into(),
                secret: secret.into(),
            },
        )
        .await
        .unwrap();
        let entry = match result {
            CreateCredentialResult::SecretSaveNotConfirmed { entry } => entry,
            other => panic!("expected unconfirmed save, got {other:?}"),
        };
        assert_eq!(entry.presence, CredentialPresence::Missing);
        assert!(!serde_json::to_string(&entry).unwrap().contains(secret));
        *vault.fail_set.lock().unwrap() = false;
        let entries = list_credentials(&store, &vault).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Research token");
        assert_eq!(entries[0].presence, CredentialPresence::Missing);
        let stored = store
            .list_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND)
            .await
            .unwrap();
        assert!(!serde_json::to_string(&stored).unwrap().contains(secret));
    }

    #[tokio::test]
    async fn create_returns_the_new_identity_even_when_unrelated_inventory_is_corrupt() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .put_json(
                MCP_SERVER_KIND,
                "corrupt-server",
                &serde_json::json!({"not":"an MCP profile"}),
            )
            .await
            .unwrap();
        let vault = TestVault::default();
        let result = create_credential(
            &store,
            &vault,
            CreateManagedCredentialRequest {
                label: "Independent entry".into(),
                secret: "secret-sentinel".into(),
            },
        )
        .await
        .unwrap();
        let entry = match result {
            CreateCredentialResult::Saved { entry } => entry,
            other => panic!("expected saved entry, got {other:?}"),
        };
        assert!(matches!(entry.target, CredentialTarget::Managed { .. }));
        assert_eq!(entry.presence, CredentialPresence::Present);
        assert!(list_credentials(&store, &vault).await.is_err());
    }

    #[tokio::test]
    async fn referenced_managed_credential_is_not_deleted_and_vault_errors_are_safe() {
        let store = Store::open_in_memory().await.unwrap();
        let id = Uuid::from_u128(30);
        let metadata = ManagedCredentialMetadata {
            id,
            label: "Managed".into(),
            created_at: Utc::now(),
        };
        store
            .put_json(MANAGED_CREDENTIAL_KIND, &id.to_string(), &metadata)
            .await
            .unwrap();
        let account = credential_account("settings", id);
        let server_id = Uuid::from_u128(31);
        store
            .put_json(
                MCP_SERVER_KIND,
                &server_id.to_string(),
                &mcp(server_id, "Server", vec![binding("TOKEN", &account)]),
            )
            .await
            .unwrap();
        let vault = TestVault::default();
        let result = delete_credential(&store, &vault, id).await.unwrap();
        assert!(matches!(result, DeleteCredentialResult::InUse { .. }));
        assert!(vault.delete_calls.lock().unwrap().is_empty());

        store
            .put_json(
                MCP_SERVER_KIND,
                &server_id.to_string(),
                &mcp(server_id, "Server", vec![]),
            )
            .await
            .unwrap();
        *vault.fail_delete.lock().unwrap() = true;
        let error = delete_credential(&store, &vault, id).await.unwrap_err();
        assert_eq!(error, "Credential secret could not be deleted");
        assert!(
            store
                .get_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND, &id.to_string())
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn managed_references_require_canonical_uuid_and_matching_metadata_identity() {
        let store = Store::open_in_memory().await.unwrap();
        let id = Uuid::from_u128(0xaaaa);
        let metadata = ManagedCredentialMetadata {
            id,
            label: "Managed".into(),
            created_at: Utc::now(),
        };
        store
            .put_json(MANAGED_CREDENTIAL_KIND, &id.to_string(), &metadata)
            .await
            .unwrap();
        let braced = format!("settings/{{{id}}}");
        let error = validate_managed_credential_references(&store, &[binding("TOKEN", &braced)])
            .await
            .unwrap_err();
        assert_eq!(error, "Managed credential reference is invalid");

        let lookup_id = Uuid::from_u128(0xbbbb);
        store
            .put_json(MANAGED_CREDENTIAL_KIND, &lookup_id.to_string(), &metadata)
            .await
            .unwrap();
        let mismatched = credential_account("settings", lookup_id);
        let error =
            validate_managed_credential_references(&store, &[binding("TOKEN", &mismatched)])
                .await
                .unwrap_err();
        assert_eq!(error, "Managed credential reference was not found");
        assert_eq!(
            delete_credential(&store, &TestVault::default(), lookup_id)
                .await
                .unwrap_err(),
            "Managed credential metadata identity is invalid"
        );
    }

    #[tokio::test]
    async fn managed_label_rejects_secret_like_assignments_without_persisting_them() {
        let store = Store::open_in_memory().await.unwrap();
        let value = "api_key=secret-sentinel";
        let error = create_credential(
            &store,
            &TestVault::default(),
            CreateManagedCredentialRequest {
                label: value.into(),
                secret: "actual-secret".into(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            "Credential label contains prohibited sensitive content"
        );
        assert!(
            store
                .list_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn managed_delete_and_new_mcp_reference_cannot_leave_a_dangling_binding() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let vault = Arc::new(TestVault::default());
        let mutations = Arc::new(CredentialMutationState::default());
        let id = Uuid::from_u128(0xcccc);
        let reference = credential_account("settings", id);
        store
            .put_json(
                MANAGED_CREDENTIAL_KIND,
                &id.to_string(),
                &ManagedCredentialMetadata {
                    id,
                    label: "Managed".into(),
                    created_at: Utc::now(),
                },
            )
            .await
            .unwrap();
        vault.set(&reference, "secret-sentinel").unwrap();
        let server_id = Uuid::from_u128(0xcccd);
        let profile = mcp(server_id, "Server", vec![binding("TOKEN", &reference)]);
        let barrier = Arc::new(tokio::sync::Barrier::new(3));

        let save = {
            let store = Arc::clone(&store);
            let mutations = Arc::clone(&mutations);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                persist_mcp_profile_with_credential_guard(&store, &mutations, &profile).await
            })
        };
        let delete = {
            let store = Arc::clone(&store);
            let vault = Arc::clone(&vault);
            let mutations = Arc::clone(&mutations);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                delete_credential_with_guard(&store, vault.as_ref(), &mutations, id).await
            })
        };
        barrier.wait().await;
        let save_result = save.await.unwrap();
        let delete_result = delete.await.unwrap();

        let deleted = matches!(delete_result, Ok(DeleteCredentialResult::Deleted));
        assert_ne!(save_result.is_ok(), deleted);
        if save_result.is_ok() {
            assert!(matches!(
                delete_result,
                Ok(DeleteCredentialResult::InUse { .. })
            ));
            assert!(
                store
                    .get_json::<ManagedCredentialMetadata>(MANAGED_CREDENTIAL_KIND, &id.to_string())
                    .await
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                vault.get(&reference).unwrap().as_deref(),
                Some("secret-sentinel")
            );
        } else {
            assert!(
                store
                    .get_json::<McpServerProfile>(MCP_SERVER_KIND, &server_id.to_string())
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }
}

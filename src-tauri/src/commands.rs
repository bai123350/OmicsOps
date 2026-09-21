use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8},
    },
};

use omicsops_adapters::{
    credentials::{CredentialVault, SystemCredentialVault, credential_account},
    llm::{ProviderProtocol, UnifiedModelClient},
    ssh::{SshAuthentication, SshSession},
};
use omicsops_core::{
    domain::{AuthenticationMethod, ConnectionProfile, ProjectSpec},
    project::RemoteProjectLayout,
};
use omicsops_mcp::McpSessionManager;
use omicsops_store::Store;
use serde::{Deserialize, Serialize};
use tauri::State;
use url::Url;
use uuid::Uuid;

use crate::inspection::{ServerInspection, inspection_command, parse_server_inspection};

pub struct AppState {
    pub repository: Store,
    pub credentials: SystemCredentialVault,
    pub mcp_sessions: McpSessionManager,
    pub active_runs: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    pub skills_root: PathBuf,
    pub skills_gate: Arc<tokio::sync::RwLock<()>>,
    pub research_last_request: Arc<tokio::sync::Mutex<HashMap<String, std::time::Instant>>>,
    pub active_kernels: crate::kernel_commands::ActiveKernelMap,
    pub project_kernel_queues: Arc<tokio::sync::Mutex<HashMap<Uuid, Arc<tokio::sync::Semaphore>>>>,
    pub sync_controls: Arc<Mutex<HashMap<Uuid, Arc<AtomicU8>>>>,
    pub browser: omicsops_browser::BrowserRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateKeySecret {
    pub path: String,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub fingerprint: String,
    pub trusted: bool,
    pub authenticated: bool,
    pub latency_ms: u128,
    pub server_os: Option<String>,
    pub remote_username: Option<String>,
    pub home: Option<String>,
    pub sftp_available: bool,
    pub python_available: bool,
    pub r_available: bool,
}

pub fn parse_authentication_secret(
    method: AuthenticationMethod,
    secret: &str,
) -> Result<SshAuthentication, String> {
    match method {
        AuthenticationMethod::Password => Ok(SshAuthentication::Password(secret.to_owned())),
        AuthenticationMethod::PrivateKey => {
            let secret: PrivateKeySecret =
                serde_json::from_str(secret).map_err(|error| error.to_string())?;
            if secret.path.trim().is_empty() {
                return Err("private key path is empty".into());
            }
            Ok(SshAuthentication::PrivateKey {
                path: secret.path.into(),
                passphrase: secret.passphrase.filter(|value| !value.is_empty()),
            })
        }
    }
}

#[tauri::command]
pub async fn list_connections(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectionProfile>, String> {
    state
        .repository
        .list_connections()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_connection(
    state: State<'_, AppState>,
    credential_mutations: State<'_, crate::credential_settings::CredentialMutationState>,
    mut profile: ConnectionProfile,
    secret: String,
) -> Result<(), String> {
    let _credential_guard = credential_mutations.lock.lock().await;
    let previous = state
        .repository
        .list_connections()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|item| item.id == profile.id);
    normalize_connection_profile(&mut profile, previous.as_ref())?;
    if secret.is_empty() {
        let existing = state
            .credentials
            .get(&profile.authentication_reference)
            .map_err(|error| error.to_string())?;
        if existing.is_none()
            || previous
                .as_ref()
                .is_some_and(|item| item.authentication != profile.authentication)
        {
            return Err("SSH credential is required for this connection".into());
        }
    } else {
        state
            .credentials
            .set(&profile.authentication_reference, &secret)
            .map_err(|error| error.to_string())?;
    }
    state
        .repository
        .save_connection(&profile)
        .await
        .map_err(|error| error.to_string())
}

pub fn normalize_connection_profile(
    profile: &mut ConnectionProfile,
    previous: Option<&ConnectionProfile>,
) -> Result<(), String> {
    profile.label = profile.label.trim().to_owned();
    profile.host = profile.host.trim().to_owned();
    profile.username = profile.username.trim().to_owned();
    if profile.label.is_empty() || profile.host.is_empty() || profile.username.is_empty() {
        return Err("label, host, and username are required".into());
    }
    if profile.port == 0 || profile.host.chars().any(char::is_whitespace) {
        return Err("SSH host or port is invalid".into());
    }
    if let Some(previous) = previous {
        if previous.host != profile.host
            || previous.port != profile.port
            || previous.username != profile.username
        {
            profile.host_key_fingerprint = None;
        } else {
            profile.host_key_fingerprint = previous.host_key_fingerprint.clone();
        }
    }
    profile.authentication_reference = credential_account("ssh", profile.id);
    Ok(())
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    profile_id: Uuid,
) -> Result<ConnectionTestResult, String> {
    let profile = find_profile(&state.repository, profile_id).await?;
    let started = std::time::Instant::now();
    let fingerprint = SshSession::probe_host_key(&profile)
        .await
        .map_err(|error| error.to_string())?;
    if profile.host_key_fingerprint.is_none() {
        return Ok(ConnectionTestResult {
            fingerprint,
            trusted: false,
            authenticated: false,
            latency_ms: started.elapsed().as_millis(),
            server_os: None,
            remote_username: None,
            home: None,
            sftp_available: false,
            python_available: false,
            r_available: false,
        });
    }
    if profile.host_key_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(format!(
            "SSH host key changed; expected {}, received {}",
            profile.host_key_fingerprint.as_deref().unwrap_or_default(),
            fingerprint
        ));
    }
    let session = connect_profile(&state, &profile).await?;
    let diagnostics = session.execute_checked("printf 'os='; uname -srm; printf 'user='; id -un; printf 'home=%s\\n' \"$HOME\"; command -v python3 >/dev/null && printf 'python=1\\n' || printf 'python=0\\n'; command -v Rscript >/dev/null && printf 'r=1\\n' || printf 'r=0\\n'").await.map_err(|error| error.to_string())?;
    let sftp_available = session.probe_sftp().await.is_ok();
    let fields: HashMap<&str, &str> = diagnostics
        .stdout
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let result = ConnectionTestResult {
        fingerprint,
        trusted: true,
        authenticated: true,
        latency_ms: started.elapsed().as_millis(),
        server_os: fields.get("os").map(|value| (*value).to_owned()),
        remote_username: fields.get("user").map(|value| (*value).to_owned()),
        home: fields.get("home").map(|value| (*value).to_owned()),
        sftp_available,
        python_available: fields.get("python") == Some(&"1"),
        r_available: fields.get("r") == Some(&"1"),
    };
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn confirm_host_key(
    state: State<'_, AppState>,
    profile_id: Uuid,
    fingerprint: String,
) -> Result<(), String> {
    let mut profile = find_profile(&state.repository, profile_id).await?;
    let observed = SshSession::probe_host_key(&profile)
        .await
        .map_err(|error| error.to_string())?;
    if fingerprint != observed {
        return Err(format!(
            "host key confirmation did not match the server; received {observed}"
        ));
    }
    profile.host_key_fingerprint = Some(fingerprint);
    state
        .repository
        .save_connection(&profile)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn inspect_project(
    state: State<'_, AppState>,
    profile_id: Uuid,
    remote_root: String,
) -> Result<ServerInspection, String> {
    let profile = find_profile(&state.repository, profile_id).await?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    let output = session
        .execute_checked(&inspection_command(&remote_root))
        .await
        .map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    parse_server_inspection(&output.stdout)
}

#[tauri::command]
pub async fn initialize_project(
    state: State<'_, AppState>,
    profile_id: Uuid,
    project: ProjectSpec,
) -> Result<ServerInspection, String> {
    let profile = find_profile(&state.repository, profile_id).await?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    let before = session
        .execute_checked(&inspection_command(&project.remote_root))
        .await
        .map_err(|error| error.to_string())?;
    let inspection = parse_server_inspection(&before.stdout)?;
    if !inspection.project_empty {
        return Err("the selected remote project directory is not empty".into());
    }
    if !inspection.owner_matches {
        return Err("the dedicated SSH user does not own the project directory".into());
    }
    session
        .execute_checked(&RemoteProjectLayout::new(&project.remote_root).initialization_command())
        .await
        .map_err(|error| error.to_string())?;
    state
        .repository
        .put_json("project", &project.id.to_string(), &project)
        .await
        .map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    Ok(inspection)
}

pub(crate) fn authentication_for_profile(
    state: &AppState,
    profile: &ConnectionProfile,
) -> Result<SshAuthentication, String> {
    let secret = state
        .credentials
        .get(&profile.authentication_reference)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "SSH credential is missing".to_string())?;
    parse_authentication_secret(profile.authentication, &secret)
}

pub(crate) async fn find_profile(
    repository: &Store,
    id: Uuid,
) -> Result<ConnectionProfile, String> {
    repository
        .list_connections()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| format!("connection profile {id} was not found"))
}

pub(crate) fn require_trusted_host(profile: &ConnectionProfile) -> Result<(), String> {
    if profile.host_key_fingerprint.is_none() {
        Err("confirm the server host-key fingerprint before remote operations".into())
    } else {
        Ok(())
    }
}

pub(crate) async fn connect_profile(
    state: &AppState,
    profile: &ConnectionProfile,
) -> Result<SshSession, String> {
    let authentication = authentication_for_profile(state, profile)?;
    SshSession::connect(profile, authentication)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn unified_model_client_for_profile(
    state: &AppState,
    profile: &omicsops_core::workspace::ModelProfile,
) -> Result<UnifiedModelClient, String> {
    if profile.fast_mode == Some(true) && !profile.supports_fast_mode() {
        return Err(
            "explicit Fast mode requires an exact supported OpenAI endpoint and model".into(),
        );
    }
    let credential = match &profile.credential_reference {
        Some(reference) => state
            .credentials
            .get(reference)
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let protocol = match profile.provider {
        omicsops_core::workspace::ModelProviderKind::Anthropic => ProviderProtocol::Anthropic,
        omicsops_core::workspace::ModelProviderKind::OpenAiCompatible => {
            ProviderProtocol::OpenAiCompatible
        }
        omicsops_core::workspace::ModelProviderKind::Ollama => ProviderProtocol::Ollama,
    };
    UnifiedModelClient::new(
        profile.id,
        protocol,
        Url::parse(&profile.base_url).map_err(|error| error.to_string())?,
        profile.model.clone(),
        credential,
    )
    .and_then(|client| client.with_reasoning_effort(profile.reasoning_effort.clone()))
    .map(|client| client.with_fast_mode(profile.fast_mode))
    .map_err(|error| error.to_string())
}

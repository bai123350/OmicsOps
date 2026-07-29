use std::{path::PathBuf, sync::Arc};

use chrono::Utc;
use omicsops_adapters::{
    credentials::{CredentialVault, SystemCredentialVault, credential_account},
    document::{ExtractedPlan, extract_plan_text},
    llm::OpenAiCompatibleClient,
    persistence::Repository,
    ssh::{SshAuthentication, SshSession},
};
use omicsops_core::{
    domain::{
        AnalysisPlan, Artifact, ArtifactKind, AuthenticationMethod, ConnectionProfile, ProjectSpec,
        RunEvent, RunState,
    },
    project::{RemoteProjectLayout, shell_quote},
};
use omicsops_runner::{
    AutonomousRunner, LlmRepairPlanner, SshRemoteExecutor, StepOutcome, generate_analysis_plan,
    scrna::{
        PBMC_DOWNLOAD_SCRIPT, PBMC_ENVIRONMENT_YAML, PBMC_REPORT_SCRIPT, PBMC_SCANPY_SCRIPT,
        SEURAT_CONVERTER_R,
    },
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use url::Url;
use uuid::Uuid;

use crate::inspection::{ServerInspection, inspection_command, parse_server_inspection};

pub struct AppState {
    pub repository: Repository,
    pub credentials: SystemCredentialVault,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    pub base_url: Url,
    pub model: String,
    pub credential_reference: String,
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
pub fn list_connections(state: State<'_, AppState>) -> Result<Vec<ConnectionProfile>, String> {
    state
        .repository
        .list_connections()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_connection(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    secret: String,
) -> Result<(), String> {
    if profile.host.trim().is_empty() || profile.username.trim().is_empty() {
        return Err("host and username are required".into());
    }
    state
        .credentials
        .set(&profile.authentication_reference, &secret)
        .map_err(|error| error.to_string())?;
    state
        .repository
        .save_connection(&profile)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    profile_id: Uuid,
) -> Result<ConnectionTestResult, String> {
    let profile = find_profile(&state.repository, profile_id)?;
    let session = connect_profile(&state, &profile).await?;
    let result = ConnectionTestResult {
        fingerprint: session.fingerprint().to_owned(),
        trusted: profile.host_key_fingerprint.as_deref() == Some(session.fingerprint()),
    };
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn confirm_host_key(
    state: State<'_, AppState>,
    profile_id: Uuid,
    fingerprint: String,
) -> Result<(), String> {
    let mut profile = find_profile(&state.repository, profile_id)?;
    profile.host_key_fingerprint = Some(fingerprint);
    state
        .repository
        .save_connection(&profile)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn inspect_project(
    state: State<'_, AppState>,
    profile_id: Uuid,
    remote_root: String,
) -> Result<ServerInspection, String> {
    let profile = find_profile(&state.repository, profile_id)?;
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
    let profile = find_profile(&state.repository, profile_id)?;
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

    let layout = RemoteProjectLayout::new(&project.remote_root);
    session
        .execute_checked(&layout.initialization_command())
        .await
        .map_err(|error| error.to_string())?;
    let uploads = [
        ("scripts/download_pbmc.sh", PBMC_DOWNLOAD_SCRIPT),
        ("scripts/analyze_pbmc.py", PBMC_SCANPY_SCRIPT),
        ("scripts/convert_h5ad_to_seurat.R", SEURAT_CONVERTER_R),
        ("scripts/render_report.py", PBMC_REPORT_SCRIPT),
        (".omicsops/environment.yml", PBMC_ENVIRONMENT_YAML),
    ];
    for (relative, contents) in uploads {
        session
            .upload_text(&format!("{}/{}", project.remote_root, relative), contents)
            .await
            .map_err(|error| error.to_string())?;
    }
    session
        .execute_checked(&format!(
            "chmod 700 {}/scripts/*.sh {}/scripts/*.py {}/scripts/*.R",
            shell_quote(&project.remote_root),
            shell_quote(&project.remote_root),
            shell_quote(&project.remote_root)
        ))
        .await
        .map_err(|error| error.to_string())?;
    state
        .repository
        .put_json("project", &project.id.to_string(), &project)
        .map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    Ok(inspection)
}

#[tauri::command]
pub fn extract_plan(path: String) -> Result<ExtractedPlan, String> {
    extract_plan_text(PathBuf::from(path).as_path()).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_llm_config(
    state: State<'_, AppState>,
    base_url: Url,
    model: String,
    api_key: String,
) -> Result<LlmConfig, String> {
    let credential_reference = credential_account("llm", Uuid::nil());
    state
        .credentials
        .set(&credential_reference, &api_key)
        .map_err(|error| error.to_string())?;
    let config = LlmConfig {
        base_url,
        model,
        credential_reference,
    };
    state
        .repository
        .put_json("config", "llm", &config)
        .map_err(|error| error.to_string())?;
    Ok(config)
}

#[tauri::command]
pub async fn probe_llm(state: State<'_, AppState>) -> Result<(), String> {
    llm_client(&state)?
        .probe_tool_calling()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn generate_plan(
    state: State<'_, AppState>,
    document_text: String,
    inspection: ServerInspection,
) -> Result<AnalysisPlan, String> {
    let inspection =
        serde_json::to_string_pretty(&inspection).map_err(|error| error.to_string())?;
    generate_analysis_plan(&llm_client(&state)?, &document_text, &inspection)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn approve_plan(
    state: State<'_, AppState>,
    mut plan: AnalysisPlan,
) -> Result<AnalysisPlan, String> {
    plan.approved = true;
    state
        .repository
        .put_json("analysis_plan", &plan.id.to_string(), &plan)
        .map_err(|error| error.to_string())?;
    Ok(plan)
}

#[tauri::command]
pub async fn start_run(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: Uuid,
    project: ProjectSpec,
    plan: AnalysisPlan,
) -> Result<Uuid, String> {
    if !plan.approved {
        return Err("the analysis plan must be approved before execution".into());
    }
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let secret = state
        .credentials
        .get(&profile.authentication_reference)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "SSH credential is missing".to_string())?;
    let authentication = parse_authentication_secret(profile.authentication, &secret)?;
    let llm = llm_client(&state)?;
    let repository = state.repository.clone();
    let run_id = Uuid::new_v4();

    tauri::async_runtime::spawn(async move {
        let mut sequence = 0_u64;
        let emit = |state_value: RunState,
                    action: &str,
                    reason: String,
                    stage: Option<String>,
                    step: Option<String>,
                    attempt: u8,
                    sequence_value: &mut u64| {
            *sequence_value += 1;
            let event = RunEvent {
                sequence: *sequence_value,
                timestamp: Utc::now(),
                run_id,
                stage_id: stage,
                step_id: step,
                attempt,
                action: action.into(),
                state: state_value,
                log_reference: None,
                reason,
            };
            let _ = repository.append_event(&event);
            let _ = app.emit("run-event", &event);
        };

        let session = match SshSession::connect(&profile, authentication).await {
            Ok(session) => Arc::new(session),
            Err(error) => {
                emit(
                    RunState::Failed,
                    "connect",
                    error.to_string(),
                    None,
                    None,
                    0,
                    &mut sequence,
                );
                return;
            }
        };
        emit(
            RunState::Running,
            "run_started",
            plan.summary.clone(),
            None,
            None,
            0,
            &mut sequence,
        );
        let allowed = project
            .allowed_network_domains
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let runner = AutonomousRunner::new(
            &project.remote_root,
            &allowed,
            SshRemoteExecutor::new(session.clone(), &project.remote_root),
            LlmRepairPlanner::new(llm),
        );

        for stage in plan.stages {
            for step in stage.steps {
                emit(
                    RunState::Running,
                    "step_started",
                    step.rationale.clone(),
                    Some(stage.id.clone()),
                    Some(step.id.clone()),
                    0,
                    &mut sequence,
                );
                match runner.run_step(step.clone()).await {
                    Ok(StepOutcome::Succeeded { attempt, .. }) => emit(
                        RunState::Running,
                        "step_succeeded",
                        "completion conditions passed".into(),
                        Some(stage.id.clone()),
                        Some(step.id),
                        attempt,
                        &mut sequence,
                    ),
                    Ok(StepOutcome::AwaitingApproval { reason, .. }) => {
                        emit(
                            RunState::PausedForApproval,
                            "approval_required",
                            reason,
                            Some(stage.id),
                            Some(step.id),
                            0,
                            &mut sequence,
                        );
                        return;
                    }
                    Ok(StepOutcome::Denied { reason }) => {
                        emit(
                            RunState::Failed,
                            "policy_denied",
                            reason,
                            Some(stage.id),
                            Some(step.id),
                            0,
                            &mut sequence,
                        );
                        return;
                    }
                    Ok(StepOutcome::NeedsAttention {
                        repairs_attempted,
                        last_error,
                    }) => {
                        emit(
                            RunState::Failed,
                            "repair_budget_exhausted",
                            last_error,
                            Some(stage.id),
                            Some(step.id),
                            repairs_attempted,
                            &mut sequence,
                        );
                        return;
                    }
                    Err(error) => {
                        emit(
                            RunState::Failed,
                            "runner_error",
                            error.to_string(),
                            Some(stage.id),
                            Some(step.id),
                            0,
                            &mut sequence,
                        );
                        return;
                    }
                }
            }
        }
        emit(
            RunState::Succeeded,
            "run_succeeded",
            "all stages completed".into(),
            None,
            None,
            0,
            &mut sequence,
        );
        if let Ok(session) = Arc::try_unwrap(session) {
            let _ = session.disconnect().await;
        }
    });
    Ok(run_id)
}

#[tauri::command]
pub async fn list_artifacts(
    state: State<'_, AppState>,
    profile_id: Uuid,
    remote_root: String,
) -> Result<Vec<Artifact>, String> {
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    let results = format!("{}/results", remote_root.trim_end_matches('/'));
    let output = session
        .execute_checked(&format!(
            "find {results} -type f -printf '%p\\t%s\\n' | sort",
            results = shell_quote(&results)
        ))
        .await
        .map_err(|error| error.to_string())?;
    let mut artifacts = Vec::new();
    for line in output.stdout.lines() {
        let Some((path, size)) = line.rsplit_once('\t') else {
            continue;
        };
        let checksum = session
            .execute_checked(&format!("sha256sum {}", shell_quote(path)))
            .await
            .map_err(|error| error.to_string())?
            .stdout
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        let kind = artifact_kind(path);
        artifacts.push(Artifact {
            remote_path: path.into(),
            kind,
            size_bytes: size.parse().unwrap_or(0),
            sha256: checksum,
            previewable: matches!(kind, ArtifactKind::HtmlReport | ArtifactKind::Figure),
            downloadable: true,
        });
    }
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    Ok(artifacts)
}

#[tauri::command]
pub async fn download_artifact(
    state: State<'_, AppState>,
    profile_id: Uuid,
    remote_path: String,
    local_path: String,
) -> Result<(), String> {
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    session
        .download_atomic(&remote_path, PathBuf::from(local_path).as_path())
        .await
        .map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())
}

fn find_profile(repository: &Repository, id: Uuid) -> Result<ConnectionProfile, String> {
    repository
        .list_connections()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| format!("connection profile {id} was not found"))
}

fn require_trusted_host(profile: &ConnectionProfile) -> Result<(), String> {
    if profile.host_key_fingerprint.is_none() {
        Err("confirm the server host-key fingerprint before remote operations".into())
    } else {
        Ok(())
    }
}

async fn connect_profile(
    state: &State<'_, AppState>,
    profile: &ConnectionProfile,
) -> Result<SshSession, String> {
    let secret = state
        .credentials
        .get(&profile.authentication_reference)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "SSH credential is missing".to_string())?;
    let authentication = parse_authentication_secret(profile.authentication, &secret)?;
    SshSession::connect(profile, authentication)
        .await
        .map_err(|error| error.to_string())
}

fn llm_client(state: &State<'_, AppState>) -> Result<OpenAiCompatibleClient, String> {
    let config: LlmConfig = state
        .repository
        .get_json("config", "llm")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "model endpoint is not configured".to_string())?;
    let api_key = state
        .credentials
        .get(&config.credential_reference)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "model API key is missing".to_string())?;
    Ok(OpenAiCompatibleClient::new(
        config.base_url,
        config.model,
        api_key,
    ))
}

fn artifact_kind(path: &str) -> ArtifactKind {
    if path.ends_with(".h5ad") {
        ArtifactKind::H5ad
    } else if path.ends_with(".rds") {
        ArtifactKind::SeuratRds
    } else if path.ends_with(".html") {
        ArtifactKind::HtmlReport
    } else if path.ends_with(".png") || path.ends_with(".svg") || path.ends_with(".pdf") {
        ArtifactKind::Figure
    } else if path.ends_with(".yml") || path.ends_with(".yaml") || path.ends_with(".lock") {
        ArtifactKind::EnvironmentLock
    } else if path.contains("audit") || path.ends_with(".jsonl") {
        ArtifactKind::AuditLog
    } else {
        ArtifactKind::Other
    }
}

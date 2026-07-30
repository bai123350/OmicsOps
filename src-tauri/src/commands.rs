use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

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
        RunCheckpoint, RunEvent, RunState,
    },
    project::{RemoteProjectLayout, require_remote_descendant, shell_quote},
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
    pub active_runs: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
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
    let run_id = Uuid::new_v4();
    let checkpoint = RunCheckpoint::new(run_id, profile_id, project.id, plan.id);
    state
        .repository
        .put_json("project", &project.id.to_string(), &project)
        .map_err(|error| error.to_string())?;
    state
        .repository
        .put_json("analysis_plan", &plan.id.to_string(), &plan)
        .map_err(|error| error.to_string())?;
    state
        .repository
        .put_json("run_checkpoint", &run_id.to_string(), &checkpoint)
        .map_err(|error| error.to_string())?;

    let authentication = authentication_for_profile(&state, &profile)?;
    spawn_checkpoint_run(
        app,
        state.repository.clone(),
        state.active_runs.clone(),
        profile,
        authentication,
        llm_client(&state)?,
        project,
        plan,
        checkpoint,
        None,
    )?;
    Ok(run_id)
}

#[tauri::command]
pub fn list_runs(state: State<'_, AppState>) -> Result<Vec<RunCheckpoint>, String> {
    state
        .repository
        .list_json("run_checkpoint")
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_run_events(state: State<'_, AppState>, run_id: Uuid) -> Result<Vec<RunEvent>, String> {
    state
        .repository
        .events_for_run(run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn resume_run(app: AppHandle, state: State<'_, AppState>, run_id: Uuid) -> Result<(), String> {
    let (mut checkpoint, profile, project, plan) = load_run_context(&state, run_id)?;
    if matches!(checkpoint.state, RunState::Succeeded | RunState::Canceled) {
        return Err(format!("run {} is already {}", run_id, checkpoint.state));
    }
    if checkpoint.pending_approval.is_some() {
        return Err("the pending approval must be resolved before resuming".into());
    }
    checkpoint.state = RunState::Preparing;
    state
        .repository
        .put_json("run_checkpoint", &run_id.to_string(), &checkpoint)
        .map_err(|error| error.to_string())?;
    let authentication = authentication_for_profile(&state, &profile)?;
    spawn_checkpoint_run(
        app,
        state.repository.clone(),
        state.active_runs.clone(),
        profile,
        authentication,
        llm_client(&state)?,
        project,
        plan,
        checkpoint,
        None,
    )
}

#[tauri::command]
pub fn approve_run(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
    approval_id: Uuid,
) -> Result<(), String> {
    let (mut checkpoint, profile, project, plan) = load_run_context(&state, run_id)?;
    let approved_command = checkpoint
        .approve(approval_id)
        .map_err(|error| error.to_string())?;
    state
        .repository
        .put_json("run_checkpoint", &run_id.to_string(), &checkpoint)
        .map_err(|error| error.to_string())?;
    let authentication = authentication_for_profile(&state, &profile)?;
    spawn_checkpoint_run(
        app,
        state.repository.clone(),
        state.active_runs.clone(),
        profile,
        authentication,
        llm_client(&state)?,
        project,
        plan,
        checkpoint,
        Some(approved_command),
    )
}

#[tauri::command]
pub fn cancel_run(app: AppHandle, state: State<'_, AppState>, run_id: Uuid) -> Result<(), String> {
    if let Some(requested) = state
        .active_runs
        .lock()
        .map_err(|_| "active run registry is unavailable".to_string())?
        .get(&run_id)
        .cloned()
    {
        requested.store(true, Ordering::SeqCst);
        return Ok(());
    }

    let mut checkpoint: RunCheckpoint = state
        .repository
        .get_json("run_checkpoint", &run_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("run {run_id} was not found"))?;
    if matches!(checkpoint.state, RunState::Succeeded | RunState::Canceled) {
        return Ok(());
    }
    checkpoint.state = RunState::Canceled;
    checkpoint.pending_approval = None;
    state
        .repository
        .put_json("run_checkpoint", &run_id.to_string(), &checkpoint)
        .map_err(|error| error.to_string())?;
    let mut sequence = next_event_sequence(&state.repository, run_id);
    emit_run_event(
        &app,
        &state.repository,
        &mut sequence,
        &checkpoint,
        "run_canceled",
        "run canceled while no remote step was active".into(),
        None,
        None,
        0,
    );
    Ok(())
}

fn authentication_for_profile(
    state: &State<'_, AppState>,
    profile: &ConnectionProfile,
) -> Result<SshAuthentication, String> {
    let secret = state
        .credentials
        .get(&profile.authentication_reference)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "SSH credential is missing".to_string())?;
    parse_authentication_secret(profile.authentication, &secret)
}

fn load_run_context(
    state: &State<'_, AppState>,
    run_id: Uuid,
) -> Result<(RunCheckpoint, ConnectionProfile, ProjectSpec, AnalysisPlan), String> {
    let checkpoint: RunCheckpoint = state
        .repository
        .get_json("run_checkpoint", &run_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("run {run_id} was not found"))?;
    let profile = find_profile(&state.repository, checkpoint.profile_id)?;
    require_trusted_host(&profile)?;
    let project = state
        .repository
        .get_json("project", &checkpoint.project_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the run project could not be recovered".to_string())?;
    let plan = state
        .repository
        .get_json("analysis_plan", &checkpoint.plan_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the run analysis plan could not be recovered".to_string())?;
    Ok((checkpoint, profile, project, plan))
}

#[allow(clippy::too_many_arguments)]
fn spawn_checkpoint_run(
    app: AppHandle,
    repository: Repository,
    active_runs: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    profile: ConnectionProfile,
    authentication: SshAuthentication,
    llm: OpenAiCompatibleClient,
    project: ProjectSpec,
    plan: AnalysisPlan,
    checkpoint: RunCheckpoint,
    approved_command: Option<String>,
) -> Result<(), String> {
    let run_id = checkpoint.run_id;
    let cancel_requested = Arc::new(AtomicBool::new(false));
    {
        let mut active = active_runs
            .lock()
            .map_err(|_| "active run registry is unavailable".to_string())?;
        if active.contains_key(&run_id) {
            return Err(format!("run {run_id} is already active"));
        }
        active.insert(run_id, cancel_requested.clone());
    }

    tauri::async_runtime::spawn(async move {
        execute_checkpoint_run(
            app,
            repository,
            profile,
            authentication,
            llm,
            project,
            plan,
            checkpoint,
            approved_command,
            cancel_requested,
        )
        .await;
        if let Ok(mut active) = active_runs.lock() {
            active.remove(&run_id);
        }
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_checkpoint_run(
    app: AppHandle,
    repository: Repository,
    profile: ConnectionProfile,
    authentication: SshAuthentication,
    llm: OpenAiCompatibleClient,
    project: ProjectSpec,
    plan: AnalysisPlan,
    mut checkpoint: RunCheckpoint,
    mut approved_command: Option<String>,
    cancel_requested: Arc<AtomicBool>,
) {
    let run_id = checkpoint.run_id;
    let mut sequence = next_event_sequence(&repository, run_id);
    if cancel_requested.load(Ordering::SeqCst) {
        checkpoint.state = RunState::Canceled;
        persist_checkpoint(&repository, &checkpoint);
        emit_run_event(
            &app,
            &repository,
            &mut sequence,
            &checkpoint,
            "run_canceled",
            "run canceled before SSH connection".into(),
            None,
            None,
            0,
        );
        return;
    }

    let session = match SshSession::connect(&profile, authentication).await {
        Ok(session) => Arc::new(session),
        Err(error) => {
            checkpoint.state = RunState::Failed;
            persist_checkpoint(&repository, &checkpoint);
            emit_run_event(
                &app,
                &repository,
                &mut sequence,
                &checkpoint,
                "connect_failed",
                error.to_string(),
                None,
                None,
                0,
            );
            return;
        }
    };
    checkpoint.state = RunState::Running;
    persist_checkpoint(&repository, &checkpoint);
    let run_action = if sequence == 0 {
        "run_started"
    } else {
        "run_resumed"
    };
    emit_run_event(
        &app,
        &repository,
        &mut sequence,
        &checkpoint,
        run_action,
        plan.summary.clone(),
        None,
        None,
        0,
    );

    let allowed = project
        .allowed_network_domains
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let runner = AutonomousRunner::new(
        &project.remote_root,
        &allowed,
        SshRemoteExecutor::new(session, &project.remote_root)
            .with_cancellation(cancel_requested.clone()),
        LlmRepairPlanner::new(llm),
    );

    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            checkpoint.state = RunState::Canceled;
            checkpoint.pending_approval = None;
            persist_checkpoint(&repository, &checkpoint);
            emit_run_event(
                &app,
                &repository,
                &mut sequence,
                &checkpoint,
                "run_canceled",
                "active step terminated by user request".into(),
                None,
                None,
                0,
            );
            return;
        }
        let Some((stage, step)) = checkpoint.current_step(&plan) else {
            checkpoint.state = RunState::Succeeded;
            persist_checkpoint(&repository, &checkpoint);
            emit_run_event(
                &app,
                &repository,
                &mut sequence,
                &checkpoint,
                "run_succeeded",
                "all stages completed".into(),
                None,
                None,
                0,
            );
            return;
        };
        let stage_id = stage.id.clone();
        let step_id = step.id.clone();
        let step = step.clone();
        checkpoint.state = RunState::Running;
        persist_checkpoint(&repository, &checkpoint);
        emit_run_event(
            &app,
            &repository,
            &mut sequence,
            &checkpoint,
            "step_started",
            step.rationale.clone(),
            Some(stage_id.clone()),
            Some(step_id.clone()),
            0,
        );

        let outcome = runner
            .run_step_with_approval(step, approved_command.as_deref())
            .await;
        approved_command = None;
        if cancel_requested.load(Ordering::SeqCst) {
            continue;
        }
        match outcome {
            Ok(StepOutcome::Succeeded { attempt, .. }) => {
                let completed = checkpoint.advance_after_success(&plan);
                checkpoint.state = if completed {
                    RunState::Succeeded
                } else {
                    RunState::Running
                };
                persist_checkpoint(&repository, &checkpoint);
                emit_run_event(
                    &app,
                    &repository,
                    &mut sequence,
                    &checkpoint,
                    "step_succeeded",
                    "completion conditions passed".into(),
                    Some(stage_id),
                    Some(step_id),
                    attempt,
                );
                if completed {
                    return;
                }
            }
            Ok(StepOutcome::AwaitingApproval { reason, command }) => {
                checkpoint.pause_for_approval(reason.clone(), command);
                persist_checkpoint(&repository, &checkpoint);
                emit_run_event(
                    &app,
                    &repository,
                    &mut sequence,
                    &checkpoint,
                    "approval_required",
                    reason,
                    Some(stage_id),
                    Some(step_id),
                    0,
                );
                return;
            }
            Ok(StepOutcome::Denied { reason }) => {
                checkpoint.state = RunState::Failed;
                persist_checkpoint(&repository, &checkpoint);
                emit_run_event(
                    &app,
                    &repository,
                    &mut sequence,
                    &checkpoint,
                    "policy_denied",
                    reason,
                    Some(stage_id),
                    Some(step_id),
                    0,
                );
                return;
            }
            Ok(StepOutcome::NeedsAttention {
                repairs_attempted,
                last_error,
            }) => {
                checkpoint.state = RunState::Failed;
                persist_checkpoint(&repository, &checkpoint);
                emit_run_event(
                    &app,
                    &repository,
                    &mut sequence,
                    &checkpoint,
                    "repair_budget_exhausted",
                    last_error,
                    Some(stage_id),
                    Some(step_id),
                    repairs_attempted,
                );
                return;
            }
            Err(error) => {
                checkpoint.state = RunState::Failed;
                persist_checkpoint(&repository, &checkpoint);
                emit_run_event(
                    &app,
                    &repository,
                    &mut sequence,
                    &checkpoint,
                    "runner_error",
                    error.to_string(),
                    Some(stage_id),
                    Some(step_id),
                    0,
                );
                return;
            }
        }
    }
}

fn persist_checkpoint(repository: &Repository, checkpoint: &RunCheckpoint) {
    let _ = repository.put_json("run_checkpoint", &checkpoint.run_id.to_string(), checkpoint);
}

fn next_event_sequence(repository: &Repository, run_id: Uuid) -> u64 {
    repository
        .events_for_run(run_id)
        .ok()
        .and_then(|events| events.last().map(|event| event.sequence))
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn emit_run_event(
    app: &AppHandle,
    repository: &Repository,
    sequence: &mut u64,
    checkpoint: &RunCheckpoint,
    action: &str,
    reason: String,
    stage_id: Option<String>,
    step_id: Option<String>,
    attempt: u8,
) {
    *sequence += 1;
    let event = RunEvent {
        sequence: *sequence,
        timestamp: Utc::now(),
        run_id: checkpoint.run_id,
        stage_id,
        step_id,
        attempt,
        action: action.into(),
        state: checkpoint.state,
        log_reference: None,
        reason,
    };
    let _ = repository.append_event(&event);
    let _ = app.emit("run-event", &event);
}

#[tauri::command]
pub async fn list_artifacts(
    state: State<'_, AppState>,
    profile_id: Uuid,
    remote_root: String,
) -> Result<Vec<Artifact>, String> {
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let project = state
        .repository
        .list_json::<ProjectSpec>("project")
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|project| {
            project.connection_id == profile_id
                && project.remote_root.trim_end_matches('/') == remote_root.trim_end_matches('/')
        })
        .ok_or_else(|| "the artifact root is not a persisted project".to_string())?;
    let session = connect_profile(&state, &profile).await?;
    let results = format!("{}/results", project.remote_root.trim_end_matches('/'));
    let canonical_results = canonical_remote_directory(
        &session,
        project.remote_root.trim_end_matches('/'),
        &results,
    )
    .await?;
    let output = session
        .execute_checked(&format!(
            "find {results} -type f -printf '%p\\t%s\\n' | sort",
            results = shell_quote(&canonical_results)
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
    let project = state
        .repository
        .list_json::<ProjectSpec>("project")
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|project| {
            project.connection_id == profile_id
                && require_remote_descendant(
                    &format!("{}/results", project.remote_root.trim_end_matches('/')),
                    &remote_path,
                )
                .is_ok()
        })
        .ok_or_else(|| "the requested artifact is outside every persisted project".to_string())?;
    let session = connect_profile(&state, &profile).await?;
    let canonical_path = canonical_remote_descendant(
        &session,
        &format!("{}/results", project.remote_root.trim_end_matches('/')),
        &remote_path,
    )
    .await?;
    let expected_sha256 = session
        .execute_checked(&format!("sha256sum -- {}", shell_quote(&canonical_path)))
        .await
        .map_err(|error| error.to_string())?
        .stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| "remote artifact checksum was empty".to_string())?
        .to_owned();
    session
        .download_atomic_verified(
            &canonical_path,
            PathBuf::from(local_path).as_path(),
            &expected_sha256,
        )
        .await
        .map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())
}

async fn canonical_remote_descendant(
    session: &SshSession,
    root: &str,
    candidate: &str,
) -> Result<String, String> {
    require_remote_descendant(root, candidate).map_err(|error| error.to_string())?;
    let output = session
        .execute_checked(&format!(
            "root=$(realpath -- {root}) && file=$(realpath -- {candidate}) && \
             case \"$file\" in \"$root\"/*) test -f \"$file\" && printf '%s' \"$file\" ;; \
             *) printf '%s\\n' 'path escaped project boundary' >&2; exit 73 ;; esac",
            root = shell_quote(root),
            candidate = shell_quote(candidate),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let canonical = output.stdout.trim().to_owned();
    if canonical.is_empty() {
        return Err("remote artifact did not resolve to a regular file".into());
    }
    Ok(canonical)
}

async fn canonical_remote_directory(
    session: &SshSession,
    root: &str,
    candidate: &str,
) -> Result<String, String> {
    require_remote_descendant(root, candidate).map_err(|error| error.to_string())?;
    let output = session
        .execute_checked(&format!(
            "root=$(realpath -- {root}) && directory=$(realpath -- {candidate}) && \
             case \"$directory\" in \"$root\"/*) test -d \"$directory\" && printf '%s' \"$directory\" ;; \
             *) printf '%s\\n' 'path escaped project boundary' >&2; exit 73 ;; esac",
            root = shell_quote(root),
            candidate = shell_quote(candidate),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let canonical = output.stdout.trim().to_owned();
    if canonical.is_empty() {
        return Err("remote artifact directory did not resolve".into());
    }
    Ok(canonical)
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

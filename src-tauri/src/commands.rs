use std::{
    collections::{HashMap, HashSet},
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
    audit::{RunEventKindV2, RunEventV2},
    domain::{
        AnalysisPlan, Artifact, ArtifactKind, AuthenticationMethod, ConnectionProfile, ProjectSpec,
        RunCheckpoint, RunEvent, RunState,
    },
    plan_v2::{
        AnalysisPlanV2, ApprovedPlan, ArtifactRecordV2, EnvironmentLock, PlanEnvironment,
        PlanningRequest, PlanningTurn, PolicyEnvelope, RepairProposalV2, RunCheckpointV2,
        RunStateV2, StepAttempt, StepSpecV2, action_hash, canonical_plan_hash, topological_steps,
    },
    project::{RemoteProjectLayout, require_remote_descendant, shell_quote},
    redaction::redact_secrets,
    tools::{ToolCatalog, ToolSummary, builtin_tool_catalog},
    validation::{PlanValidation, assess_repair, validate_plan_v2},
};
use omicsops_runner::{
    AutonomousRunner, LlmRepairPlanner, SshRemoteExecutor, SshV2Executor, StepOutcome,
    generate_analysis_plan,
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
    pub skills_root: PathBuf,
    pub research_last_request: Arc<tokio::sync::Mutex<HashMap<String, std::time::Instant>>>,
    pub active_kernels: crate::kernel_commands::ActiveKernelMap,
    pub project_kernel_queues: Arc<tokio::sync::Mutex<HashMap<Uuid, Arc<tokio::sync::Semaphore>>>>,
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
pub fn list_tools() -> Result<Vec<ToolSummary>, String> {
    builtin_tool_catalog()
        .map(|catalog| catalog.summaries())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn planning_turn(
    state: State<'_, AppState>,
    request: PlanningRequest,
) -> Result<PlanningTurn, String> {
    let catalog = builtin_tool_catalog().map_err(|error| error.to_string())?;
    let metadata = serde_json::json!({
        "goal": request.goal,
        "environment_summary": request.environment_summary,
        "answers": request.answers,
        "tools": catalog.summaries(),
    });
    let prompt = redact_secrets(&metadata.to_string(), &[] as &[&str]);
    llm_client(&state)?.call_tool(
        "Plan a bioinformatics workflow using only the supplied versioned tools. Ask concise clarification questions when inputs, design, organism, reference, or outputs are ambiguous. Never request or reproduce FASTQ reads, matrix rows, result-table rows, credentials, or other analysis data.",
        &prompt,
        "submit_planning_turn",
    ).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub fn validate_plan(plan: AnalysisPlanV2) -> Result<PlanValidation, String> {
    let catalog = builtin_tool_catalog().map_err(|error| error.to_string())?;
    Ok(validate_plan_v2(&plan, &catalog))
}

#[tauri::command]
pub fn approve_plan(
    state: State<'_, AppState>,
    mut plan: AnalysisPlanV2,
    envelope: PolicyEnvelope,
) -> Result<ApprovedPlan, String> {
    plan.policy = envelope.clone();
    let catalog = builtin_tool_catalog().map_err(|error| error.to_string())?;
    let validation = validate_plan_v2(&plan, &catalog);
    if !validation.valid {
        return Err(serde_json::to_string(&validation).map_err(|error| error.to_string())?);
    }
    let approved = ApprovedPlan {
        id: Uuid::new_v4(),
        plan_id: plan.id,
        plan_hash: canonical_plan_hash(&plan).map_err(|error| error.to_string())?,
        policy: envelope,
        approved_at: Utc::now(),
    };
    state
        .repository
        .put_json("analysis_plan_v2", &plan.id.to_string(), &plan)
        .map_err(|error| error.to_string())?;
    state
        .repository
        .save_approved_plan(&approved)
        .map_err(|error| error.to_string())?;
    Ok(approved)
}

#[tauri::command]
pub async fn start_run(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: Uuid,
    project_id: Uuid,
    approved_plan_id: Uuid,
) -> Result<Uuid, String> {
    let approved = state
        .repository
        .get_approved_plan(approved_plan_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "approved plan record does not exist".to_owned())?;
    let plan: AnalysisPlanV2 = state
        .repository
        .get_json("analysis_plan_v2", &approved.plan_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "frozen approved plan does not exist".to_owned())?;
    if canonical_plan_hash(&plan).map_err(|error| error.to_string())? != approved.plan_hash {
        return Err("approved plan hash no longer matches the frozen plan".into());
    }
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let project: ProjectSpec = state
        .repository
        .get_json("project", &project_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project does not exist".to_owned())?;
    let active = state
        .repository
        .list_json::<RunCheckpointV2>("run_checkpoint_v2")
        .map_err(|error| error.to_string())?
        .into_iter()
        .any(|run| {
            run.project_id == project_id
                && matches!(
                    run.state,
                    RunStateV2::Preparing | RunStateV2::Running | RunStateV2::PausedForApproval
                )
        });
    if active {
        return Err("this project already has an active run".into());
    }
    let run_id = Uuid::new_v4();
    let checkpoint = RunCheckpointV2::new(run_id, profile_id, project_id, approved_plan_id);
    state
        .repository
        .put_json("run_checkpoint_v2", &run_id.to_string(), &checkpoint)
        .map_err(|error| error.to_string())?;
    let event = RunEventV2::new(
        1,
        run_id,
        RunEventKindV2::RunStarted,
        "approved run created",
        None,
        std::collections::BTreeMap::from([
            ("plan_hash".into(), approved.plan_hash),
            ("approved_plan_id".into(), approved_plan_id.to_string()),
        ]),
    )
    .map_err(|error| error.to_string())?;
    state
        .repository
        .append_audit_event(&event)
        .map_err(|error| error.to_string())?;
    let authentication = authentication_for_profile(&state, &profile)?;
    let repair_llm = llm_client(&state)?;
    spawn_v2_run(
        app,
        state.repository.clone(),
        state.active_runs.clone(),
        profile,
        authentication,
        project,
        plan,
        checkpoint,
        false,
        repair_llm,
    )?;
    Ok(run_id)
}

#[tauri::command]
pub fn export_run_bundle(
    state: State<'_, AppState>,
    run_id: Uuid,
    local_path: String,
) -> Result<(), String> {
    let checkpoint: RunCheckpointV2 = state
        .repository
        .get_json("run_checkpoint_v2", &run_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "run does not exist".to_owned())?;
    let approved = state
        .repository
        .get_approved_plan(checkpoint.approved_plan_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "approved plan does not exist".to_owned())?;
    let plan: AnalysisPlanV2 = state
        .repository
        .get_json("analysis_plan_v2", &approved.plan_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "frozen plan does not exist".to_owned())?;
    let events = state
        .repository
        .audit_events_for_run(run_id)
        .map_err(|error| error.to_string())?;
    if events.iter().any(|event| !event.verify_hash())
        || events
            .windows(2)
            .any(|pair| pair[1].prev_hash.as_deref() != Some(pair[0].event_hash.as_str()))
    {
        return Err("audit event hash chain is invalid".into());
    }
    let attempts = state
        .repository
        .step_attempts_for_run(run_id)
        .map_err(|error| error.to_string())?;
    let environment_lock = state
        .repository
        .environment_lock_for_run(run_id)
        .map_err(|error| error.to_string())?;
    let artifacts = state
        .repository
        .list_json::<ArtifactRecordV2>("artifact_v2")
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|artifact| artifact.run_id == run_id)
        .collect::<Vec<_>>();
    let tools = builtin_tool_catalog()
        .map_err(|error| error.to_string())?
        .manifests();
    let bundle = serde_json::json!({
        "schema_version": 2,
        "exported_at": Utc::now(),
        "checkpoint": checkpoint,
        "approved_plan": approved,
        "plan": plan,
        "tool_manifests": tools,
        "step_attempts": attempts,
        "environment_lock": environment_lock,
        "artifacts": artifacts,
        "events": events,
    });
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let file = options
        .open(&local_path)
        .map_err(|error| format!("cannot create run bundle: {error}"))?;
    serde_json::to_writer_pretty(file, &bundle).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_runs_v2(state: State<'_, AppState>) -> Result<Vec<RunCheckpointV2>, String> {
    state
        .repository
        .list_json("run_checkpoint_v2")
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_run_events_v2(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<Vec<RunEventV2>, String> {
    state
        .repository
        .audit_events_for_run(run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_artifacts_v2(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<Vec<ArtifactRecordV2>, String> {
    Ok(state
        .repository
        .list_json::<ArtifactRecordV2>("artifact_v2")
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|artifact| artifact.run_id == run_id)
        .collect())
}

#[tauri::command]
pub fn list_step_attempts_v2(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<Vec<StepAttempt>, String> {
    state
        .repository
        .step_attempts_for_run(run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_environment_lock_v2(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<Option<EnvironmentLock>, String> {
    state
        .repository
        .environment_lock_for_run(run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn resume_run_v2(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<(), String> {
    let checkpoint: RunCheckpointV2 = state
        .repository
        .get_json("run_checkpoint_v2", &run_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "run does not exist".to_owned())?;
    if checkpoint.state == RunStateV2::NeedsAttention {
        return Err("run needs explicit attention and cannot be silently rerun".into());
    }
    if matches!(
        checkpoint.state,
        RunStateV2::Succeeded | RunStateV2::Canceled
    ) {
        return Err("run is already terminal".into());
    }
    let approved = state
        .repository
        .get_approved_plan(checkpoint.approved_plan_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "approval record is missing".to_owned())?;
    let plan: AnalysisPlanV2 = state
        .repository
        .get_json("analysis_plan_v2", &approved.plan_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "frozen plan is missing".to_owned())?;
    if canonical_plan_hash(&plan).map_err(|error| error.to_string())? != approved.plan_hash {
        return Err("frozen plan hash mismatch".into());
    }
    let profile = find_profile(&state.repository, checkpoint.profile_id)?;
    require_trusted_host(&profile)?;
    let project: ProjectSpec = state
        .repository
        .get_json("project", &checkpoint.project_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project is missing".to_owned())?;
    let authentication = authentication_for_profile(&state, &profile)?;
    let repair_llm = llm_client(&state)?;
    append_v2_event(
        &app,
        &state.repository,
        run_id,
        RunEventKindV2::RunResumed,
        "recovery requested; remote state will be reconciled",
        Default::default(),
    );
    spawn_v2_run(
        app,
        state.repository.clone(),
        state.active_runs.clone(),
        profile,
        authentication,
        project,
        plan,
        checkpoint,
        true,
        repair_llm,
    )
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
pub fn approve_legacy_plan(
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
pub async fn legacy_start_run(
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

    let legacy: Option<RunCheckpoint> = state
        .repository
        .get_json("run_checkpoint", &run_id.to_string())
        .map_err(|error| error.to_string())?;
    let Some(mut checkpoint) = legacy else {
        let mut checkpoint: RunCheckpointV2 = state
            .repository
            .get_json("run_checkpoint_v2", &run_id.to_string())
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("run {run_id} was not found"))?;
        if !matches!(
            checkpoint.state,
            RunStateV2::Succeeded | RunStateV2::Canceled
        ) {
            checkpoint.state = RunStateV2::Canceled;
            persist_v2_checkpoint(&state.repository, &checkpoint);
            append_v2_event(
                &app,
                &state.repository,
                run_id,
                RunEventKindV2::RunCanceled,
                "run canceled while no remote step was active",
                Default::default(),
            );
        }
        return Ok(());
    };
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
fn spawn_v2_run(
    app: AppHandle,
    repository: Repository,
    active_runs: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    profile: ConnectionProfile,
    authentication: SshAuthentication,
    project: ProjectSpec,
    plan: AnalysisPlanV2,
    checkpoint: RunCheckpointV2,
    recovering: bool,
    repair_llm: OpenAiCompatibleClient,
) -> Result<(), String> {
    let run_id = checkpoint.run_id;
    let cancel_requested = Arc::new(AtomicBool::new(false));
    {
        let mut active = active_runs
            .lock()
            .map_err(|_| "active run registry is unavailable".to_owned())?;
        if active.insert(run_id, cancel_requested.clone()).is_some() {
            return Err(format!("run {run_id} is already active"));
        }
    }
    tauri::async_runtime::spawn(async move {
        execute_v2_run(
            app,
            repository,
            profile,
            authentication,
            project,
            plan,
            checkpoint,
            cancel_requested,
            recovering,
            repair_llm,
        )
        .await;
        if let Ok(mut active) = active_runs.lock() {
            active.remove(&run_id);
        }
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_v2_run(
    app: AppHandle,
    repository: Repository,
    profile: ConnectionProfile,
    authentication: SshAuthentication,
    project: ProjectSpec,
    plan: AnalysisPlanV2,
    mut checkpoint: RunCheckpointV2,
    cancel_requested: Arc<AtomicBool>,
    mut recovering: bool,
    repair_llm: OpenAiCompatibleClient,
) {
    let session = match SshSession::connect(&profile, authentication).await {
        Ok(session) => Arc::new(session),
        Err(error) => {
            checkpoint.needs_attention(format!("SSH connection failed: {error}"));
            persist_v2_checkpoint(&repository, &checkpoint);
            append_v2_event(
                &app,
                &repository,
                checkpoint.run_id,
                RunEventKindV2::NeedsAttention,
                "SSH connection failed",
                Default::default(),
            );
            return;
        }
    };
    checkpoint.state = RunStateV2::Running;
    persist_v2_checkpoint(&repository, &checkpoint);
    let catalog = match builtin_tool_catalog() {
        Ok(catalog) => catalog,
        Err(error) => {
            checkpoint.needs_attention(error.to_string());
            persist_v2_checkpoint(&repository, &checkpoint);
            return;
        }
    };
    let executor = SshV2Executor::new(
        session.clone(),
        &project.remote_root,
        catalog.clone(),
        cancel_requested.clone(),
    );
    let steps = match topological_steps(&plan) {
        Ok(steps) => steps,
        Err(error) => {
            checkpoint.needs_attention(error.to_string());
            persist_v2_checkpoint(&repository, &checkpoint);
            return;
        }
    };
    for step in steps {
        if checkpoint.completed_steps.contains(&step.id) {
            let Some(expected_hash) = checkpoint.action_hashes.get(&step.id) else {
                checkpoint.needs_attention(format!(
                    "checkpoint is missing the action hash for {}",
                    step.id
                ));
                persist_v2_checkpoint(&repository, &checkpoint);
                return;
            };
            let completed_attempt = repository
                .step_attempts_for_run(checkpoint.run_id)
                .unwrap_or_default()
                .into_iter()
                .filter(|attempt| {
                    attempt.step_id == step.id && attempt.action_hash == *expected_hash
                })
                .map(|attempt| attempt.attempt)
                .max()
                .unwrap_or(0);
            if let Err(reason) = executor
                .verify_completed(step, completed_attempt, expected_hash)
                .await
            {
                checkpoint.needs_attention(reason.clone());
                persist_v2_checkpoint(&repository, &checkpoint);
                append_v2_event(
                    &app,
                    &repository,
                    checkpoint.run_id,
                    RunEventKindV2::NeedsAttention,
                    &reason,
                    std::collections::BTreeMap::from([("step_id".into(), step.id.clone())]),
                );
                return;
            }
            continue;
        }
        if cancel_requested.load(Ordering::SeqCst) {
            checkpoint.state = RunStateV2::Canceled;
            persist_v2_checkpoint(&repository, &checkpoint);
            append_v2_event(
                &app,
                &repository,
                checkpoint.run_id,
                RunEventKindV2::RunCanceled,
                "run canceled",
                Default::default(),
            );
            return;
        }
        let (executed_step, hash) = match execute_v2_step_with_repairs(
            &app,
            &repository,
            &executor,
            &repair_llm,
            &plan,
            &catalog,
            checkpoint.run_id,
            step,
            &project.remote_root,
            recovering,
        )
        .await
        {
            Ok(success) => success,
            Err(reason) => {
                checkpoint.needs_attention(reason.clone());
                persist_v2_checkpoint(&repository, &checkpoint);
                append_v2_event(
                    &app,
                    &repository,
                    checkpoint.run_id,
                    RunEventKindV2::NeedsAttention,
                    &reason,
                    std::collections::BTreeMap::from([("step_id".into(), step.id.clone())]),
                );
                return;
            }
        };
        recovering = false;
        for relative_path in &executed_step.expected_artifacts {
            let (size_bytes, sha256) = match executor.artifact_metadata(relative_path).await {
                Ok(metadata) => metadata,
                Err(reason) => {
                    checkpoint.needs_attention(format!(
                        "artifact metadata failed for {relative_path}: {reason}"
                    ));
                    persist_v2_checkpoint(&repository, &checkpoint);
                    return;
                }
            };
            let artifact = ArtifactRecordV2 {
                run_id: checkpoint.run_id,
                source_step_id: executed_step.id.clone(),
                remote_path: format!(
                    "{}/{}",
                    project.remote_root.trim_end_matches('/'),
                    relative_path.trim_start_matches('/')
                ),
                size_bytes,
                sha256,
                verified: true,
            };
            let artifact_id = format!("{}:{}", checkpoint.run_id, relative_path);
            let _ = repository.put_json("artifact_v2", &artifact_id, &artifact);
        }
        checkpoint.mark_verified(&executed_step.id, hash);
        persist_v2_checkpoint(&repository, &checkpoint);
        append_v2_event(
            &app,
            &repository,
            checkpoint.run_id,
            RunEventKindV2::StepSucceeded,
            "all verifications passed",
            std::collections::BTreeMap::from([("step_id".into(), executed_step.id.clone())]),
        );
    }
    let lock_path = format!(
        "{}/.omicsops/environment.lock",
        project.remote_root.trim_end_matches('/')
    );
    let lock_tmp = format!("{lock_path}.tmp");
    let lock_output = session.execute_checked(&format!(
        "micromamba list --explicit > {tmp} && mv {tmp} {lock} && stat -c '%s' {lock} && sha256sum {lock}",
        tmp = shell_quote(&lock_tmp), lock = shell_quote(&lock_path),
    )).await;
    let lock_output = match lock_output {
        Ok(output) => output,
        Err(error) => {
            checkpoint.needs_attention(format!("environment lock capture failed: {error}"));
            persist_v2_checkpoint(&repository, &checkpoint);
            append_v2_event(
                &app,
                &repository,
                checkpoint.run_id,
                RunEventKindV2::NeedsAttention,
                "environment lock capture failed",
                Default::default(),
            );
            return;
        }
    };
    let declared_dependencies = match &plan.environment {
        PlanEnvironment::Micromamba { dependencies, .. } => dependencies.clone(),
    };
    let mut lock_lines = lock_output.stdout.lines();
    let lock_size = lock_lines
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    let lock_sha = lock_lines
        .next()
        .and_then(|value| value.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    let lock = EnvironmentLock {
        run_id: checkpoint.run_id,
        backend: "micromamba".into(),
        remote_path: lock_path.clone(),
        sha256: lock_sha.clone(),
        declared_dependencies,
    };
    let _ = repository.save_environment_lock(&lock);
    let lock_artifact = ArtifactRecordV2 {
        run_id: checkpoint.run_id,
        source_step_id: "environment-lock".into(),
        remote_path: lock_path,
        size_bytes: lock_size,
        sha256: lock_sha,
        verified: true,
    };
    let _ = repository.put_json(
        "artifact_v2",
        &format!("{}:environment-lock", checkpoint.run_id),
        &lock_artifact,
    );
    checkpoint.state = RunStateV2::Succeeded;
    persist_v2_checkpoint(&repository, &checkpoint);
    append_v2_event(
        &app,
        &repository,
        checkpoint.run_id,
        RunEventKindV2::RunSucceeded,
        "all steps verified",
        Default::default(),
    );
    drop(executor);
    if let Ok(session) = Arc::try_unwrap(session) {
        let _ = session.disconnect().await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_v2_step_with_repairs(
    app: &AppHandle,
    repository: &Repository,
    executor: &SshV2Executor,
    repair_llm: &OpenAiCompatibleClient,
    plan: &AnalysisPlanV2,
    catalog: &ToolCatalog,
    run_id: Uuid,
    approved_step: &StepSpecV2,
    project_root: &str,
    recovering: bool,
) -> Result<(StepSpecV2, String), String> {
    let mut current = approved_step.clone();
    let mut action_hashes = HashSet::new();
    let mut strategies = HashSet::new();
    for attempt_number in 0_u8..=3 {
        let hash = action_hash(&current.action).map_err(|error| error.to_string())?;
        if !action_hashes.insert(hash.clone()) {
            return Err("repair repeated an already attempted action".into());
        }
        append_v2_event(
            app,
            repository,
            run_id,
            RunEventKindV2::StepStarted,
            &current.title,
            std::collections::BTreeMap::from([
                ("step_id".into(), current.id.clone()),
                ("attempt".into(), attempt_number.to_string()),
                ("action_hash".into(), hash.clone()),
            ]),
        );
        let started_at = Utc::now();
        let result = if recovering && attempt_number == 0 {
            executor.recover(&current, attempt_number, &hash).await
        } else {
            executor.execute(&current, attempt_number).await
        }
        .map_err(|error| redact_secrets(&error, &[] as &[&str]))?;
        let attempt = StepAttempt {
            run_id,
            step_id: current.id.clone(),
            attempt: attempt_number,
            action_hash: hash.clone(),
            process_group_id: result.process_group_id,
            started_at,
            finished_at: Some(Utc::now()),
            exit_code: Some(result.exit_status),
            log_path: format!("{project_root}/logs/{}.{attempt_number}.log", current.id),
            manifest_path: format!(
                "{project_root}/.omicsops/state/{}.{attempt_number}.manifest.json",
                current.id
            ),
            verifications: result
                .verifications
                .into_iter()
                .map(|mut verification| {
                    verification.observed = redact_secrets(&verification.observed, &[] as &[&str]);
                    verification
                })
                .collect(),
        };
        repository
            .save_step_attempt(&attempt)
            .map_err(|error| error.to_string())?;
        let passed = attempt.exit_code == Some(0)
            && attempt
                .verifications
                .iter()
                .all(|verification| verification.passed);
        if passed {
            return Ok((current, hash));
        }
        append_v2_event(
            app,
            repository,
            run_id,
            RunEventKindV2::VerificationFailed,
            "step did not pass all verifications",
            std::collections::BTreeMap::from([
                ("step_id".into(), current.id.clone()),
                ("attempt".into(), attempt_number.to_string()),
            ]),
        );
        if attempt_number == 3 {
            return Err("three distinct repair strategies were exhausted".into());
        }
        let error_tail = attempt
            .verifications
            .iter()
            .filter(|verification| !verification.passed)
            .map(|verification| verification.observed.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = serde_json::json!({
            "tool_action": current.action,
            "resources": current.resources,
            "exit_code": attempt.exit_code,
            "redacted_error_tail": error_tail.chars().rev().take(4000).collect::<String>().chars().rev().collect::<String>(),
            "previous_strategies": strategies,
            "allowed_tools": plan.policy.allowed_tools,
            "allowed_domains": plan.policy.allowed_domains,
        });
        let proposal: RepairProposalV2 = repair_llm.call_tool(
            "Diagnose this failed bioinformatics tool action. Propose one distinct replacement action. Do not change output paths, verifications, risk, resources, tools, versions, or domains beyond the supplied approval envelope. Return metadata and the redacted error context only.",
            &redact_secrets(&prompt.to_string(), &[] as &[&str]),
            "submit_v2_repair",
        ).await.map_err(|error| error.to_string())?;
        if !strategies.insert(proposal.strategy.clone()) {
            return Err("repair repeated an already attempted strategy".into());
        }
        let mut proposed = current.clone();
        proposed.action = proposal.replacement_action;
        proposed.rationale = format!("{} Repair: {}", proposed.rationale, proposal.diagnosis);
        let assessment = assess_repair(plan, approved_step, &proposed, catalog);
        if !assessment.within_envelope {
            append_v2_event(
                app,
                repository,
                run_id,
                RunEventKindV2::ApprovalRequired,
                "repair exceeds the approved policy envelope",
                std::collections::BTreeMap::from([
                    ("step_id".into(), current.id.clone()),
                    (
                        "diffs".into(),
                        serde_json::to_string(&assessment.diffs).unwrap_or_default(),
                    ),
                    (
                        "validation".into(),
                        serde_json::to_string(&assessment.validation).unwrap_or_default(),
                    ),
                ]),
            );
            return Err("repair exceeds the approved policy envelope and requires approval".into());
        }
        current = proposed;
    }
    Err("repair loop ended unexpectedly".into())
}

fn persist_v2_checkpoint(repository: &Repository, checkpoint: &RunCheckpointV2) {
    let _ = repository.put_json(
        "run_checkpoint_v2",
        &checkpoint.run_id.to_string(),
        checkpoint,
    );
}

fn append_v2_event(
    app: &AppHandle,
    repository: &Repository,
    run_id: Uuid,
    kind: RunEventKindV2,
    message: &str,
    details: std::collections::BTreeMap<String, String>,
) {
    let existing = repository.audit_events_for_run(run_id).unwrap_or_default();
    let event = RunEventV2::new(
        existing.last().map_or(1, |event| event.sequence + 1),
        run_id,
        kind,
        message,
        existing.last().map(|event| event.event_hash.clone()),
        details,
    );
    if let Ok(event) = event {
        if repository.append_audit_event(&event).is_ok() {
            let _ = app.emit("run-event-v2", event);
        }
    }
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

pub(crate) fn find_profile(repository: &Repository, id: Uuid) -> Result<ConnectionProfile, String> {
    repository
        .list_connections()
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

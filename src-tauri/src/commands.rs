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
    llm::{OpenAiCompatibleClient, ProviderProtocol, UnifiedModelClient},
    persistence::Repository,
    ssh::{SshAuthentication, SshSession},
};
use omicsops_agent::{ModelMessage, ModelRequest, ToolArgumentBuffer};
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
use schemars::JsonSchema;
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
    pub authenticated: bool,
    pub latency_ms: u128,
    pub server_os: Option<String>,
    pub remote_username: Option<String>,
    pub home: Option<String>,
    pub sftp_available: bool,
    pub python_available: bool,
    pub r_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRunStreamEvent {
    pub run_id: Uuid,
    pub sequence: u64,
    pub timestamp: chrono::DateTime<Utc>,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub iteration: Option<u64>,
}

fn emit_agent_run_stream(
    app: &AppHandle,
    run_id: Uuid,
    sequence: &mut u64,
    kind: &str,
    title: &str,
    content: impl Into<String>,
    iteration: Option<u64>,
) {
    *sequence += 1;
    let _ = app.emit(
        "agent-run-event",
        AgentRunStreamEvent {
            run_id,
            sequence: *sequence,
            timestamp: Utc::now(),
            kind: kind.into(),
            title: title.into(),
            content: content.into(),
            iteration,
        },
    );
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
    mut profile: ConnectionProfile,
    secret: String,
) -> Result<(), String> {
    let previous = state
        .repository
        .list_connections()
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
    let profile = find_profile(&state.repository, profile_id)?;
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
    let diagnostics = session
        .execute_checked("printf 'os='; uname -srm; printf 'user='; id -un; printf 'home=%s\\n' \"$HOME\"; command -v python3 >/dev/null && printf 'python=1\\n' || printf 'python=0\\n'; command -v Rscript >/dev/null && printf 'r=1\\n' || printf 'r=0\\n'")
        .await
        .map_err(|error| error.to_string())?;
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
    let mut profile = find_profile(&state.repository, profile_id)?;
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
    let project = current_project_spec(&state.repository, project_id)?;
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
    let agent_model = if plan.metadata.get("execution_mode").map(String::as_str)
        == Some("remote_agent")
    {
        let model_profile_id = plan
            .metadata
            .get("model_profile_id")
            .ok_or_else(|| "approved remote-agent plan has no model profile".to_string())?
            .parse::<Uuid>()
            .map_err(|_| "approved remote-agent plan has an invalid model profile".to_string())?;
        Some(unified_model_client(&state, model_profile_id)?)
    } else {
        None
    };
    let repair_llm = if agent_model.is_some() {
        None
    } else {
        Some(llm_client(&state)?)
    };
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
        agent_model,
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
    let project = current_project_spec(&state.repository, checkpoint.project_id)?;
    let authentication = authentication_for_profile(&state, &profile)?;
    let agent_model =
        if plan.metadata.get("execution_mode").map(String::as_str) == Some("remote_agent") {
            let id = plan
                .metadata
                .get("model_profile_id")
                .ok_or_else(|| "remote-agent plan has no model profile".to_string())?
                .parse::<Uuid>()
                .map_err(|_| "remote-agent plan has an invalid model profile".to_string())?;
            Some(unified_model_client(&state, id)?)
        } else {
            None
        };
    let repair_llm = if agent_model.is_some() {
        None
    } else {
        Some(llm_client(&state)?)
    };
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
        agent_model,
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
    repair_llm: Option<OpenAiCompatibleClient>,
    agent_model: Option<UnifiedModelClient>,
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
            agent_model,
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
    repair_llm: Option<OpenAiCompatibleClient>,
    agent_model: Option<UnifiedModelClient>,
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
    if plan.metadata.get("execution_mode").map(String::as_str) == Some("remote_agent") {
        let Some(model) = agent_model.as_ref() else {
            checkpoint.needs_attention("remote-agent model is unavailable");
            persist_v2_checkpoint(&repository, &checkpoint);
            return;
        };
        match execute_remote_agent_task(
            &app,
            &repository,
            &session,
            model,
            &project,
            &plan,
            checkpoint.run_id,
            &cancel_requested,
        )
        .await
        {
            Ok(()) => {
                checkpoint.mark_verified("remote-agent-task", "adaptive-agent-loop");
                checkpoint.state = RunStateV2::Succeeded;
                persist_v2_checkpoint(&repository, &checkpoint);
                append_v2_event(
                    &app,
                    &repository,
                    checkpoint.run_id,
                    RunEventKindV2::RunSucceeded,
                    "remote agent completed the approved goal",
                    Default::default(),
                );
            }
            Err(reason) => {
                let mut sequence = u64::MAX - 1;
                emit_agent_run_stream(
                    &app,
                    checkpoint.run_id,
                    &mut sequence,
                    "agent_failed",
                    "Remote agent needs attention",
                    reason.clone(),
                    None,
                );
                checkpoint.needs_attention(reason.clone());
                persist_v2_checkpoint(&repository, &checkpoint);
                append_v2_event(
                    &app,
                    &repository,
                    checkpoint.run_id,
                    RunEventKindV2::NeedsAttention,
                    &reason,
                    Default::default(),
                );
            }
        }
        return;
    }
    if let Err(reason) = prepare_v2_environment(&session, &project.remote_root, &plan).await {
        checkpoint.needs_attention(reason.clone());
        persist_v2_checkpoint(&repository, &checkpoint);
        append_v2_event(
            &app,
            &repository,
            checkpoint.run_id,
            RunEventKindV2::NeedsAttention,
            &reason,
            Default::default(),
        );
        return;
    }
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
        let Some(repair_llm) = repair_llm.as_ref() else {
            checkpoint.needs_attention("fixed-tool run has no repair model");
            persist_v2_checkpoint(&repository, &checkpoint);
            return;
        };
        let (executed_step, hash) = match execute_v2_step_with_repairs(
            &app,
            &repository,
            &executor,
            repair_llm,
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
    let environment_path = format!(
        "{}/.omicsops/env",
        project.remote_root.trim_end_matches('/')
    );
    let lock_output = session.execute_checked(&format!(
        "micromamba list --prefix {environment} --explicit > {tmp} && mv {tmp} {lock} && stat -c '%s' {lock} && sha256sum {lock}",
        environment = shell_quote(&environment_path), tmp = shell_quote(&lock_tmp), lock = shell_quote(&lock_path),
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

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct RemoteAgentAction {
    /// One of `run` or `finish`.
    kind: String,
    /// Shell command for a `run` action. It executes from the approved project root.
    command: Option<String>,
    /// Why this action is the next safe step, or the final result summary.
    reason: String,
    /// Relative project paths to completed result artifacts. Used by `finish`.
    #[serde(default)]
    artifacts: Vec<String>,
}

pub fn validate_agent_command(command: &str, project_root: &str) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() || command.len() > 32_000 || command.contains('\0') {
        return Err("agent submitted an empty or oversized terminal command".into());
    }
    let lowered = command.to_ascii_lowercase();
    let denied = [
        "sudo ",
        "su -",
        "rm -rf",
        "rm -fr",
        "mkfs",
        "shutdown",
        "reboot",
        "poweroff",
        ":(){",
        "dd if=",
        "chmod -r",
        "chown -r",
        "git reset --hard",
        "git clean -f",
        "> /dev/",
        "$home",
        "${home}",
        "cd /",
        "cd ~",
        "../",
    ];
    if denied.iter().any(|token| lowered.contains(token)) {
        return Err(
            "agent terminal action was blocked by the approved remote safety policy".into(),
        );
    }
    for protected in [
        "/etc/", "/usr/", "/var/", "/root/", "/boot/", "/sys/", "/proc/",
    ] {
        if lowered.contains(protected) && !project_root.to_ascii_lowercase().starts_with(protected)
        {
            return Err(format!(
                "agent terminal action referenced protected path {protected}"
            ));
        }
    }
    for token in command.split(|character: char| {
        character.is_whitespace()
            || matches!(
                character,
                '\'' | '"' | '=' | '(' | ')' | ';' | '|' | '<' | '>'
            )
    }) {
        let candidate = token.trim_matches(|character: char| matches!(character, ',' | ':'));
        if candidate.starts_with('/')
            && !candidate.starts_with(project_root)
            && !matches!(
                candidate,
                "/dev/null" | "/usr/bin/env" | "/bin/bash" | "/bin/sh"
            )
        {
            return Err(format!(
                "agent terminal action referenced a path outside the approved project root: {candidate}"
            ));
        }
    }
    Ok(())
}

async fn next_remote_agent_action(
    model: &UnifiedModelClient,
    system: &str,
    prompt: &str,
) -> Result<RemoteAgentAction, String> {
    let schema = serde_json::to_value(schemars::schema_for!(RemoteAgentAction))
        .map_err(|error| error.to_string())?;
    let mut buffer = ToolArgumentBuffer::new("submit_remote_agent_action");
    let mut event_error = None;
    model
        .stream_with(
            ModelRequest {
                system: system.into(),
                messages: vec![ModelMessage {
                    role: "user".into(),
                    content: prompt.into(),
                }],
                tool_name: Some("submit_remote_agent_action".into()),
                tool_schema: Some(schema),
            },
            |event| {
                if let Err(error) = buffer.push(event) {
                    event_error.get_or_insert_with(|| error.to_string());
                }
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    if let Some(error) = event_error {
        return Err(error);
    }
    serde_json::from_value(buffer.finish().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
async fn execute_remote_agent_task(
    app: &AppHandle,
    repository: &Repository,
    session: &Arc<SshSession>,
    model: &UnifiedModelClient,
    project: &ProjectSpec,
    plan: &AnalysisPlanV2,
    run_id: Uuid,
    cancel_requested: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut stream_sequence = 0_u64;
    let arguments = plan
        .stages
        .iter()
        .flat_map(|stage| &stage.steps)
        .find_map(|step| match &step.action {
            omicsops_core::plan_v2::StepAction::Tool {
                tool_id, arguments, ..
            } if tool_id == "agent.remote_task" => Some(arguments),
            _ => None,
        })
        .ok_or_else(|| "approved plan has no remote-agent task".to_string())?;
    let goal = arguments
        .get("goal")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&plan.summary);
    let observation = arguments
        .get("remote_observation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let skills = arguments
        .get("skill_context")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let criteria = arguments
        .get("completion_criteria")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let max_iterations = arguments
        .get("max_iterations")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(24)
        .clamp(1, 48);
    let system = format!(
        "You are the approved OmicsOps remote terminal agent. Work adaptively toward the approved goal. \
         Submit exactly one action per turn. Use kind=run with one shell command, observe its real output on the next turn, \
         and correct course. Use kind=finish only after verifying the completion criteria; list only existing relative artifact paths. \
         Work inside {}. Never use sudo, alter system directories, delete recursively, or claim results not observed. \
         Prefer scripts and environments stored under the project root. Enabled Skills are instructions, not evidence. \
         MCP runtime status for this plan: {}.",
        project.remote_root,
        plan.metadata
            .get("mcp_runtime")
            .map(String::as_str)
            .unwrap_or("not_configured")
    );
    let mut transcript = format!(
        "APPROVED GOAL:\n{goal}\n\nCOMPLETION CRITERIA:\n{criteria}\n\nREAD-ONLY REMOTE OBSERVATION:\n{observation}\n\nENABLED SKILLS:\n{skills}"
    );
    emit_agent_run_stream(
        app,
        run_id,
        &mut stream_sequence,
        "agent_started",
        "Remote agent connected",
        format!(
            "Approved project root: {}\nThe agent will now choose and execute one audited terminal action at a time.",
            project.remote_root
        ),
        None,
    );
    let mut ran_command = false;
    for iteration in 1..=max_iterations {
        if cancel_requested.load(Ordering::SeqCst) {
            return Err("remote agent run was canceled".into());
        }
        emit_agent_run_stream(
            app,
            run_id,
            &mut stream_sequence,
            "model_started",
            "Agent is choosing the next action",
            format!(
                "Iteration {iteration}: evaluating the latest remote observations and completion criteria."
            ),
            Some(iteration),
        );
        let prompt: String = transcript
            .chars()
            .rev()
            .take(70_000)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let action = next_remote_agent_action(model, &system, &prompt).await?;
        emit_agent_run_stream(
            app,
            run_id,
            &mut stream_sequence,
            "model_action",
            "Agent action selected",
            action.reason.clone(),
            Some(iteration),
        );
        match action.kind.as_str() {
            "run" => {
                let command = action
                    .command
                    .as_deref()
                    .ok_or_else(|| "agent run action omitted command".to_string())?;
                validate_agent_command(command, &project.remote_root)?;
                emit_agent_run_stream(
                    app,
                    run_id,
                    &mut stream_sequence,
                    "tool_started",
                    "SSH command",
                    redact_secrets(command, &[] as &[&str]),
                    Some(iteration),
                );
                append_v2_event(
                    app,
                    repository,
                    run_id,
                    RunEventKindV2::StepStarted,
                    &format!("agent terminal action {iteration}"),
                    std::collections::BTreeMap::from([
                        ("iteration".into(), iteration.to_string()),
                        ("reason".into(), action.reason.clone()),
                        ("command".into(), redact_secrets(command, &[] as &[&str])),
                    ]),
                );
                let wrapped = format!("cd -- {} && {}", shell_quote(&project.remote_root), command);
                let app_for_output = app.clone();
                let output_run_id = run_id;
                let output_iteration = iteration;
                let output_sequence = std::sync::Arc::new(std::sync::Mutex::new(stream_sequence));
                let callback_sequence = output_sequence.clone();
                let output = session
                    .execute_streaming(&wrapped, move |stderr, chunk| {
                        let content = redact_secrets(chunk, &[] as &[&str]);
                        if content.is_empty() {
                            return;
                        }
                        if let Ok(mut sequence) = callback_sequence.lock() {
                            emit_agent_run_stream(
                                &app_for_output,
                                output_run_id,
                                &mut sequence,
                                if stderr { "stderr" } else { "stdout" },
                                if stderr { "stderr" } else { "stdout" },
                                content,
                                Some(output_iteration),
                            );
                        }
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                stream_sequence = output_sequence
                    .lock()
                    .map(|value| *value)
                    .unwrap_or(stream_sequence);
                ran_command = true;
                let stdout: String = output
                    .stdout
                    .chars()
                    .rev()
                    .take(20_000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                let stderr: String = output
                    .stderr
                    .chars()
                    .rev()
                    .take(20_000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                transcript.push_str(&format!("\n\nACTION {iteration}\nReason: {}\nCommand: {}\nExit: {}\nSTDOUT:\n{}\nSTDERR:\n{}",
                    action.reason, command, output.status, stdout, stderr));
                append_v2_event(
                    app,
                    repository,
                    run_id,
                    if output.status == 0 {
                        RunEventKindV2::StepSucceeded
                    } else {
                        RunEventKindV2::StepFailed
                    },
                    &format!("agent terminal action {iteration} exited {}", output.status),
                    std::collections::BTreeMap::from([
                        ("iteration".into(), iteration.to_string()),
                        ("exit_code".into(), output.status.to_string()),
                        (
                            "stdout_tail".into(),
                            redact_secrets(&stdout, &[] as &[&str]),
                        ),
                        (
                            "stderr_tail".into(),
                            redact_secrets(&stderr, &[] as &[&str]),
                        ),
                    ]),
                );
                emit_agent_run_stream(
                    app,
                    run_id,
                    &mut stream_sequence,
                    "tool_completed",
                    "SSH command completed",
                    format!("Exit code: {}", output.status),
                    Some(iteration),
                );
            }
            "finish" => {
                if !ran_command {
                    return Err(
                        "agent attempted to finish without executing or verifying the remote task"
                            .into(),
                    );
                }
                if action.artifacts.is_empty() {
                    return Err(
                        "agent attempted to finish without declaring result artifacts".into(),
                    );
                }
                for relative in &action.artifacts {
                    omicsops_core::project::validate_relative_remote_path(std::path::Path::new(
                        relative,
                    ))
                    .map_err(|error| error.to_string())?;
                    let remote_path = format!(
                        "{}/{}",
                        project.remote_root.trim_end_matches('/'),
                        relative.trim_start_matches('/')
                    );
                    let metadata = session
                        .execute_checked(&format!(
                            "test -f {0} && stat -c '%s' {0} && sha256sum {0}",
                            shell_quote(&remote_path)
                        ))
                        .await
                        .map_err(|error| {
                            format!("declared artifact {relative} is not a verified file: {error}")
                        })?;
                    let mut lines = metadata.stdout.lines();
                    let size_bytes = lines
                        .next()
                        .and_then(|value| value.parse().ok())
                        .ok_or_else(|| format!("artifact {relative} has no size"))?;
                    let sha256 = lines
                        .next()
                        .and_then(|value| value.split_whitespace().next())
                        .ok_or_else(|| format!("artifact {relative} has no hash"))?
                        .to_string();
                    let record = ArtifactRecordV2 {
                        run_id,
                        source_step_id: "remote-agent-task".into(),
                        remote_path,
                        size_bytes,
                        sha256,
                        verified: true,
                    };
                    repository
                        .put_json("artifact_v2", &format!("{run_id}:{relative}"), &record)
                        .map_err(|error| error.to_string())?;
                }
                append_v2_event(
                    app,
                    repository,
                    run_id,
                    RunEventKindV2::StepSucceeded,
                    &action.reason,
                    std::collections::BTreeMap::from([(
                        "artifacts".into(),
                        action.artifacts.join("\n"),
                    )]),
                );
                emit_agent_run_stream(
                    app,
                    run_id,
                    &mut stream_sequence,
                    "agent_completed",
                    "Remote agent completed",
                    format!(
                        "{}\n\nVerified artifacts:\n{}",
                        action.reason,
                        action.artifacts.join("\n")
                    ),
                    Some(iteration),
                );
                return Ok(());
            }
            other => return Err(format!("agent submitted unsupported action kind {other}")),
        }
    }
    Err(format!(
        "remote agent exhausted its approved limit of {max_iterations} terminal actions"
    ))
}

async fn prepare_v2_environment(
    session: &SshSession,
    project_root: &str,
    plan: &AnalysisPlanV2,
) -> Result<(), String> {
    let (channels, dependencies) = match &plan.environment {
        PlanEnvironment::Micromamba {
            channels,
            dependencies,
        } => (channels, dependencies),
    };
    if dependencies.is_empty() {
        return Err("approved environment has no dependencies".into());
    }
    let environment = format!("{}/.omicsops/env", project_root.trim_end_matches('/'));
    let channel_arguments = channels
        .iter()
        .map(|channel| format!("--channel {}", shell_quote(channel)))
        .collect::<Vec<_>>()
        .join(" ");
    let dependency_arguments = dependencies
        .iter()
        .map(|dependency| shell_quote(dependency))
        .collect::<Vec<_>>()
        .join(" ");
    let base = format!(
        "micromamba {{operation}} --yes --prefix {} {} {}",
        shell_quote(&environment),
        channel_arguments,
        dependency_arguments
    );
    let create = base.replace("{operation}", "create");
    let install = base.replace("{operation}", "install");
    session
        .execute_checked(&format!(
            "mkdir -p {} && if test -x {}/bin/python; then {}; else {}; fi",
            shell_quote(&format!("{}/.omicsops", project_root.trim_end_matches('/'))),
            shell_quote(&environment),
            install,
            create
        ))
        .await
        .map_err(|error| format!("environment preparation failed: {error}"))?;
    Ok(())
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

fn unified_model_client(
    state: &State<'_, AppState>,
    model_profile_id: Uuid,
) -> Result<UnifiedModelClient, String> {
    let profile = state
        .repository
        .get_model_profile(model_profile_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "model profile not found".to_string())?;
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
        profile.model,
        credential,
    )
    .map_err(|error| error.to_string())
}

fn current_project_spec(repository: &Repository, project_id: Uuid) -> Result<ProjectSpec, String> {
    let project = repository
        .get_project(project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project does not exist".to_string())?;
    Ok(ProjectSpec {
        id: project.id,
        connection_id: project
            .connection_id
            .ok_or_else(|| "project has no remote connection".to_string())?,
        remote_root: project
            .remote_root
            .ok_or_else(|| "project has no remote root".to_string())?,
        plan_summary: project.description,
        data_sources: Vec::new(),
        resource_limits: omicsops_core::domain::ResourceLimits::default(),
        allowed_network_domains: Vec::new(),
    })
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

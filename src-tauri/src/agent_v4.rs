use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use base64::Engine as _;
use chrono::Utc;
use omicsops_adapters::{
    credentials::SystemCredentialVault,
    kernel::{kernel_driver, validate_capture_paths, validate_kernel_code},
    llm::{RequestBudget, UnifiedModelClient},
    ssh::{SshJsonlProcess, SshSession},
};
use omicsops_agent::provider::{
    ProviderRequest, ProviderStreamEvent, ProviderToolCallAccumulator, ProviderToolSpec,
};
use omicsops_agent::{
    KernelEvent, KernelEventDecoder, KernelEventKind, KernelLanguage, KernelRequest,
};
use omicsops_agent_core::{
    AgentCoreErrorV4, AgentCoreV4, AgentLimitsV4, EventStoreV4, ModelImageRefV4, ModelPortV4,
    ModelRequestV4, ModelStreamEventV4, ModelTurnV4, PlanApprovalScopeV4, PlanToolAuthorizationV4,
    PromptLayersV4, ReviewerRequestV4, ScientificStateStoreV4, ScientificUpdateV4, ToolPortV4,
};
use omicsops_core::{
    project::{require_remote_descendant, shell_quote},
    workspace::Project,
};
pub use omicsops_dto::{
    AgentV4RequestPlanRevisionRequest, PlanRevisionStatusV4, ProposedPlanRevisionV4,
    RequestPlanRevisionResponseV4,
};
use omicsops_knowledge::{
    KnowledgeErrorV4, McpToolIndexV4, MemoryDocumentV4, SkillDocumentV4,
    authorize_mcp_read_only_target, authorize_mcp_use, freeze_skill, markdown_sections,
    schema_digest, search_mcp_tools, search_memory, search_skills,
};
use omicsops_mcp::McpSessionManager;
use omicsops_process::background_command;
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, AgentRequestRouteV4, ApprovalPolicyV4, AutonomyModeV4,
    BrowserApprovalBindingV4, BrowserApprovalScopeV4, BrowserAuthorizationV4, BrowserSessionKindV4,
    BrowserTabSummaryV4, ComputeBackendDescriptorV4, ComputeBackendKindV4, ComputeSelectionV4,
    ContextArchiveV4, ContextCheckpointV4, ExecutionContextKeyV4, ExecutionPlanV4,
    IsolationStrengthV4, KernelLanguageV4, ModelErrorClassV4, ModelFailureV4, NetworkPolicyV4,
    OutputCaptureV4, ReviewerReportV4, RunExecutionKindV4, RunSpecV4, RuntimeArtifactV4,
    RuntimeResultV4, ToolApprovalDecisionV4, ToolCallV4, ToolDescriptorV4, ToolEffectV4,
    ToolOutcomeV4, UncertainResolutionV4,
};
use omicsops_runtime::{
    ContainerKernelBackendV4, KernelBackendV4, KernelProcessV4, LocalKernelBackendV4,
    RuntimeManagerV4,
};
use omicsops_science::{
    AnalysisDeclarationV4, AnalysisStatusV4, DatasetStageV4, EvidenceDeclarationV4,
    RuntimeIdentityV4, ScientificStateV4, VerifiedArtifactFactV4, VerifiedDatasetFactV4,
};
use omicsops_store::{
    PlanApprovalResultV4, PlanCancellationResultV4, PlanRevisionFinalizeOptionsV4, Store,
};
use omicsops_tools::{ToolExecutorV4, ToolRegistryV4, builtin_tool_definitions_v4};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::commands::{
    AppState, authentication_for_profile, find_profile, require_trusted_host, unified_model_client,
};
pub use crate::dto::{ConversationAgentStateV4, RunSummaryV4, SessionAgentModeV4};
use crate::p1_commands::{
    McpServerProfile, MemorySearchRequest, invoke_configured_mcp_tool_v4, memory_facts,
};

static PROJECT_SIDE_EFFECT_LOCKS_V4: OnceLock<std::sync::Mutex<HashMap<Uuid, Weak<Mutex<()>>>>> =
    OnceLock::new();
static SCIENTIFIC_STATE_LOCKS_V4: OnceLock<std::sync::Mutex<HashMap<Uuid, Weak<Mutex<()>>>>> =
    OnceLock::new();

/// Guard ordinary Agent/Plan starts against an active proposal in the same
/// conversation. Explicit approve/request/cancel commands intentionally do
/// not call this helper and remain available while the conversation is locked.
pub async fn ensure_v4_start_allowed(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), String> {
    repository
        .ensure_conversation_unlocked_v4(project_id, conversation_id)
        .await
        .map_err(|error| error.to_string())
}

/// Validate the only resumable plan-generation state. Terminal runs are
/// rejected before looking at legacy `spec == None` records, and a pending
/// proposal can never be silently regenerated into a newer revision.
pub async fn ensure_v4_resume_allowed(
    repository: &Store,
    run_id: Uuid,
) -> Result<ProposedPlanRevisionV4, String> {
    let record = load_record(repository, run_id).await?;
    if matches!(
        record.status.as_str(),
        "completed" | "cancelled" | "failed" | "needs_attention"
    ) {
        return Err("V4 run is terminal and cannot be resumed".into());
    }
    let latest = repository
        .latest_proposed_plan_revision_v4(record.project_id, record.conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "V4 run has no revising plan revision to resume".to_string())?;
    if latest.run_id != run_id {
        return Err("latest plan revision belongs to a different V4 run".into());
    }
    if latest.status == PlanRevisionStatusV4::Pending {
        return Err(
            "pending plan revision must be approved, revised, or cancelled before resume".into(),
        );
    }
    if latest.status == PlanRevisionStatusV4::Cancelled {
        return Err("cancelled plan revision cannot be resumed".into());
    }
    if latest.status != PlanRevisionStatusV4::Revising {
        return Err("only the latest revising plan revision can be resumed".into());
    }
    Ok(latest)
}

/// Reserve the next generating revision for a request-change resume. Keeping
/// this service seam separate from the Tauri `State` makes the command-level
/// transition deterministic and testable without a live window.
pub async fn begin_v4_plan_resume(
    repository: &Store,
    run_id: Uuid,
) -> Result<ProposedPlanRevisionV4, String> {
    let record = load_record(repository, run_id).await?;
    ensure_v4_resume_allowed(repository, run_id).await?;
    ensure_v4_request_resume_status(&record.status)?;
    repository
        .acquire_plan_revision_resume_v4(
            record.project_id,
            record.conversation_id,
            record.run_id,
            &record.objective,
            Utc::now(),
        )
        .await
        .map_err(|error| error.to_string())
}

fn project_side_effect_lock_v4(project_id: Uuid) -> Arc<Mutex<()>> {
    let locks = PROJECT_SIDE_EFFECT_LOCKS_V4.get_or_init(Default::default);
    let mut locks = locks.lock().expect("V4 project lock registry");
    if let Some(lock) = locks.get(&project_id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(project_id, Arc::downgrade(&lock));
    lock
}

pub const AGENT_V4_EVENT_CHANNEL: &str = "agent-v4-event";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartPlanningV4Request {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
    pub objective: String,
    pub compute_selection: ComputeSelectionV4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartDirectV4Request {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
    pub objective: String,
    pub compute_selection: ComputeSelectionV4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovePlanV4Request {
    pub run_id: Uuid,
    #[serde(default)]
    pub approval_hash: Option<String>,
    #[serde(default)]
    pub plan_hash: Option<String>,
    #[serde(default)]
    pub revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeBackendsV4Request {
    pub project_id: Uuid,
    #[serde(default)]
    pub container_image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeBackendAvailabilityV4 {
    pub descriptor: ComputeBackendDescriptorV4,
    pub selectable: bool,
    pub reason: Option<String>,
    pub python_status: String,
    pub r_status: String,
    pub resolved_image_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerV4Request {
    pub run_id: Uuid,
    pub question_id: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveUncertainV4Request {
    pub run_id: Uuid,
    pub call_id: String,
    pub resolution: UncertainResolutionV4,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecideToolApprovalV4Request {
    pub run_id: Uuid,
    pub approval_id: String,
    pub call_hash: String,
    pub decision: ToolApprovalDecisionV4,
    #[serde(default)]
    pub browser_scope: Option<BrowserApprovalScopeV4>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunRecordV4 {
    run_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    model_profile_id: Uuid,
    objective: String,
    status: String,
    plan: Option<ExecutionPlanV4>,
    plan_hash: Option<String>,
    #[serde(default)]
    compute_selection: Option<ComputeSelectionV4>,
    #[serde(default)]
    approval_hash: Option<String>,
    #[serde(default)]
    plan_revision: Option<u64>,
    #[serde(default)]
    spec: Option<RunSpecV4>,
}

#[tauri::command]
pub async fn agent_v4_compute_backends(
    state: State<'_, AppState>,
    request: ComputeBackendsV4Request,
) -> Result<Vec<ComputeBackendAvailabilityV4>, String> {
    let project = workspace_project(&state.repository, request.project_id).await?;
    let mut backends = Vec::new();

    let local_root = std::fs::canonicalize(&project.local_root);
    let python = program_available("python").await;
    let r = program_available("Rscript").await;
    let local_reason = local_root
        .as_ref()
        .err()
        .map(|error| format!("local project root is unavailable: {error}"));
    backends.push(ComputeBackendAvailabilityV4 {
        descriptor: ComputeBackendDescriptorV4 {
            schema_version: 4,
            backend_id: "local".into(),
            kind: ComputeBackendKindV4::Local,
            isolation: IsolationStrengthV4::Process,
            available: local_root.is_ok() && (python || r),
            supports_python: python,
            supports_r: r,
            supports_network_policy: false,
        },
        selectable: local_root.is_ok() && (python || r),
        reason: local_reason
            .or_else(|| (!python && !r).then(|| "Python and R were not found".into())),
        python_status: if python { "available" } else { "unavailable" }.into(),
        r_status: if r { "available" } else { "unavailable" }.into(),
        resolved_image_id: None,
    });

    if let (Some(connection_id), Some(remote_root)) = (project.connection_id, &project.remote_root)
    {
        let profile = find_profile(&state.repository, connection_id).await?;
        let trusted = profile.host_key_fingerprint.is_some();
        let mut ssh_python = false;
        let mut ssh_r = false;
        let mut reason = (!trusted).then(|| "SSH host key is not trusted".to_string());
        if trusted {
            let authentication = authentication_for_profile(&state, &profile)?;
            match SshSession::connect(&profile, authentication).await {
                Ok(session) => match resolve_root(&session, remote_root).await {
                    Ok(_) => {
                        if let Ok(output) = session
                            .execute_checked("printf 'python='; command -v python >/dev/null && printf yes || printf no; printf '\\nr='; command -v Rscript >/dev/null && printf yes || printf no")
                            .await
                        {
                            ssh_python = output.stdout.contains("python=yes");
                            ssh_r = output.stdout.contains("r=yes");
                        }
                    }
                    Err(error) => reason = Some(error),
                },
                Err(error) => reason = Some(error.to_string()),
            }
        }
        let available = reason.is_none() && (ssh_python || ssh_r);
        backends.push(ComputeBackendAvailabilityV4 {
            descriptor: ComputeBackendDescriptorV4 {
                schema_version: 4,
                backend_id: format!("ssh:{connection_id}"),
                kind: ComputeBackendKindV4::Ssh,
                isolation: IsolationStrengthV4::Process,
                available,
                supports_python: ssh_python,
                supports_r: ssh_r,
                supports_network_policy: false,
            },
            selectable: available,
            reason: reason
                .or_else(|| (!available).then(|| "Python and R were not found on SSH".into())),
            python_status: if ssh_python {
                "available"
            } else {
                "unavailable"
            }
            .into(),
            r_status: if ssh_r { "available" } else { "unavailable" }.into(),
            resolved_image_id: None,
        });
    }

    for (program, kind) in [
        ("docker", ComputeBackendKindV4::Docker),
        ("podman", ComputeBackendKindV4::Podman),
    ] {
        let engine = program_available(program).await;
        let (image_id, image_error) = if engine {
            match request
                .container_image
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                Some(image) => inspect_container_image(program, image).await,
                None => (None, Some("enter an existing local container image".into())),
            }
        } else {
            (None, Some(format!("{program} engine was not found")))
        };
        let backend_id = program.to_string();
        let selectable = image_id.is_some();
        backends.push(ComputeBackendAvailabilityV4 {
            descriptor: ComputeBackendDescriptorV4 {
                schema_version: 4,
                backend_id,
                kind,
                isolation: IsolationStrengthV4::Container,
                available: selectable,
                supports_python: selectable,
                supports_r: selectable,
                supports_network_policy: true,
            },
            selectable,
            reason: image_error,
            python_status: if selectable {
                "unverified"
            } else {
                "unavailable"
            }
            .into(),
            r_status: if selectable {
                "unverified"
            } else {
                "unavailable"
            }
            .into(),
            resolved_image_id: image_id,
        });
    }
    Ok(backends)
}

#[tauri::command]
pub async fn agent_v4_start_planning(
    app: AppHandle,
    state: State<'_, AppState>,
    request: StartPlanningV4Request,
) -> Result<RunSummaryV4, String> {
    if request.objective.trim().is_empty() {
        return Err("V4 objective is empty".into());
    }
    ensure_v4_start_allowed(
        &state.repository,
        request.project_id,
        request.conversation_id,
    )
    .await?;
    let project = workspace_project(&state.repository, request.project_id).await?;
    validate_compute_selection(&state, &project, &request.compute_selection).await?;
    let run_id = Uuid::new_v4();
    let mut record = RunRecordV4 {
        run_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        model_profile_id: request.model_profile_id,
        objective: request.objective.clone(),
        status: "planning".into(),
        plan: None,
        plan_hash: None,
        compute_selection: Some(request.compute_selection.clone()),
        approval_hash: None,
        plan_revision: None,
        spec: None,
    };
    let generation = state
        .repository
        .start_plan_run_v4(
            record.run_id,
            record.project_id,
            record.conversation_id,
            &record.status,
            &serde_json::to_value(&record).map_err(|error| error.to_string())?,
            &record.objective,
            Utc::now(),
        )
        .await
        .map_err(|error| error.to_string())?;
    record.plan_revision = Some(generation.revision);
    let planning_cancelled = Arc::new(AtomicBool::new(false));
    let _planning_guard =
        match register_active_run_guard(&state.active_runs, run_id, planning_cancelled.clone()) {
            Ok(Some(guard)) => guard,
            Ok(None) => {
                let message = "V4 planning run is already active";
                return Err(terminate_plan_generation_with_diagnostics(
                    &state.repository,
                    record.project_id,
                    record.conversation_id,
                    record.run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Cancelled,
                    message,
                )
                .await);
            }
            Err(error) => {
                return Err(terminate_plan_generation_with_diagnostics(
                    &state.repository,
                    record.project_id,
                    record.conversation_id,
                    record.run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Cancelled,
                    &error,
                )
                .await);
            }
        };
    if planning_cancelled.load(Ordering::SeqCst)
        || !plan_generation_is_active(&state.repository, generation.id).await?
    {
        return Err("V4 planning was cancelled".into());
    }
    let (model, tools) = match compose(
        &state,
        &project,
        &request.compute_selection,
        request.model_profile_id,
        run_id,
        request.conversation_id,
        None,
        None,
        None,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Err(terminate_plan_generation_with_diagnostics(
                &state.repository,
                record.project_id,
                record.conversation_id,
                record.run_id,
                generation.revision,
                PlanRevisionStatusV4::Cancelled,
                &error,
            )
            .await);
        }
    };
    let event_store = RepositoryEventStoreV4 {
        repository: state.repository.clone(),
        app: app.clone(),
    };
    let science_store = RepositoryScientificStateStoreV4 {
        repository: state.repository.clone(),
        backend_id: request.compute_selection.backend_id.clone(),
        mutation_lock: scientific_state_lock_v4(project.id),
    };
    let core = AgentCoreV4 {
        model: model.as_ref(),
        tools: tools.registry.as_ref(),
        events: &event_store,
        science: Some(&science_store),
    };
    let plan_result = core
        .plan_with_scope(
            run_id,
            request.project_id,
            request.conversation_id,
            &request.objective,
            PlanApprovalScopeV4 {
                project_id: request.project_id,
                conversation_id: request.conversation_id,
                run_id,
                revision_id: generation.id,
                revision: generation.revision,
            },
            planning_cancelled.clone(),
        )
        .await;
    if !plan_generation_is_active(&state.repository, generation.id).await?
        && !matches!(&plan_result, Err(AgentCoreErrorV4::WaitingForApproval))
    {
        return Err("V4 planning was cancelled".into());
    }
    let plan = match plan_result {
        Ok(plan) => plan,
        Err(AgentCoreErrorV4::WaitingForInput) => {
            state
                .repository
                .terminate_plan_generation_v4(
                    request.project_id,
                    request.conversation_id,
                    run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Revising,
                    Some("planning is waiting for user input"),
                )
                .await
                .map_err(|error| error.to_string())?;
            return Ok(RunSummaryV4 {
                run_id,
                status: "waiting_for_input".into(),
                plan: None,
                plan_hash: None,
                compute_selection: Some(request.compute_selection),
                approval_hash: None,
                plan_revision: Some(generation.revision),
                session_mode: Some(SessionAgentModeV4::Plan),
            });
        }
        Err(AgentCoreErrorV4::WaitingForApproval) => {
            if planning_cancelled.load(Ordering::SeqCst) {
                let message = "V4 planning was cancelled";
                return Err(terminate_plan_generation_with_diagnostics(
                    &state.repository,
                    request.project_id,
                    request.conversation_id,
                    run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Cancelled,
                    message,
                )
                .await);
            }
            state
                .repository
                .pause_plan_generation_for_approval_v4(
                    request.project_id,
                    request.conversation_id,
                    run_id,
                    generation.revision,
                    Some("planning is waiting for exact MCP tool approval"),
                )
                .await
                .map_err(|error| error.to_string())?;
            return Ok(RunSummaryV4 {
                run_id,
                status: "waiting_for_approval".into(),
                plan: None,
                plan_hash: None,
                compute_selection: Some(request.compute_selection),
                approval_hash: None,
                plan_revision: Some(generation.revision),
                session_mode: Some(SessionAgentModeV4::Plan),
            });
        }
        Err(error) => {
            let message = error.to_string();
            return Err(terminate_plan_generation_with_diagnostics(
                &state.repository,
                request.project_id,
                request.conversation_id,
                run_id,
                generation.revision,
                PlanRevisionStatusV4::Cancelled,
                &message,
            )
            .await);
        }
    };
    let hash = match plan.canonical_hash() {
        Ok(hash) => hash,
        Err(error) => {
            let message = error.to_string();
            return Err(terminate_plan_generation_with_diagnostics(
                &state.repository,
                record.project_id,
                record.conversation_id,
                record.run_id,
                generation.revision,
                PlanRevisionStatusV4::Cancelled,
                &message,
            )
            .await);
        }
    };
    if planning_cancelled.load(Ordering::SeqCst) {
        let message = "V4 planning was cancelled";
        return Err(terminate_plan_generation_with_diagnostics(
            &state.repository,
            record.project_id,
            record.conversation_id,
            record.run_id,
            generation.revision,
            PlanRevisionStatusV4::Cancelled,
            message,
        )
        .await);
    }
    let approval_hash = match RunSpecV4::approval_hash_for(
        run_id,
        request.project_id,
        request.conversation_id,
        request.model_profile_id,
        &plan,
        &request.compute_selection,
    ) {
        Ok(hash) => hash,
        Err(error) => {
            let message = error.to_string();
            return Err(terminate_plan_generation_with_diagnostics(
                &state.repository,
                record.project_id,
                record.conversation_id,
                record.run_id,
                generation.revision,
                PlanRevisionStatusV4::Cancelled,
                &message,
            )
            .await);
        }
    };
    let revision = state
        .repository
        .finalize_plan_revision_v4_with_options(
            request.project_id,
            request.conversation_id,
            run_id,
            generation.revision,
            plan.clone(),
            plan_markdown(&plan),
            hash.clone(),
            Utc::now(),
            PlanRevisionFinalizeOptionsV4 {
                approval_hash: Some(approval_hash.clone()),
                compute_selection: Some(request.compute_selection.clone()),
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    if planning_cancelled.load(Ordering::SeqCst) {
        let cancellation = state
            .repository
            .cancel_plan_v4(record.project_id, record.conversation_id, record.run_id)
            .await
            .map_err(|error| error.to_string())?;
        if let Some(error) = broadcast_events_best_effort(&cancellation.events, |event| {
            app.emit(AGENT_V4_EVENT_CHANNEL, event)
                .map_err(|error| error.to_string())
        }) {
            eprintln!("failed to broadcast committed V4 cancellation event: {error}");
        }
        return Err("V4 planning was cancelled".into());
    }
    Ok(RunSummaryV4 {
        run_id,
        status: "awaiting_approval".into(),
        plan: Some(plan),
        plan_hash: Some(hash),
        compute_selection: Some(request.compute_selection),
        approval_hash: Some(approval_hash),
        plan_revision: Some(revision.revision),
        session_mode: Some(SessionAgentModeV4::Plan),
    })
}

#[tauri::command]
pub async fn agent_v4_start_direct(
    app: AppHandle,
    state: State<'_, AppState>,
    request: StartDirectV4Request,
) -> Result<RunSummaryV4, String> {
    if request.objective.trim().is_empty() {
        return Err("V4 direct objective is empty".into());
    }
    ensure_v4_start_allowed(
        &state.repository,
        request.project_id,
        request.conversation_id,
    )
    .await?;
    let project = workspace_project(&state.repository, request.project_id).await?;
    validate_compute_selection(&state, &project, &request.compute_selection).await?;
    let (_model, tools) = compose(
        &state,
        &project,
        &request.compute_selection,
        request.model_profile_id,
        Uuid::new_v4(),
        request.conversation_id,
        None,
        None,
        None,
    )
    .await?;
    let run_id = tools.run_id();
    let capabilities = tools
        .registry
        .descriptors(omicsops_protocol::RunModeV4::Execute)
        .into_iter()
        .filter(|tool| !matches!(tool.id.as_str(), "agent.request_input" | "agent.complete"))
        .map(|tool| tool.id)
        .collect::<BTreeSet<_>>();
    let messages = state
        .repository
        .messages_for_conversation(request.conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let conversation = serde_json::to_string(&messages).map_err(|error| error.to_string())?;
    let (conversation, _) = bounded_excerpt(&conversation, 32 * 1024);
    let plan = direct_execution_plan(request.objective.trim(), &conversation, capabilities);
    let approval_hash = RunSpecV4::approval_hash_for(
        run_id,
        request.project_id,
        request.conversation_id,
        request.model_profile_id,
        &plan,
        &request.compute_selection,
    )
    .map_err(|error| error.to_string())?;
    let mut spec = RunSpecV4::freeze_ordinary_agent_with_compute(
        run_id,
        request.project_id,
        request.conversation_id,
        request.model_profile_id,
        plan.clone(),
        request.compute_selection.clone(),
        &approval_hash,
        Utc::now(),
    )
    .map_err(|error| error.to_string())?;
    spec.delegated_model =
        freeze_delegated_model(&state.repository, request.model_profile_id).await?;
    spec.spec_hash = Some(
        spec.calculate_spec_hash()
            .map_err(|error| error.to_string())?,
    );
    let record = RunRecordV4 {
        run_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        model_profile_id: request.model_profile_id,
        objective: request.objective,
        status: "running".into(),
        plan: Some(plan),
        plan_hash: Some(spec.approved_plan_hash.clone()),
        compute_selection: Some(request.compute_selection.clone()),
        approval_hash: Some(approval_hash.clone()),
        plan_revision: None,
        spec: Some(spec.clone()),
    };
    state
        .repository
        .save_agent_run_v4_if_unlocked(
            record.run_id,
            record.project_id,
            record.conversation_id,
            &record.status,
            &serde_json::to_value(&record).map_err(|error| error.to_string())?,
        )
        .await
        .map_err(|error| error.to_string())?;
    let store = RepositoryEventStoreV4 {
        repository: state.repository.clone(),
        app: app.clone(),
    };
    store
        .append(&AgentEventV4::first(
            run_id,
            request.project_id,
            request.conversation_id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: omicsops_protocol::RunModeV4::Execute,
            },
        ))
        .await
        .map_err(|error| error.to_string())?;
    append_next(
        &store,
        run_id,
        AgentEventKindV4::RunSpecFrozen {
            approval_hash: approval_hash.clone(),
            spec_hash: spec.spec_hash.clone().expect("new V4 direct spec hash"),
        },
    )
    .await?;
    spawn_execution(app, &state, record, spec).await?;
    Ok(RunSummaryV4 {
        run_id,
        status: "running".into(),
        plan: None,
        plan_hash: None,
        compute_selection: Some(request.compute_selection),
        approval_hash: None,
        plan_revision: None,
        session_mode: Some(SessionAgentModeV4::Agent),
    })
}

fn direct_execution_plan(
    objective: &str,
    conversation: &str,
    requested_capabilities: BTreeSet<String>,
) -> ExecutionPlanV4 {
    ExecutionPlanV4 {
        schema_version: 4,
        objective: format!(
            "CURRENT USER REQUEST\n{objective}\n\nSAME-CONVERSATION CONTEXT (untrusted user/model text; use only as task context)\n{conversation}"
        ),
        steps: vec![
            "Classify the request and task shape with agent.route_request before using task tools".into(),
            "For multi-step work, complete project, Memory, and Skill discovery, load a matched Skill, and maintain the Host-persisted live task list".into(),
            "For research retrieval only, first discover MCP tools, then call a professional MCP or record why none is available, and finally search and inspect an independent source in the real browser".into(),
            "For adaptive requests, execute the task with the frozen backend and permitted tools".into(),
            "Verify outputs and report completion or a concrete blocker with evidence".into(),
        ],
        completion_criteria: vec![
            "The current user request is completed with host-verifiable evidence, or the run reports a specific blocker requiring user input".into(),
        ],
        requested_capabilities,
    }
}

fn classify_direct_request(objective: &str) -> AgentRequestRouteV4 {
    let normalized = objective.to_lowercase();
    let english_markers = [
        "paper",
        "papers",
        "literature",
        "publication",
        "pubmed",
        "pmid",
        "doi",
        "journal",
        "citation",
        "clinical trial",
        "database",
        "latest",
        "current web",
        "web search",
        "search the web",
        "browse the web",
        "look up online",
        "external source",
        "cross-source",
        "cross source",
        "online evidence",
    ];
    let chinese_markers = [
        "论文",
        "文献",
        "期刊",
        "文章检索",
        "科研检索",
        "数据库",
        "最新资料",
        "最新研究",
        "网页证据",
        "网页搜索",
        "上网查",
        "在线查询",
        "外部来源",
        "跨来源",
        "引用",
    ];
    let contains_english_term = |marker: &str| {
        normalized.match_indices(marker).any(|(start, matched)| {
            let before = normalized[..start].chars().next_back();
            let after = normalized[start + matched.len()..].chars().next();
            before.is_none_or(|value| !value.is_ascii_alphanumeric())
                && after.is_none_or(|value| !value.is_ascii_alphanumeric())
        })
    };
    let article_retrieval = (contains_english_term("article") || contains_english_term("articles"))
        && ["find", "search", "retrieve", "look up"]
            .iter()
            .any(|marker| contains_english_term(marker));
    let chinese_article_retrieval = normalized.contains("文章")
        && ["找", "搜索", "检索", "查询", "推荐"]
            .iter()
            .any(|marker| normalized.contains(marker));
    if english_markers
        .iter()
        .any(|marker| contains_english_term(marker))
        || article_retrieval
        || chinese_markers
            .iter()
            .any(|marker| normalized.contains(marker))
        || chinese_article_retrieval
        || normalized.contains("http://")
        || normalized.contains("https://")
    {
        AgentRequestRouteV4::ResearchRetrieval
    } else {
        AgentRequestRouteV4::Adaptive
    }
}

fn plan_markdown(plan: &ExecutionPlanV4) -> String {
    let steps = plan
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| format!("{}. {}", index + 1, step))
        .collect::<Vec<_>>();
    let criteria = plan
        .completion_criteria
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>();
    format!(
        "# Execution plan\n\n{}\n\n## Steps\n{}\n\n## Completion criteria\n{}",
        plan.objective,
        steps.join("\n"),
        criteria.join("\n")
    )
}

/// The mutation-before-spawn portion of plan approval. It performs all
/// request/hash checks before calling Store, and uses the Store legacy seam
/// when the run predates a persisted proposal. That seam materializes and
/// approves revision one in one transaction.
#[derive(Debug)]
pub(crate) struct PlanApprovalServiceResult {
    pub plan: ExecutionPlanV4,
    pub plan_hash: String,
    pub revision: u64,
    pub selection: ComputeSelectionV4,
    pub expected_approval: String,
    pub spec: RunSpecV4,
    pub approval: PlanApprovalResultV4,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlanApprovalRunContext {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub model_profile_id: Uuid,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn approve_plan_revision_for_command(
    repository: &Store,
    context: PlanApprovalRunContext,
    record_plan_hash: Option<&str>,
    record_plan: ExecutionPlanV4,
    selection: ComputeSelectionV4,
    request: &ApprovePlanV4Request,
    run_value: &Value,
) -> Result<PlanApprovalServiceResult, String> {
    let PlanApprovalRunContext {
        project_id,
        conversation_id,
        run_id,
        model_profile_id,
    } = context;
    if request.run_id != run_id {
        return Err("approval request run does not match the persisted run".into());
    }
    let latest = repository
        .latest_proposed_plan_revision_v4(project_id, conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let legacy = latest.is_none();
    let (plan, plan_hash, revision) = match latest {
        Some(latest) => {
            if latest.run_id != run_id || latest.plan != record_plan {
                return Err("latest plan revision does not match the V4 run".into());
            }
            let revision = request.revision.unwrap_or(latest.revision);
            if revision != latest.revision {
                return Err("only the latest plan revision can be approved".into());
            }
            (latest.plan, latest.plan_hash, latest.revision)
        }
        None => {
            let plan_hash = record_plan_hash
                .ok_or_else(|| "V4 plan hash is missing".to_string())?
                .to_owned();
            let actual_hash = record_plan
                .canonical_hash()
                .map_err(|error| error.to_string())?;
            if actual_hash != plan_hash {
                return Err("V4 plan hash does not match the frozen plan".into());
            }
            if request.revision.is_some_and(|revision| revision != 1) {
                return Err("only the latest plan revision can be approved".into());
            }
            (record_plan, plan_hash, 1)
        }
    };
    let expected_approval = RunSpecV4::approval_hash_for(
        run_id,
        project_id,
        conversation_id,
        model_profile_id,
        &plan,
        &selection,
    )
    .map_err(|error| error.to_string())?;
    let accepted = match (
        request.approval_hash.as_deref(),
        request.plan_hash.as_deref(),
    ) {
        (Some(value), _) => value == expected_approval || (legacy && value == plan_hash),
        (None, Some(value)) => value == plan_hash || value == expected_approval,
        (None, None) => false,
    };
    if !accepted {
        return Err("V4 approval hash does not match the frozen plan and compute selection".into());
    }
    let spec = RunSpecV4::freeze_with_compute(
        run_id,
        project_id,
        conversation_id,
        model_profile_id,
        plan.clone(),
        selection.clone(),
        &expected_approval,
        Utc::now(),
    )
    .map_err(|error| error.to_string())?;
    let mut persisted_run_value = run_value.clone();
    let object = persisted_run_value
        .as_object_mut()
        .ok_or_else(|| "approved run value must be a JSON object".to_string())?;
    object.insert(
        "plan".into(),
        serde_json::to_value(&plan).map_err(|error| error.to_string())?,
    );
    object.insert("plan_hash".into(), Value::String(plan_hash.clone()));
    object.insert(
        "compute_selection".into(),
        serde_json::to_value(&selection).map_err(|error| error.to_string())?,
    );
    object.insert(
        "approval_hash".into(),
        Value::String(expected_approval.clone()),
    );
    object.insert("plan_revision".into(), Value::from(revision));
    let approval = if legacy {
        repository
            .approve_legacy_plan_revision_v4(
                project_id,
                conversation_id,
                run_id,
                plan.clone(),
                plan_markdown(&plan),
                &plan_hash,
                &spec,
                &persisted_run_value,
            )
            .await
    } else {
        repository
            .approve_plan_revision_v4(
                project_id,
                conversation_id,
                run_id,
                revision,
                &plan_hash,
                &spec,
                &persisted_run_value,
            )
            .await
    }
    .map_err(|error| error.to_string())?;
    Ok(PlanApprovalServiceResult {
        plan,
        plan_hash,
        revision,
        selection,
        expected_approval,
        spec,
        approval,
    })
}

pub(crate) async fn plan_generation_is_active(
    repository: &Store,
    revision_id: Uuid,
) -> Result<bool, String> {
    Ok(repository
        .proposed_plan_revision_v4(revision_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some_and(|revision| revision.status == PlanRevisionStatusV4::Generating))
}

/// Cancel planning durably before returning from the command. Execution runs
/// only receive the in-memory cancellation token; planning runs additionally
/// transition their revision/run/event/mode state through Store atomically.
pub(crate) async fn cancel_active_run_for_command(
    repository: &Store,
    run_id: Uuid,
    active_token: Option<Arc<AtomicBool>>,
) -> Result<Option<PlanCancellationResultV4>, String> {
    let record = load_record(repository, run_id).await?;
    let revisions = repository
        .proposed_plan_revisions_v4(record.project_id, record.conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let planning = revisions.iter().any(|revision| {
        revision.run_id == run_id
            && (revision.status.is_active()
                || (revision.status == PlanRevisionStatusV4::Cancelled
                    && record.status == "cancelled"))
    });
    if planning {
        if let Some(token) = &active_token {
            token.store(true, Ordering::SeqCst);
        }
        return repository
            .cancel_plan_v4(record.project_id, record.conversation_id, run_id)
            .await
            .map(Some)
            .map_err(|error| error.to_string());
    }
    if let Some(token) = active_token {
        token.store(true, Ordering::SeqCst);
        return Ok(None);
    }
    Err("V4 run is not active".into())
}

pub(crate) async fn cancel_active_run_command_response<F>(
    repository: &Store,
    run_id: Uuid,
    active_token: Option<Arc<AtomicBool>>,
    emit: F,
) -> Result<(), String>
where
    F: FnMut(&AgentEventV4) -> Result<(), String>,
{
    if let Some(cancellation) =
        cancel_active_run_for_command(repository, run_id, active_token).await?
    {
        if let Some(error) = broadcast_events_best_effort(&cancellation.events, emit) {
            eprintln!("failed to broadcast committed V4 cancellation event: {error}");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn agent_v4_approve_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ApprovePlanV4Request,
) -> Result<RunSummaryV4, String> {
    let record = load_record(&state.repository, request.run_id).await?;
    if record.status != "awaiting_approval" {
        return Err("V4 run is not awaiting approval".into());
    }
    let record_plan = record
        .plan
        .clone()
        .ok_or_else(|| "V4 plan is missing".to_string())?;
    let project = workspace_project(&state.repository, record.project_id).await?;
    let selection = match record.compute_selection.clone() {
        Some(selection) => selection,
        None => legacy_ssh_selection(&project)?,
    };
    validate_compute_selection(&state, &project, &selection).await?;
    let run_value = serde_json::to_value(&record).map_err(|error| error.to_string())?;
    let approved = approve_plan_revision_for_command(
        &state.repository,
        PlanApprovalRunContext {
            project_id: record.project_id,
            conversation_id: record.conversation_id,
            run_id: record.run_id,
            model_profile_id: record.model_profile_id,
        },
        record.plan_hash.as_deref(),
        record_plan,
        selection,
        &request,
        &run_value,
    )
    .await?;
    let mut record = record;
    record.status = "running".into();
    record.plan = Some(approved.plan.clone());
    record.plan_hash = Some(approved.plan_hash.clone());
    record.compute_selection = Some(approved.selection.clone());
    record.approval_hash = Some(approved.expected_approval.clone());
    record.spec = Some(approved.spec.clone());
    record.plan_revision = Some(approved.revision);
    // The Store transaction has committed before any event is emitted or
    // execution is spawned. A UI reconnect therefore observes a coherent
    // approval/mode/spec state even if the process exits here.
    if let Some(error) = broadcast_events_best_effort(&approved.approval.events, |event| {
        app.emit(AGENT_V4_EVENT_CHANNEL, event)
            .map_err(|error| error.to_string())
    }) {
        eprintln!("failed to broadcast committed V4 approval event: {error}");
    }
    let approved_plan_hash = approved.spec.approved_plan_hash.clone();
    spawn_execution(app, &state, record.clone(), approved.spec.clone()).await?;
    Ok(RunSummaryV4 {
        run_id: record.run_id,
        status: "running".into(),
        plan: Some(approved.plan),
        plan_hash: Some(approved_plan_hash),
        compute_selection: Some(approved.selection),
        approval_hash: Some(approved.expected_approval),
        plan_revision: Some(approved.revision),
        session_mode: Some(SessionAgentModeV4::Agent),
    })
}

/// Shared command helper used by the native boundary and deterministic
/// contract tests. It resolves project/conversation ownership from the run,
/// so callers cannot submit feedback for another project's conversation.
pub async fn request_plan_revision_response(
    repository: &Store,
    request: &AgentV4RequestPlanRevisionRequest,
) -> Result<RequestPlanRevisionResponseV4, String> {
    request_plan_revision_committed(repository, request)
        .await
        .map(|(response, _event)| response)
}

/// Store feedback and its hash-chained event atomically. The event is handed
/// to the Tauri command for post-commit broadcast only.
pub async fn request_plan_revision_committed(
    repository: &Store,
    request: &AgentV4RequestPlanRevisionRequest,
) -> Result<(RequestPlanRevisionResponseV4, AgentEventV4), String> {
    let record = load_record(repository, request.run_id).await?;
    let result = repository
        .request_plan_revision_v4_with_event(
            record.project_id,
            record.conversation_id,
            record.run_id,
            &request.plan_hash,
            &request.feedback,
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok((
        RequestPlanRevisionResponseV4 {
            run_id: result.revision.run_id,
            revision: result.revision.revision,
            plan_hash: result.revision.plan_hash,
            status: result.revision.status,
            feedback: result.revision.feedback,
        },
        result.event,
    ))
}

/// Complete the request-changes command boundary. The Store transaction is
/// committed before the event is handed to the UI, so an emit failure must
/// not turn a durable revision request into a command failure or strand the
/// conversation lock.
pub(crate) async fn request_plan_revision_command_response<F>(
    repository: &Store,
    request: &AgentV4RequestPlanRevisionRequest,
    emit: F,
) -> Result<RequestPlanRevisionResponseV4, String>
where
    F: FnMut(&AgentEventV4) -> Result<(), String>,
{
    let (response, event) = request_plan_revision_committed(repository, request).await?;
    if let Some(error) = broadcast_events_best_effort(std::slice::from_ref(&event), emit) {
        eprintln!("failed to broadcast committed V4 plan revision request event: {error}");
    }
    Ok(response)
}

#[tauri::command]
pub async fn agent_v4_request_plan_revision(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AgentV4RequestPlanRevisionRequest,
) -> Result<RequestPlanRevisionResponseV4, String> {
    request_plan_revision_command_response(&state.repository, &request, |event| {
        app.emit(AGENT_V4_EVENT_CHANNEL, event)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn agent_v4_resume(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<(), String> {
    // A waiting execution removes itself from the active registry immediately
    // after emitting its pause event. An approval can arrive in that narrow
    // window, so give the old task time to yield before starting the resume.
    // If another resume already won the slot, this request is idempotent.
    wait_for_active_run_to_yield(&state.active_runs, run_id).await?;
    let mut record = load_record(&state.repository, run_id).await?;
    if matches!(
        record.status.as_str(),
        "completed" | "cancelled" | "failed" | "needs_attention"
    ) {
        return Err("V4 run is terminal".into());
    }
    if record.spec.is_none() {
        // Reject pending/cancelled/replayed plan runs before any legacy
        // compute-selection fallback or model composition can obscure the
        // request -> revising -> resume contract.
        let latest = ensure_v4_resume_allowed(&state.repository, run_id).await?;
        let approval_resume = record.status == "waiting_for_approval";
        if approval_resume {
            let events = state
                .repository
                .agent_events_v4(run_id)
                .await
                .map_err(|error| error.to_string())?;
            let scope = PlanApprovalScopeV4 {
                project_id: record.project_id,
                conversation_id: record.conversation_id,
                run_id: record.run_id,
                revision_id: latest.id,
                revision: latest.revision,
            };
            plan_approval_resume_decision(&events, scope)?;
        } else {
            ensure_v4_request_resume_status(&record.status)?;
        }
        let project = workspace_project(&state.repository, record.project_id).await?;
        let selection = record
            .compute_selection
            .clone()
            .unwrap_or(legacy_ssh_selection(&project)?);
        validate_compute_selection(&state, &project, &selection).await?;
        let generation = if approval_resume {
            state
                .repository
                .resume_plan_generation_after_approval_v4(
                    record.project_id,
                    record.conversation_id,
                    record.run_id,
                    latest.id,
                    latest.revision,
                    Utc::now(),
                )
                .await
                .map_err(|error| error.to_string())?
        } else {
            begin_v4_plan_resume(&state.repository, run_id).await?
        };
        let planning_cancelled = Arc::new(AtomicBool::new(false));
        let _planning_guard = match register_active_run_guard(
            &state.active_runs,
            record.run_id,
            planning_cancelled.clone(),
        ) {
            Ok(Some(guard)) => guard,
            Ok(None) => {
                let message = "V4 planning run is already active";
                return Err(terminate_plan_generation_with_diagnostics(
                    &state.repository,
                    record.project_id,
                    record.conversation_id,
                    record.run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Cancelled,
                    message,
                )
                .await);
            }
            Err(error) => {
                return Err(terminate_plan_generation_with_diagnostics(
                    &state.repository,
                    record.project_id,
                    record.conversation_id,
                    record.run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Cancelled,
                    &error,
                )
                .await);
            }
        };
        if planning_cancelled.load(Ordering::SeqCst)
            || !plan_generation_is_active(&state.repository, generation.id).await?
        {
            return Err("V4 planning was cancelled".into());
        }
        let (model, tools) = match compose(
            &state,
            &project,
            &selection,
            record.model_profile_id,
            record.run_id,
            record.conversation_id,
            None,
            None,
            None,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                return Err(terminate_plan_generation_with_diagnostics(
                    &state.repository,
                    record.project_id,
                    record.conversation_id,
                    record.run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Cancelled,
                    &error,
                )
                .await);
            }
        };
        let store = RepositoryEventStoreV4 {
            repository: state.repository.clone(),
            app: app.clone(),
        };
        let science_store = RepositoryScientificStateStoreV4 {
            repository: state.repository.clone(),
            backend_id: selection.backend_id.clone(),
            mutation_lock: scientific_state_lock_v4(project.id),
        };
        let core = AgentCoreV4 {
            model: model.as_ref(),
            tools: tools.registry.as_ref(),
            events: &store,
            science: Some(&science_store),
        };
        let plan_result = core
            .plan_with_scope(
                record.run_id,
                record.project_id,
                record.conversation_id,
                &record.objective,
                PlanApprovalScopeV4 {
                    project_id: record.project_id,
                    conversation_id: record.conversation_id,
                    run_id: record.run_id,
                    revision_id: generation.id,
                    revision: generation.revision,
                },
                planning_cancelled.clone(),
            )
            .await;
        if !plan_generation_is_active(&state.repository, generation.id).await?
            && !matches!(&plan_result, Err(AgentCoreErrorV4::WaitingForApproval))
        {
            return Err("V4 planning was cancelled".into());
        }
        match plan_result {
            Ok(plan) => {
                let hash = match plan.canonical_hash() {
                    Ok(hash) => hash,
                    Err(error) => {
                        let message = error.to_string();
                        return Err(terminate_plan_generation_with_diagnostics(
                            &state.repository,
                            record.project_id,
                            record.conversation_id,
                            record.run_id,
                            generation.revision,
                            PlanRevisionStatusV4::Cancelled,
                            &message,
                        )
                        .await);
                    }
                };
                if planning_cancelled.load(Ordering::SeqCst) {
                    let message = "V4 planning was cancelled";
                    return Err(terminate_plan_generation_with_diagnostics(
                        &state.repository,
                        record.project_id,
                        record.conversation_id,
                        record.run_id,
                        generation.revision,
                        PlanRevisionStatusV4::Cancelled,
                        message,
                    )
                    .await);
                }
                let approval_hash = match RunSpecV4::approval_hash_for(
                    record.run_id,
                    record.project_id,
                    record.conversation_id,
                    record.model_profile_id,
                    &plan,
                    &selection,
                ) {
                    Ok(hash) => hash,
                    Err(error) => {
                        let message = error.to_string();
                        return Err(terminate_plan_generation_with_diagnostics(
                            &state.repository,
                            record.project_id,
                            record.conversation_id,
                            record.run_id,
                            generation.revision,
                            PlanRevisionStatusV4::Cancelled,
                            &message,
                        )
                        .await);
                    }
                };
                let _revision = state
                    .repository
                    .finalize_plan_revision_v4_with_options(
                        record.project_id,
                        record.conversation_id,
                        record.run_id,
                        generation.revision,
                        plan.clone(),
                        plan_markdown(&plan),
                        hash.clone(),
                        Utc::now(),
                        PlanRevisionFinalizeOptionsV4 {
                            approval_hash: Some(approval_hash),
                            compute_selection: Some(selection.clone()),
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                if planning_cancelled.load(Ordering::SeqCst) {
                    let cancellation = state
                        .repository
                        .cancel_plan_v4(record.project_id, record.conversation_id, record.run_id)
                        .await
                        .map_err(|error| error.to_string())?;
                    if let Some(error) =
                        broadcast_events_best_effort(&cancellation.events, |event| {
                            app.emit(AGENT_V4_EVENT_CHANNEL, event)
                                .map_err(|error| error.to_string())
                        })
                    {
                        eprintln!("failed to broadcast committed V4 cancellation event: {error}");
                    }
                    return Err("V4 planning was cancelled".into());
                }
                return Ok(());
            }
            Err(AgentCoreErrorV4::WaitingForInput) => {
                state
                    .repository
                    .terminate_plan_generation_v4(
                        record.project_id,
                        record.conversation_id,
                        record.run_id,
                        generation.revision,
                        PlanRevisionStatusV4::Revising,
                        Some("planning is waiting for user input"),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
            Err(AgentCoreErrorV4::WaitingForApproval) => {
                if planning_cancelled.load(Ordering::SeqCst) {
                    let message = "V4 planning was cancelled";
                    return Err(terminate_plan_generation_with_diagnostics(
                        &state.repository,
                        record.project_id,
                        record.conversation_id,
                        record.run_id,
                        generation.revision,
                        PlanRevisionStatusV4::Cancelled,
                        message,
                    )
                    .await);
                }
                state
                    .repository
                    .pause_plan_generation_for_approval_v4(
                        record.project_id,
                        record.conversation_id,
                        record.run_id,
                        generation.revision,
                        Some("planning is waiting for exact MCP tool approval"),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
            Err(error) => {
                let message = error.to_string();
                return Err(terminate_plan_generation_with_diagnostics(
                    &state.repository,
                    record.project_id,
                    record.conversation_id,
                    record.run_id,
                    generation.revision,
                    PlanRevisionStatusV4::Cancelled,
                    &message,
                )
                .await);
            }
        }
    }
    let Some(mut spec) = record.spec.clone() else {
        return Err("V4 run has no frozen specification or resumable plan revision".into());
    };
    if spec.compute_selection.is_none() {
        let project = workspace_project(&state.repository, record.project_id).await?;
        let selection = legacy_ssh_selection(&project)?;
        validate_compute_selection(&state, &project, &selection).await?;
        let approval_hash = RunSpecV4::approval_hash_for(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            spec.model_profile_id,
            &spec.plan,
            &selection,
        )
        .map_err(|error| error.to_string())?;
        spec = RunSpecV4::freeze_with_compute(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            spec.model_profile_id,
            spec.plan,
            selection.clone(),
            &approval_hash,
            spec.created_at,
        )
        .map_err(|error| error.to_string())?;
        let store = RepositoryEventStoreV4 {
            repository: state.repository.clone(),
            app: app.clone(),
        };
        append_next(
            &store,
            record.run_id,
            AgentEventKindV4::RunSpecFrozen {
                approval_hash: approval_hash.clone(),
                spec_hash: spec.spec_hash.clone().expect("upgraded V4 spec hash"),
            },
        )
        .await?;
        record.compute_selection = Some(selection);
        record.approval_hash = Some(approval_hash);
        record.spec = Some(spec.clone());
        save_record(&state.repository, &record).await?;
    }
    validate_frozen_spec(&state.repository, &spec).await?;
    let existing_events = state
        .repository
        .agent_events_v4(run_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(call_id) = recoverable_system_environment_ensure(
        &existing_events,
        spec.compute_selection
            .as_ref()
            .map(|selection| selection.environment.as_str()),
    ) {
        let store = RepositoryEventStoreV4 {
            repository: state.repository.clone(),
            app: app.clone(),
        };
        append_next(
            &store,
            run_id,
            AgentEventKindV4::ToolDispatchResolved {
                call_id,
                resolution: UncertainResolutionV4::SideEffectNotObserved,
                evidence: "legacy system environment ensure failed before mutation; V4 system ensure is now an immutable readiness check".into(),
            },
        )
        .await?;
    }
    spawn_execution(app, &state, record, spec).await
}

fn ensure_v4_request_resume_status(status: &str) -> Result<(), String> {
    if matches!(status, "waiting_for_input" | "planning") {
        Ok(())
    } else {
        Err("only a waiting-for-input or request-changes planning Plan run can be resumed".into())
    }
}

fn recoverable_system_environment_ensure(
    events: &[AgentEventV4],
    frozen_environment: Option<&str>,
) -> Option<String> {
    if frozen_environment != Some("system")
        || !events.iter().any(|event| {
            matches!(
                &event.event,
                AgentEventKindV4::RunFailed { message }
                    if message.contains("system environment cannot be created or changed")
                        || message.contains("environments are immutable in V4")
            )
        })
    {
        return None;
    }
    let call_id = events.iter().rev().find_map(|event| match &event.event {
        AgentEventKindV4::ToolDispatchStarted {
            call_id, tool_id, ..
        } if tool_id == "runtime.environment.ensure" => Some(call_id.clone()),
        _ => None,
    })?;
    let resolved = events.iter().any(|event| {
        matches!(&event.event, AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == call_id)
            || matches!(&event.event, AgentEventKindV4::ToolOutcomeReused { outcome, .. } if outcome.call_id == call_id)
            || matches!(&event.event, AgentEventKindV4::ToolDispatchResolved { call_id: resolved, .. } if resolved == &call_id)
    });
    (!resolved).then_some(call_id)
}

#[tauri::command]
pub async fn agent_v4_cancel(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<(), String> {
    let active_token = state
        .active_runs
        .lock()
        .map_err(|_| "active run registry unavailable".to_string())?
        .get(&run_id)
        .cloned();
    cancel_active_run_command_response(&state.repository, run_id, active_token, |event| {
        app.emit(AGENT_V4_EVENT_CHANNEL, event)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn agent_v4_answer(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AnswerV4Request,
) -> Result<(), String> {
    let events = state
        .repository
        .agent_events_v4(request.run_id)
        .await
        .map_err(|error| error.to_string())?;
    let answer = validate_answer_v4(&events, &request.question_id, &request.answer)?;
    let store = RepositoryEventStoreV4 {
        repository: state.repository.clone(),
        app,
    };
    append_next(
        &store,
        request.run_id,
        AgentEventKindV4::UserInputAnswered {
            question_id: request.question_id,
            answer,
        },
    )
    .await
}

fn validate_answer_v4(
    events: &[AgentEventV4],
    requested_question_id: &str,
    supplied_answer: &str,
) -> Result<String, String> {
    let answer = supplied_answer.trim().to_owned();
    if answer.is_empty() {
        return Err("answer cannot be empty".into());
    }
    let requested = events.iter().any(|event| {
        matches!(&event.event, AgentEventKindV4::InputRequested { question_id, .. } if question_id == requested_question_id)
    });
    if !requested {
        return Err("the referenced V4 question does not exist".into());
    }
    if events.iter().any(|event| {
        matches!(&event.event, AgentEventKindV4::UserInputAnswered { question_id, .. } if question_id == requested_question_id)
    }) {
        return Err("the referenced V4 question was already answered".into());
    }
    let latest_unanswered = events.iter().rev().find_map(|event| match &event.event {
        AgentEventKindV4::InputRequested { question_id, .. }
            if !events.iter().any(|candidate| {
                matches!(&candidate.event, AgentEventKindV4::UserInputAnswered { question_id: answered, .. } if answered == question_id)
            }) => Some(question_id),
        _ => None,
    });
    if latest_unanswered.map(String::as_str) != Some(requested_question_id) {
        return Err("only the latest unanswered V4 question can be answered".into());
    }
    Ok(answer)
}

fn plan_approval_resume_decision(
    events: &[AgentEventV4],
    scope: PlanApprovalScopeV4,
) -> Result<ToolApprovalDecisionV4, String> {
    let mut found = None;
    for event in events {
        let AgentEventKindV4::ToolApprovalRequested { request } = &event.event else {
            continue;
        };
        let call_is_pending = events.iter().any(|candidate| {
            matches!(
                &candidate.event,
                AgentEventKindV4::ToolRequested { call }
                    if call == &request.call
            )
        }) && !events.iter().any(|candidate| {
            matches!(
                &candidate.event,
                AgentEventKindV4::ToolFinished { outcome }
                    if outcome.call_id == request.call.call_id
            ) || matches!(
                &candidate.event,
                AgentEventKindV4::ToolOutcomeReused { outcome, .. }
                    if outcome.call_id == request.call.call_id
            )
        }) && !events.iter().any(|candidate| {
            matches!(
                &candidate.event,
                AgentEventKindV4::ToolDispatchStarted { call_id, .. }
                    if call_id == &request.call.call_id
            ) || matches!(
                &candidate.event,
                AgentEventKindV4::ToolDispatchUncertain { call_id, .. }
                    if call_id == &request.call.call_id
            ) || matches!(
                &candidate.event,
                AgentEventKindV4::ToolDispatchResolved { call_id, .. }
                    if call_id == &request.call.call_id
            )
        });
        if request.mode != omicsops_protocol::RunModeV4::Plan
            || request.scope_hash.as_deref() != Some(scope.hash().as_str())
            || request.effect != ToolEffectV4::ReadOnly
            || request.call.tool_id != "use_mcp_tool"
            || !call_is_pending
        {
            continue;
        }
        request
            .validate_with_scope(
                scope.run_id,
                &scope.hash(),
                omicsops_protocol::RunModeV4::Plan,
            )
            .map_err(|error| error.to_string())?;
        if found.is_some() {
            return Err("multiple pending Plan tool approvals are not supported".into());
        }
        let mut decision = None;
        for candidate in events {
            if let AgentEventKindV4::ToolApprovalDecided {
                approval_id,
                call_hash,
                decision: value,
            } = &candidate.event
            {
                if approval_id == &request.approval_id {
                    if call_hash != &request.call_hash || decision.is_some() {
                        return Err("tool approval decision is duplicated or tampered".into());
                    }
                    decision = Some(*value);
                }
            }
        }
        found = Some(decision.ok_or_else(|| {
            "Plan tool approval is still undecided; decide it before resuming".to_string()
        })?);
    }
    found.ok_or_else(|| "V4 approval resume has no pending Plan tool approval".into())
}

#[tauri::command]
pub async fn agent_v4_decide_tool_approval(
    app: AppHandle,
    state: State<'_, AppState>,
    request: DecideToolApprovalV4Request,
) -> Result<(), String> {
    let record = load_record(&state.repository, request.run_id).await?;
    let events = state
        .repository
        .agent_events_v4(request.run_id)
        .await
        .map_err(|error| error.to_string())?;
    let approval = events.iter().find_map(|event| match &event.event {
        AgentEventKindV4::ToolApprovalRequested { request: approval }
            if approval.approval_id == request.approval_id =>
        {
            Some(approval)
        }
        _ => None,
    });
    let approval = approval.ok_or("tool approval request was not found")?;
    if approval.call_hash != request.call_hash {
        return Err("tool approval call hash mismatch".into());
    }
    let is_browser_call =
        approval.call.tool_id == "browser_setup" || approval.call.tool_id.starts_with("web_");
    if request.browser_scope.is_some()
        && (request.decision != ToolApprovalDecisionV4::Approved || !is_browser_call)
    {
        return Err("browserScope is only valid for an approved browser call".into());
    }
    let browser_authorization =
        if request.decision == ToolApprovalDecisionV4::Approved && is_browser_call {
            let scope = request
                .browser_scope
                .unwrap_or(BrowserApprovalScopeV4::Once);
            let binding = crate::browser_commands::browser_binding_for_call(
                &approval.call.tool_id,
                &approval.call.arguments,
            )?;
            let (project_id, conversation_id) = match scope {
                BrowserApprovalScopeV4::Once | BrowserApprovalScopeV4::Conversation => {
                    (Some(record.project_id), Some(record.conversation_id))
                }
                BrowserApprovalScopeV4::Project => (Some(record.project_id), None),
                BrowserApprovalScopeV4::Global => (None, None),
            };
            let canonical = json!({
                "scope":scope,
                "binding":binding,
                "project_id":project_id,
                "conversation_id":conversation_id,
            });
            Some(BrowserAuthorizationV4 {
                id: hex::encode(sha2::Sha256::digest(
                    serde_json::to_vec(&canonical).map_err(|error| error.to_string())?,
                )),
                scope,
                binding,
                project_id,
                conversation_id,
                created_at_ms: Utc::now().timestamp_millis(),
            })
        } else {
            None
        };
    let event = if let Some(spec) = &record.spec {
        let spec_hash = spec
            .spec_hash
            .clone()
            .ok_or("V4 run has no frozen spec hash")?;
        if approval.mode != omicsops_protocol::RunModeV4::Execute || approval.scope_hash.is_some() {
            return Err("Execute approval request has an unexpected Plan binding".into());
        }
        approval
            .validate(request.run_id, &spec_hash)
            .map_err(|error| error.to_string())?;
        if let Some(authorization) = &browser_authorization {
            state
                .repository
                .decide_tool_approval_v4_with_browser_authorization(
                    record.project_id,
                    record.conversation_id,
                    request.run_id,
                    &request.approval_id,
                    &request.call_hash,
                    request.decision,
                    &spec_hash,
                    authorization,
                )
                .await
                .map_err(|error| error.to_string())?
        } else {
            state
                .repository
                .decide_tool_approval_v4(
                    record.project_id,
                    record.conversation_id,
                    request.run_id,
                    &request.approval_id,
                    &request.call_hash,
                    request.decision,
                    &spec_hash,
                    omicsops_protocol::RunModeV4::Execute,
                    None,
                )
                .await
                .map_err(|error| error.to_string())?
        }
    } else {
        let latest = state
            .repository
            .latest_proposed_plan_revision_v4(record.project_id, record.conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or("V4 run has no active plan revision")?;
        let consistent_phase = (record.status == "planning"
            && latest.status == PlanRevisionStatusV4::Generating)
            || (record.status == "waiting_for_approval"
                && latest.status == PlanRevisionStatusV4::Revising);
        if latest.run_id != request.run_id || !consistent_phase {
            return Err(
                "Plan approval does not belong to the current planning revision phase".into(),
            );
        }
        let scope = PlanApprovalScopeV4 {
            project_id: record.project_id,
            conversation_id: record.conversation_id,
            run_id: request.run_id,
            revision_id: latest.id,
            revision: latest.revision,
        };
        let scope_hash = scope.hash();
        if approval.mode != omicsops_protocol::RunModeV4::Plan
            || approval.scope_hash.as_deref() != Some(scope_hash.as_str())
            || approval.effect != ToolEffectV4::ReadOnly
            || approval.call.tool_id != "use_mcp_tool"
        {
            return Err("Plan approval request has an invalid current revision binding".into());
        }
        approval
            .validate_with_scope(
                request.run_id,
                &scope_hash,
                omicsops_protocol::RunModeV4::Plan,
            )
            .map_err(|error| error.to_string())?;
        state
            .repository
            .decide_tool_approval_v4(
                record.project_id,
                record.conversation_id,
                request.run_id,
                &request.approval_id,
                &request.call_hash,
                request.decision,
                &scope_hash,
                omicsops_protocol::RunModeV4::Plan,
                Some(&scope_hash),
            )
            .await
            .map_err(|error| error.to_string())?
    };
    app.emit(AGENT_V4_EVENT_CHANNEL, &event)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn agent_v4_resolve_uncertain(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ResolveUncertainV4Request,
) -> Result<(), String> {
    if request.evidence.trim().is_empty() {
        return Err("uncertain-dispatch resolution requires verification evidence".into());
    }
    let events = state
        .repository
        .agent_events_v4(request.run_id)
        .await
        .map_err(|error| error.to_string())?;
    let marked = events.iter().any(|event| {
        matches!(&event.event, AgentEventKindV4::ToolDispatchUncertain { call_id, .. } if call_id == &request.call_id)
    });
    if !marked {
        return Err("the referenced call is not marked uncertain".into());
    }
    let resolved = events.iter().any(|event| {
        matches!(&event.event, AgentEventKindV4::ToolDispatchResolved { call_id, .. } if call_id == &request.call_id)
    });
    if resolved {
        return Err("the uncertain dispatch is already resolved".into());
    }
    let store = RepositoryEventStoreV4 {
        repository: state.repository.clone(),
        app,
    };
    append_next(
        &store,
        request.run_id,
        AgentEventKindV4::ToolDispatchResolved {
            call_id: request.call_id,
            resolution: request.resolution,
            evidence: request.evidence,
        },
    )
    .await
}

#[tauri::command]
pub async fn agent_v4_events(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<Vec<AgentEventV4>, String> {
    reconcile_run_terminal_event(&app, &state, run_id).await?;
    state
        .repository
        .agent_events_v4(run_id)
        .await
        .map_err(|error| error.to_string())
}

/// Read one conversation's durable Agent state without requiring a live
/// Tauri runtime. The Store supplies all source rows from one SQLite
/// transaction; this adapter validates and types the persisted run JSON at
/// the command boundary.
pub async fn conversation_state_response(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<ConversationAgentStateV4, String> {
    let snapshot = repository
        .conversation_agent_state_v4(project_id, conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    if snapshot.locked && snapshot.mode != SessionAgentModeV4::Plan {
        return Err("conversation has an active plan lock but is not in Plan mode".into());
    }

    let latest_run = snapshot
        .latest_run_json
        .map(|value| {
            let record: RunRecordV4 = serde_json::from_value(value)
                .map_err(|error| format!("latest Agent V4 run has invalid record JSON: {error}"))?;
            if record.run_id == Uuid::nil() {
                return Err("latest Agent V4 run has an invalid run id".into());
            }
            if record.project_id != project_id || record.conversation_id != conversation_id {
                return Err(
                    "latest Agent V4 run does not belong to the requested conversation".into(),
                );
            }
            run_summary_from_record(
                &record,
                snapshot.mode,
                snapshot.latest_plan_revision.as_ref(),
            )
        })
        .transpose()?;

    if snapshot.latest_plan_revision.is_some() && latest_run.is_none() {
        return Err("latest plan revision has no corresponding Agent V4 run".into());
    }

    Ok(ConversationAgentStateV4 {
        project_id: snapshot.project_id,
        conversation_id: snapshot.conversation_id,
        mode: snapshot.mode,
        locked: snapshot.locked,
        latest_plan_revision: snapshot.latest_plan_revision,
        latest_run,
    })
}

fn run_summary_from_record(
    record: &RunRecordV4,
    mode: SessionAgentModeV4,
    latest_plan_revision: Option<&ProposedPlanRevisionV4>,
) -> Result<RunSummaryV4, String> {
    let matching_revision =
        latest_plan_revision.filter(|revision| revision.run_id == record.run_id);
    if let (Some(record_revision), Some(revision)) = (record.plan_revision, matching_revision) {
        if record_revision != revision.revision {
            return Err(
                "latest Agent V4 run plan revision does not match the latest plan revision".into(),
            );
        }
    }
    let plan_revision = matching_revision
        .map(|revision| revision.revision)
        .or(record.plan_revision);
    Ok(RunSummaryV4 {
        run_id: record.run_id,
        status: record.status.clone(),
        plan: record.plan.clone(),
        plan_hash: record.plan_hash.clone(),
        compute_selection: record.compute_selection.clone(),
        approval_hash: record.approval_hash.clone(),
        plan_revision,
        session_mode: Some(mode),
    })
}

#[tauri::command]
pub async fn agent_v4_conversation_state(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<ConversationAgentStateV4, String> {
    conversation_state_response(&state.repository, project_id, conversation_id).await
}

#[tauri::command]
pub async fn agent_v4_events_for_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Vec<AgentEventV4>, String> {
    let records = state
        .repository
        .agent_runs_for_context_v4(project_id, conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    for value in records {
        let record: RunRecordV4 =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        reconcile_run_terminal_event(&app, &state, record.run_id).await?;
    }
    state
        .repository
        .agent_events_for_context_v4(project_id, conversation_id)
        .await
        .map_err(|error| error.to_string())
}

async fn reconcile_run_terminal_event(
    app: &AppHandle,
    state: &AppState,
    run_id: Uuid,
) -> Result<(), String> {
    let mut record = load_record(&state.repository, run_id).await?;
    let events = state
        .repository
        .agent_events_v4(run_id)
        .await
        .map_err(|error| error.to_string())?;
    if has_terminal_event(&events) {
        return Ok(());
    }
    let active = state
        .active_runs
        .lock()
        .map_err(|_| "active run registry unavailable".to_string())?
        .contains_key(&run_id);
    let terminal = missing_terminal_event(&record, &events, active, Utc::now());
    if terminal.is_some() && matches!(record.status.as_str(), "completed" | "running") {
        record.status = "failed".into();
    }
    if let Some(terminal) = terminal {
        let store = RepositoryEventStoreV4 {
            repository: state.repository.clone(),
            app: app.clone(),
        };
        append_terminal_event(&store, run_id, terminal).await?;
        save_record(&state.repository, &record).await?;
    }
    Ok(())
}

fn has_terminal_event(events: &[AgentEventV4]) -> bool {
    events.iter().any(|event| {
        matches!(
            &event.event,
            AgentEventKindV4::RunCompleted
                | AgentEventKindV4::RunFailed { .. }
                | AgentEventKindV4::RunNeedsAttention { .. }
                | AgentEventKindV4::RunCancelled
        )
    })
}

fn missing_terminal_event(
    record: &RunRecordV4,
    events: &[AgentEventV4],
    active: bool,
    now: chrono::DateTime<Utc>,
) -> Option<AgentEventKindV4> {
    let stale_running = record.status == "running"
        && !active
        && events.last().is_some_and(|last| {
            now.signed_duration_since(last.occurred_at) > chrono::Duration::minutes(2)
        });
    match record.status.as_str() {
        "failed" => Some(AgentEventKindV4::RunFailed {
            message: "the previous execution stopped without persisting its final error; retry from the verified event chain".into(),
        }),
        "cancelled" => Some(AgentEventKindV4::RunCancelled),
        "needs_attention" => Some(AgentEventKindV4::RunNeedsAttention {
            message: "the previous execution requires attention but its final event was interrupted".into(),
        }),
        "completed" => Some(AgentEventKindV4::RunFailed {
            message: "the previous execution ended without a durable completion response; retry from the verified event chain".into(),
        }),
        "running" if stale_running => Some(AgentEventKindV4::RunFailed {
            message: "the desktop process stopped while this run was active; retry from the verified event chain".into(),
        }),
        _ => None,
    }
}

async fn spawn_execution(
    app: AppHandle,
    state: &AppState,
    mut record: RunRecordV4,
    spec: RunSpecV4,
) -> Result<(), String> {
    spec.validate_integrity()
        .map_err(|error| error.to_string())?;
    validate_frozen_spec(&state.repository, &spec).await?;
    let selection = spec
        .compute_selection
        .clone()
        .ok_or("V4 execution spec is missing a compute selection")?;
    let project = workspace_project(&state.repository, spec.project_id).await?;
    validate_compute_selection(state, &project, &selection).await?;
    let forced_route = (spec.execution_kind == RunExecutionKindV4::OrdinaryAgent)
        .then(|| classify_direct_request(&record.objective));
    let (model, tools) = compose(
        state,
        &project,
        &selection,
        spec.model_profile_id,
        spec.run_id,
        spec.conversation_id,
        Some(&spec.plan.requested_capabilities),
        forced_route,
        spec.delegated_model.as_ref(),
    )
    .await?;
    let cancelled = Arc::new(AtomicBool::new(false));
    if !register_active_run(&state.active_runs, spec.run_id, cancelled.clone())? {
        // Resume/approval actions are idempotent. A duplicate UI submission
        // must not replace the cancellation token of the execution already
        // running for this run ID.
        return Ok(());
    }
    let repository = state.repository.clone();
    let active = state.active_runs.clone();
    let browser = state.browser.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = async {
            let store = RepositoryEventStoreV4 {
                repository: repository.clone(),
                app: app.clone(),
            };
            let science_store = RepositoryScientificStateStoreV4 {
                repository: repository.clone(),
                backend_id: selection.backend_id.clone(),
                mutation_lock: scientific_state_lock_v4(project.id),
            };
            AgentCoreV4 {
                model: model.as_ref(),
                tools: tools.registry.as_ref(),
                events: &store,
                science: Some(&science_store),
            }
            .execute_with_limits(&spec, AgentLimitsV4::default(), &cancelled)
            .await
            .map_err(|error| error.to_string())
        }
        .await;
        let waiting = outcome
            .as_ref()
            .is_err_and(|error| error == "run is waiting for user input");
        let waiting_for_approval = outcome
            .as_ref()
            .is_err_and(|error| error == "run is waiting for tool approval");
        let uncertain = outcome
            .as_ref()
            .is_err_and(|error| error.contains("side-effect dispatch is uncertain"));
        let verifier_attention = outcome
            .as_ref()
            .is_err_and(|error| error.starts_with("run needs attention:"));
        if outcome.is_ok() || (!waiting && !waiting_for_approval) {
            let config = browser.config().await;
            let mut cleanup_sessions = Vec::new();
            let mut cleanup_tabs = Vec::new();
            for session in [
                omicsops_browser::BrowserSessionKind::Shared,
                omicsops_browser::BrowserSessionKind::Workspace,
            ] {
                let tabs = browser.run_tab_summaries(session, spec.run_id).await;
                let needs_confirmation = if config.auto_close_turn_tabs {
                    browser.close_run_tabs(session, spec.run_id).await.is_err()
                } else {
                    !tabs.is_empty()
                };
                if needs_confirmation {
                    let session_v4 = match session {
                        omicsops_browser::BrowserSessionKind::Shared => {
                            BrowserSessionKindV4::Shared
                        }
                        omicsops_browser::BrowserSessionKind::Workspace => {
                            BrowserSessionKindV4::Workspace
                        }
                    };
                    cleanup_sessions.push(session_v4);
                    cleanup_tabs.extend(tabs.into_iter().map(|tab| BrowserTabSummaryV4 {
                        session: session_v4,
                        tab_id: tab.tab_id,
                        run_id: tab.run_id,
                        title: tab.title,
                        origin: tab.origin,
                        created_by_run: tab.created_by_run,
                    }));
                }
            }
            if !cleanup_sessions.is_empty() {
                let event_store = RepositoryEventStoreV4 {
                    repository: repository.clone(),
                    app: app.clone(),
                };
                let message = if config.auto_close_turn_tabs {
                    "OmicsOps could not confirm automatic cleanup of this run's browser tabs. Review and close them explicitly."
                } else {
                    "This run left browser tabs open because automatic per-run cleanup is disabled. Confirm when you want OmicsOps to close them."
                };
                if let Err(error) = append_next(
                    &event_store,
                    spec.run_id,
                    AgentEventKindV4::BrowserTabCleanupRequired {
                        sessions: cleanup_sessions,
                        tabs: cleanup_tabs,
                        message: message.into(),
                    },
                )
                .await
                {
                    eprintln!("failed to persist browser tab cleanup prompt: {error}");
                }
            }
        }
        record.status = if outcome.is_ok() {
            "completed"
        } else if waiting {
            "waiting_for_input"
        } else if waiting_for_approval {
            "waiting_for_approval"
        } else if uncertain || verifier_attention {
            "needs_attention"
        } else if cancelled.load(Ordering::SeqCst) {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        if let Err(error) = &outcome {
            if !cancelled.load(Ordering::SeqCst) && !waiting && !waiting_for_approval {
                let store = RepositoryEventStoreV4 {
                    repository: repository.clone(),
                    app: app.clone(),
                };
                let event = if uncertain || verifier_attention {
                    AgentEventKindV4::RunNeedsAttention {
                        message: error.clone(),
                    }
                } else {
                    AgentEventKindV4::RunFailed {
                        message: error.clone(),
                    }
                };
                let already_recorded = verifier_attention
                    && repository
                        .agent_events_v4(spec.run_id)
                        .await
                        .ok()
                        .and_then(|events| events.last().cloned())
                        .is_some_and(|event| {
                            matches!(event.event, AgentEventKindV4::RunNeedsAttention { .. })
                        });
                if !already_recorded {
                    if append_terminal_event(&store, spec.run_id, event)
                        .await
                        .is_err()
                    {
                        // The status row lets the reconciliation path repair a
                        // missing terminal event on the next UI poll/startup.
                        record.status = "failed".into();
                    }
                }
            }
        }
        let _ = save_record(&repository, &record).await;
        remove_active_run(&active, spec.run_id, &cancelled);
    });
    Ok(())
}

async fn append_terminal_event(
    store: &RepositoryEventStoreV4,
    run_id: Uuid,
    event: AgentEventKindV4,
) -> Result<(), String> {
    let mut last_error = None;
    for _ in 0..3 {
        let events = store.load(run_id).await?;
        if events.iter().any(|event| {
            matches!(
                &event.event,
                AgentEventKindV4::RunCompleted
                    | AgentEventKindV4::RunFailed { .. }
                    | AgentEventKindV4::RunNeedsAttention { .. }
                    | AgentEventKindV4::RunCancelled
            )
        }) {
            return Ok(());
        }
        let previous = events.last().ok_or("V4 run has no event")?;
        match store
            .append(&AgentEventV4::next(previous, Utc::now(), event.clone()))
            .await
        {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "failed to persist terminal V4 event".into()))
}

async fn terminate_plan_generation_with_diagnostics(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    revision: u64,
    status: PlanRevisionStatusV4,
    message: &str,
) -> String {
    match repository
        .terminate_plan_generation_v4(
            project_id,
            conversation_id,
            run_id,
            revision,
            status,
            Some(message),
        )
        .await
    {
        Ok(_) => message.to_owned(),
        Err(cleanup) => format!("{message}; cleanup failed: {cleanup}"),
    }
}

fn broadcast_events_best_effort<F>(events: &[AgentEventV4], mut emit: F) -> Option<String>
where
    F: FnMut(&AgentEventV4) -> Result<(), String>,
{
    let mut first_error = None;
    for event in events {
        if let Err(error) = emit(event) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error
}

fn register_active_run(
    active_runs: &Arc<std::sync::Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    run_id: Uuid,
    token: Arc<AtomicBool>,
) -> Result<bool, String> {
    let mut active = active_runs
        .lock()
        .map_err(|_| "active run registry unavailable".to_string())?;
    match active.entry(run_id) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(token);
            Ok(true)
        }
        std::collections::hash_map::Entry::Occupied(_) => Ok(false),
    }
}

fn remove_active_run(
    active_runs: &Arc<std::sync::Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    run_id: Uuid,
    token: &Arc<AtomicBool>,
) {
    let Ok(mut active) = active_runs.lock() else {
        return;
    };
    if active
        .get(&run_id)
        .is_some_and(|current| Arc::ptr_eq(current, token))
    {
        active.remove(&run_id);
    }
}

struct ActiveRunGuard {
    active_runs: Arc<std::sync::Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    run_id: Uuid,
    token: Arc<AtomicBool>,
}

impl Drop for ActiveRunGuard {
    fn drop(&mut self) {
        remove_active_run(&self.active_runs, self.run_id, &self.token);
    }
}

fn register_active_run_guard(
    active_runs: &Arc<std::sync::Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    run_id: Uuid,
    token: Arc<AtomicBool>,
) -> Result<Option<ActiveRunGuard>, String> {
    if !register_active_run(active_runs, run_id, token.clone())? {
        return Ok(None);
    }
    Ok(Some(ActiveRunGuard {
        active_runs: active_runs.clone(),
        run_id,
        token,
    }))
}

async fn wait_for_active_run_to_yield(
    active_runs: &Arc<std::sync::Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    run_id: Uuid,
) -> Result<(), String> {
    wait_for_active_run_to_yield_with(
        active_runs,
        run_id,
        100,
        std::time::Duration::from_millis(20),
    )
    .await
}

async fn wait_for_active_run_to_yield_with(
    active_runs: &Arc<std::sync::Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    run_id: Uuid,
    poll_attempts: usize,
    poll_delay: std::time::Duration,
) -> Result<(), String> {
    for _ in 0..poll_attempts {
        let is_active = active_runs
            .lock()
            .map_err(|_| "active run registry unavailable".to_string())?
            .contains_key(&run_id);
        if !is_active {
            return Ok(());
        }
        tokio::time::sleep(poll_delay).await;
    }
    Err("V4 run is still active after the resume wait timeout; no resume was started".into())
}

// Keeps V4 composition independent from Tauri internals; concrete credentials are resolved before spawning.
struct ComposedToolsV4 {
    run_id: Uuid,
    registry: Arc<ToolRegistryV4>,
}
impl ComposedToolsV4 {
    fn run_id(&self) -> Uuid {
        self.run_id
    }
}

async fn freeze_delegated_model(
    repository: &Store,
    main_id: Uuid,
) -> Result<Option<omicsops_protocol::DelegatedModelBindingV4>, String> {
    let main = repository
        .get_model_profile(main_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("model profile not found")?;
    let Some(child_id) = main.delegated_model_profile_id else {
        return Ok(None);
    };
    let child = repository
        .get_model_profile(child_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("configured delegated model profile not found")?;
    if child_id == main_id || !child.supports_tools {
        return Err("delegated model must be a separate tool-capable profile".into());
    }
    Ok(Some(omicsops_protocol::DelegatedModelBindingV4 {
        profile_id: child.id,
        configuration_hash: child.execution_configuration_hash(),
    }))
}

fn validate_delegated_profile(
    profile: &omicsops_core::workspace::ModelProfile,
    binding: &omicsops_protocol::DelegatedModelBindingV4,
) -> Result<(), String> {
    if !profile.supports_tools
        || profile.id != binding.profile_id
        || profile.execution_configuration_hash() != binding.configuration_hash
    {
        return Err(
            "frozen delegated model configuration changed; restore the profile or start a new run"
                .into(),
        );
    }
    Ok(())
}

async fn compose(
    state: &AppState,
    project: &Project,
    selection: &ComputeSelectionV4,
    model_profile_id: Uuid,
    run_id: Uuid,
    conversation_id: Uuid,
    execute_capabilities: Option<&BTreeSet<String>>,
    forced_route: Option<AgentRequestRouteV4>,
    delegated_binding: Option<&omicsops_protocol::DelegatedModelBindingV4>,
) -> Result<(Arc<DesktopModelPortV4>, ComposedToolsV4), String> {
    let (filesystem, environment_port, backend): (
        Arc<dyn ProjectFilesystemPortV4>,
        Arc<dyn RuntimeEnvironmentPortV4>,
        Arc<dyn KernelBackendV4>,
    ) = match selection.backend_kind {
        ComputeBackendKindV4::Ssh => {
            let connection_id = project
                .connection_id
                .ok_or("project has no remote connection")?;
            if selection.backend_id != format!("ssh:{connection_id}") {
                return Err("frozen SSH backend does not match the project binding".into());
            }
            let profile = find_profile(&state.repository, connection_id).await?;
            require_trusted_host(&profile)?;
            let auth = authentication_for_profile(state, &profile)?;
            let session = Arc::new(
                SshSession::connect(&profile, auth)
                    .await
                    .map_err(|error| error.to_string())?,
            );
            session
                .execute_checked(
                    "if command -v python >/dev/null || command -v Rscript >/dev/null; then :; else printf 'SSH V4 backend requires Python or R\\n' >&2; exit 69; fi",
                )
                .await
                .map_err(|error| error.to_string())?;
            let configured_root = project
                .remote_root
                .as_deref()
                .ok_or("project has no remote root")?;
            let root = resolve_root(&session, configured_root).await?;
            (
                Arc::new(SshProjectFilesystemV4 {
                    session: session.clone(),
                    root: root.clone(),
                }),
                Arc::new(SshEnvironmentPortV4 {
                    session: session.clone(),
                    root: root.clone(),
                }),
                Arc::new(SshKernelBackendV4 {
                    session,
                    root,
                    project_id: project.id,
                    backend_id: selection.backend_id.clone(),
                }),
            )
        }
        ComputeBackendKindV4::Local => {
            let filesystem = Arc::new(LocalProjectFilesystemV4::new(&project.local_root)?);
            let backend = Arc::new(LocalKernelBackendV4::new(&project.local_root)?);
            (filesystem, Arc::new(LocalEnvironmentPortV4), backend)
        }
        ComputeBackendKindV4::Docker | ComputeBackendKindV4::Podman => {
            let image = selection
                .container_image
                .as_ref()
                .ok_or("container image is missing")?;
            let filesystem = Arc::new(LocalProjectFilesystemV4::new(&project.local_root)?);
            let backend: Arc<dyn KernelBackendV4> = match selection.backend_kind {
                ComputeBackendKindV4::Docker => Arc::new(ContainerKernelBackendV4::docker(
                    &project.local_root,
                    &image.image_id,
                )?),
                ComputeBackendKindV4::Podman => Arc::new(ContainerKernelBackendV4::podman(
                    &project.local_root,
                    &image.image_id,
                )?),
                _ => unreachable!(),
            };
            (
                filesystem,
                Arc::new(ContainerEnvironmentPortV4 {
                    program: match selection.backend_kind {
                        ComputeBackendKindV4::Docker => "docker".into(),
                        ComputeBackendKindV4::Podman => "podman".into(),
                        _ => unreachable!(),
                    },
                    image_id: image.image_id.clone(),
                }),
                backend,
            )
        }
    };
    let descriptor = backend.descriptor();
    if descriptor.backend_id != selection.backend_id
        || descriptor.kind != selection.backend_kind
        || !descriptor.permits(selection.autonomy_mode)
    {
        return Err(
            "frozen compute selection does not match the runtime backend descriptor".into(),
        );
    }
    let mut prompt = filesystem.prompt_layers(&selection.backend_id).await?;
    prompt.environment.push_str(&format!(
        "; frozen_environment={}; autonomy={:?}; approval_policy={:?}; network_policy={:?}; every runtime call must use the frozen environment",
        selection.environment,
        selection.autonomy_mode,
        selection.approval_policy,
        selection.network_policy
    ));
    let runtime = Arc::new(RuntimeManagerV4::new(backend));
    let model_profile = state
        .repository
        .get_model_profile(model_profile_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("model profile not found")?;
    let executor = Arc::new(DesktopToolExecutorV4 {
        repository: state.repository.clone(),
        mcp_sessions: state.mcp_sessions.clone(),
        credentials: state.credentials,
        filesystem,
        environment_port,
        selection: selection.clone(),
        project_id: project.id,
        run_id,
        conversation_id,
        backend_id: selection.backend_id.clone(),
        runtime,
        browser: state.browser.clone(),
        local_project_root: PathBuf::from(&project.local_root),
        browser_authorizations: Arc::new(std::sync::Mutex::new(
            state
                .repository
                .list_browser_authorizations_v4()
                .await
                .map_err(|error| error.to_string())?,
        )),
        forced_route,
    });
    let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), executor)
        .map_err(|error| error.to_string())?
        .with_side_effect_lock(project_side_effect_lock_v4(project.id));
    let registry = if let Some(capabilities) = execute_capabilities {
        registry.with_execute_capabilities(capabilities.clone())
    } else {
        registry
    };
    let delegated = if let Some(binding) = delegated_binding {
        let child = state
            .repository
            .get_model_profile(binding.profile_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or("frozen delegated model profile not found")?;
        validate_delegated_profile(&child, binding)?;
        Some((
            binding.clone(),
            Box::new(DesktopModelPortV4 {
                client: crate::commands::unified_model_client_for_profile(state, &child)?
                    .with_request_budget(RequestBudget {
                        context_window_tokens: child.effective_context_window_tokens(),
                        reserved_output_tokens: 4096,
                        safety_margin_tokens: 1024,
                    }),
                prompt: prompt.clone(),
                project_root: PathBuf::from(&project.local_root),
                supports_vision: child.supports_vision,
                delegated: None,
            }),
        ))
    } else {
        None
    };
    Ok((
        Arc::new(DesktopModelPortV4 {
            client: unified_model_client(state, model_profile_id)
                .await?
                .with_request_budget(RequestBudget {
                    context_window_tokens: model_profile.effective_context_window_tokens(),
                    // This is a requested output allowance, not an inferred
                    // maximum capability of an unknown model.
                    reserved_output_tokens: 4096,
                    safety_margin_tokens: 1024,
                }),
            prompt,
            project_root: PathBuf::from(&project.local_root),
            supports_vision: model_profile.supports_vision,
            delegated,
        }),
        ComposedToolsV4 {
            run_id,
            registry: Arc::new(registry),
        },
    ))
}

struct DesktopModelPortV4 {
    client: UnifiedModelClient,
    prompt: PromptLayersV4,
    project_root: PathBuf,
    supports_vision: bool,
    delegated: Option<(
        omicsops_protocol::DelegatedModelBindingV4,
        Box<DesktopModelPortV4>,
    )>,
}
impl DesktopModelPortV4 {
    fn prepare_request(
        &self,
        request: ModelRequestV4,
        load_images: bool,
    ) -> Result<ProviderRequest, ModelFailureV4> {
        let tools = request
            .tools
            .into_iter()
            .map(|tool| ProviderToolSpec {
                id: tool.id,
                description: tool.description,
                input_schema: tool.input_schema,
            })
            .collect();
        let mut context = request.context;
        let content = if request.image_refs.is_empty() {
            omicsops_agent::ModelMessageContent::Text(context)
        } else if !self.supports_vision {
            context.push_str("\n\nHOST IMAGE NOTICE\nScreenshot files were saved and hash-verified, but this exact provider/API-host/model profile is not vision-capable. Do not claim to have visually inspected them.");
            omicsops_agent::ModelMessageContent::Text(context)
        } else {
            let mut parts = vec![omicsops_agent::ModelContentPart::Text { text: context }];
            for image in &request.image_refs {
                let bytes = if load_images {
                    verified_model_image(&self.project_root, image)?
                } else {
                    // Preserve the image part for budget validation without
                    // accessing disk. Unknown image cost must not become zero.
                    Vec::new()
                };
                parts.push(omicsops_agent::ModelContentPart::Image {
                    media_type: image.media_type.clone(),
                    data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                });
            }
            omicsops_agent::ModelMessageContent::Parts(parts)
        };
        Ok(ProviderRequest {
            system: request.system,
            messages: vec![omicsops_agent::ModelMessage {
                role: "user".into(),
                content,
            }],
            tools,
            require_strict_json_fallback: true,
        })
    }
}

#[async_trait]
impl ModelPortV4 for DesktopModelPortV4 {
    fn delegated_model(
        &self,
        binding: Option<&omicsops_protocol::DelegatedModelBindingV4>,
    ) -> Result<Option<&dyn ModelPortV4>, ModelFailureV4> {
        match (binding, &self.delegated) {
            (None, _) => Ok(None),
            (Some(expected), Some((actual, model))) if expected == actual => {
                Ok(Some(model.as_ref()))
            }
            _ => Err(ModelFailureV4::permanent(
                ModelErrorClassV4::InvalidRequest,
                "frozen delegated model binding is unavailable or changed",
            )),
        }
    }
    fn prompt_layers(&self) -> PromptLayersV4 {
        self.prompt.clone()
    }

    fn validate_request(&self, request: &ModelRequestV4) -> Result<(), ModelFailureV4> {
        let provider_request = self.prepare_request(request.clone(), false)?;
        self.client
            .validate_request(&provider_request)
            .map_err(|error| {
                ModelFailureV4::permanent(ModelErrorClassV4::InvalidRequest, error.to_string())
            })
    }

    async fn stream(
        &self,
        request: ModelRequestV4,
        on_event: &mut (dyn FnMut(ModelStreamEventV4) + Send),
    ) -> Result<ModelTurnV4, ModelFailureV4> {
        self.validate_request(&request)?;
        let provider_request = self.prepare_request(request, true)?;
        let mut text = String::new();
        let mut calls = ProviderToolCallAccumulator::default();
        let mut provider_error = None;
        let mut accumulator_error = None;
        self.client
            .stream_with_provider(provider_request, |event| match event {
                ProviderStreamEvent::TextDelta { text: delta } => {
                    text.push_str(&delta);
                    on_event(ModelStreamEventV4::TextDelta(delta));
                }
                ProviderStreamEvent::Retrying {
                    attempt,
                    delay_ms,
                    message,
                } => on_event(ModelStreamEventV4::ProviderRetrying {
                    attempt,
                    delay_ms,
                    message,
                }),
                ProviderStreamEvent::Error { code, message, .. } => {
                    provider_error = Some(classify_model_failure(&format!("{code}: {message}")));
                }
                other if accumulator_error.is_none() => {
                    if let Err(error) = calls.push(&other) {
                        accumulator_error = Some(ModelFailureV4::permanent(
                            ModelErrorClassV4::InvalidResponse,
                            error.to_string(),
                        ));
                    }
                }
                _ => {}
            })
            .await
            .map_err(|error| classify_model_failure(&error.to_string()))?;
        if let Some(error) = provider_error.or(accumulator_error) {
            return Err(error);
        }
        let tool_calls: Vec<ToolCallV4> = calls
            .finish()
            .map_err(|error| {
                ModelFailureV4::permanent(ModelErrorClassV4::InvalidResponse, error.to_string())
            })?
            .into_iter()
            .map(|call| ToolCallV4 {
                call_id: call.call_id,
                tool_id: call.tool_id,
                arguments: call.arguments,
            })
            .collect();
        if text.trim().is_empty() && tool_calls.is_empty() {
            return Err(ModelFailureV4::permanent(
                ModelErrorClassV4::InvalidResponse,
                "empty_model_response: no public text or tools; prior run evidence is retained",
            ));
        }
        Ok(ModelTurnV4 {
            public_text: text,
            tool_calls,
        })
    }

    async fn review(&self, request: ReviewerRequestV4) -> Result<ReviewerReportV4, ModelFailureV4> {
        let context = serde_json::to_string(&request).map_err(|error| {
            ModelFailureV4::permanent(ModelErrorClassV4::InvalidRequest, error.to_string())
        })?;
        let submit = ToolDescriptorV4 {
            id: "agent.submit_review".into(),
            description: "Submit the independent read-only ReviewerReportV4. Every finding must cite evidence present in the frozen review context.".into(),
            input_schema: json!({
                "type":"object",
                "required":["schema_version","summary","findings"],
                "properties":{
                    "schema_version":{"type":"integer","const":4},
                    "summary":{"type":"string"},
                    "findings":{"type":"array","maxItems":8,"items":{
                        "type":"object",
                        "required":["severity","code","message","evidence"],
                        "properties":{
                            "severity":{"type":"string","enum":["error","warn","ok"]},
                            "code":{"type":"string"},
                            "message":{"type":"string"},
                            "evidence":{"type":"array","minItems":1,"items":{"type":"string"}}
                        }
                    }}
                }
            }),
            effect: ToolEffectV4::ReadOnly,
        };
        let turn = self
            .stream(
                ModelRequestV4 {
                    system: "You are an independent read-only scientific Reviewer. You receive only the frozen objective, completion criteria, Host-verified Scientific State, completion proposal, and deterministic verification report. You cannot modify the run. Check sample completeness, numerical/report consistency, evidence support, provenance, seed, versions, and statistical fields. Call agent.submit_review exactly once; cite evidence for every finding.".into(),
                    context,
                    tools: vec![submit],
                    image_refs: vec![],
                },
                &mut |_| {},
            )
            .await?;
        let arguments = turn
            .tool_calls
            .into_iter()
            .find(|call| call.tool_id == "agent.submit_review")
            .map(|call| call.arguments)
            .ok_or_else(|| {
                ModelFailureV4::permanent(
                    ModelErrorClassV4::InvalidResponse,
                    "reviewer did not call agent.submit_review",
                )
            })?;
        let report: ReviewerReportV4 = serde_json::from_value(arguments).map_err(|error| {
            ModelFailureV4::permanent(ModelErrorClassV4::InvalidResponse, error.to_string())
        })?;
        report.validate().map_err(|error| {
            ModelFailureV4::permanent(ModelErrorClassV4::InvalidResponse, error.to_string())
        })?;
        Ok(report)
    }
}

fn verified_model_image(
    project_root: &Path,
    image: &ModelImageRefV4,
) -> Result<Vec<u8>, ModelFailureV4> {
    let fail =
        |message: String| ModelFailureV4::permanent(ModelErrorClassV4::InvalidRequest, message);
    let relative = Path::new(&image.relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || image.media_type != "image/png"
        || image.size_bytes > 20 * 1024 * 1024
    {
        return Err(fail("unsafe or unsupported model image reference".into()));
    }
    let root = std::fs::canonicalize(project_root).map_err(|error| fail(error.to_string()))?;
    let candidate = root.join(relative);
    let metadata =
        std::fs::symlink_metadata(&candidate).map_err(|error| fail(error.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != image.size_bytes
    {
        return Err(fail("model image metadata changed before use".into()));
    }
    let canonical = std::fs::canonicalize(&candidate).map_err(|error| fail(error.to_string()))?;
    if !canonical.starts_with(&root) {
        return Err(fail("model image escaped the project root".into()));
    }
    let bytes = std::fs::read(canonical).map_err(|error| fail(error.to_string()))?;
    if hex::encode(sha2::Sha256::digest(&bytes)) != image.sha256 {
        return Err(fail("model image SHA-256 changed before use".into()));
    }
    Ok(bytes)
}

fn classify_model_failure(message: &str) -> ModelFailureV4 {
    let lower = message.to_ascii_lowercase();
    if lower.contains("context_length_exceeded") || lower.contains("context_window_exceeded") {
        ModelFailureV4::permanent(ModelErrorClassV4::ContextOverflow, message)
    } else if lower.contains("429") || lower.contains("rate limit") {
        ModelFailureV4::transient(ModelErrorClassV4::RateLimited, message)
    } else if ["500", "502", "503", "504"]
        .iter()
        .any(|status| lower.contains(status))
    {
        ModelFailureV4::transient(ModelErrorClassV4::Server, message)
    } else if lower.contains("timed out") || lower.contains("timeout") {
        ModelFailureV4::transient(ModelErrorClassV4::Timeout, message)
    } else if lower.contains("connect")
        || lower.contains("transport")
        || lower.contains("partial output")
        || lower.contains("error decoding response body")
        || lower.contains("unexpected eof")
        || lower.contains("stream ended")
        || lower.contains("incomplete message")
    {
        ModelFailureV4::transient(ModelErrorClassV4::Transport, message)
    } else if lower.contains("401") || lower.contains("403") || lower.contains("credential") {
        ModelFailureV4::permanent(ModelErrorClassV4::Authentication, message)
    } else if lower.contains("400") || lower.contains("422") {
        ModelFailureV4::permanent(ModelErrorClassV4::InvalidRequest, message)
    } else {
        ModelFailureV4::permanent(ModelErrorClassV4::InvalidResponse, message)
    }
}

#[derive(Debug, Clone)]
struct VerifiedProjectFileV4 {
    size_bytes: u64,
    sha256: String,
}

#[async_trait]
trait ProjectFilesystemPortV4: Send + Sync {
    async fn list(&self, path: &str) -> Result<String, String>;
    async fn read(&self, path: &str) -> Result<String, String>;
    async fn verify_file(&self, path: &str) -> Result<VerifiedProjectFileV4, String>;
    async fn prompt_layers(&self, backend_id: &str) -> Result<PromptLayersV4, String>;
}

#[async_trait]
trait RuntimeEnvironmentPortV4: Send + Sync {
    async fn software_versions(
        &self,
        language: KernelLanguageV4,
        environment: &str,
        requirements: Vec<String>,
    ) -> Result<BTreeMap<String, String>, String>;
    async fn ensure(&self, language: KernelLanguageV4, environment: &str)
    -> Result<String, String>;
}

struct LocalProjectFilesystemV4 {
    root: PathBuf,
}

impl LocalProjectFilesystemV4 {
    fn new(root: impl AsRef<Path>) -> Result<Self, String> {
        Ok(Self {
            root: std::fs::canonicalize(root).map_err(|error| error.to_string())?,
        })
    }

    fn resolve_existing(&self, relative: &str) -> Result<PathBuf, String> {
        if Path::new(relative).is_absolute() {
            return Err("project path must be relative".into());
        }
        let candidate = self.root.join(relative);
        let candidate_metadata =
            std::fs::symlink_metadata(&candidate).map_err(|error| error.to_string())?;
        if candidate_metadata.file_type().is_symlink() {
            return Err("project path cannot be a symbolic link".into());
        }
        let path = std::fs::canonicalize(candidate).map_err(|error| error.to_string())?;
        if !path.starts_with(&self.root) {
            return Err("project path escaped the canonical project root".into());
        }
        Ok(path)
    }

    fn read_optional(&self, relative: &str) -> Result<String, String> {
        let path = self.root.join(relative);
        if !path.exists() {
            return Ok(String::new());
        }
        let path = self.resolve_existing(relative)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "project rules path is not a regular file: {relative}"
            ));
        }
        let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        Ok(bounded_excerpt(&content, 32 * 1024).0)
    }
}

#[async_trait]
impl ProjectFilesystemPortV4 for LocalProjectFilesystemV4 {
    async fn list(&self, path: &str) -> Result<String, String> {
        let start = self.resolve_existing(path)?;
        if !start.is_dir() {
            return Err("project list target is not a directory".into());
        }
        let mut rows = Vec::new();
        let mut pending = vec![(start, 0usize)];
        while let Some((directory, depth)) = pending.pop() {
            let entries = std::fs::read_dir(&directory).map_err(|error| error.to_string())?;
            for entry in entries {
                let entry = entry.map_err(|error| error.to_string())?;
                let entry_path = entry.path();
                let metadata =
                    std::fs::symlink_metadata(&entry_path).map_err(|error| error.to_string())?;
                let relative = entry_path
                    .strip_prefix(&self.root)
                    .map_err(|_| "project listing escaped root")?
                    .to_string_lossy()
                    .replace('\\', "/");
                rows.push(format!(
                    "{}\t{}\t{}",
                    if metadata.is_dir() { "d" } else { "f" },
                    metadata.len(),
                    relative
                ));
                if metadata.is_dir() && depth < 1 && rows.len() < 400 {
                    pending.push((entry_path, depth + 1));
                }
                if rows.len() >= 400 {
                    break;
                }
            }
            if rows.len() >= 400 {
                break;
            }
        }
        rows.sort();
        Ok(rows.join("\n"))
    }

    async fn read(&self, path: &str) -> Result<String, String> {
        let path = self.resolve_existing(path)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("project read target is not a regular file".into());
        }
        let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        Ok(content.lines().take(400).collect::<Vec<_>>().join("\n"))
    }

    async fn verify_file(&self, path: &str) -> Result<VerifiedProjectFileV4, String> {
        let path = self.resolve_existing(path)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("verified path is not a regular file".into());
        }
        let mut file = File::open(path).map_err(|error| error.to_string())?;
        let mut digest = sha2::Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(VerifiedProjectFileV4 {
            size_bytes: metadata.len(),
            sha256: hex::encode(digest.finalize()),
        })
    }

    async fn prompt_layers(&self, backend_id: &str) -> Result<PromptLayersV4, String> {
        let agents = self.read_optional("AGENTS.md")?;
        let override_rules = self.read_optional(".omicsops/AGENT.md")?;
        let mut layers = PromptLayersV4::default();
        layers.project_rules = project_rules_layer(&agents, &override_rules);
        layers.environment = format!(
            "backend={backend_id}; persistent Python/R kernels; project-relative paths only; credentials remain Host references"
        );
        Ok(layers)
    }
}

struct SshProjectFilesystemV4 {
    session: Arc<SshSession>,
    root: String,
}

impl SshProjectFilesystemV4 {
    async fn resolve_existing(&self, path: &str) -> Result<String, String> {
        let remote = project_path(&self.root, path)?;
        let output = self
            .session
            .execute_checked(&format!("realpath -- {}", shell_quote(&remote)))
            .await
            .map_err(|error| error.to_string())?;
        let resolved = output.stdout.trim();
        if resolved == self.root.trim_end_matches('/') {
            return Ok(resolved.to_owned());
        }
        require_remote_descendant(&self.root, resolved).map_err(|error| error.to_string())?;
        Ok(resolved.to_owned())
    }
}

#[async_trait]
impl ProjectFilesystemPortV4 for SshProjectFilesystemV4 {
    async fn list(&self, path: &str) -> Result<String, String> {
        let remote = self.resolve_existing(path).await?;
        let output = self
            .session
            .execute_checked(&format!(
                "find {} -maxdepth 2 -printf '%y\\t%s\\t%p\\n' | head -400",
                shell_quote(&remote)
            ))
            .await
            .map_err(|error| error.to_string())?;
        Ok(relative_listing(&output.stdout, &self.root))
    }

    async fn read(&self, path: &str) -> Result<String, String> {
        let raw = project_path(&self.root, path)?;
        let remote = self.resolve_existing(path).await?;
        self.session
            .execute_checked(&format!(
                "test ! -L {0} && test -f {1} && sed -n '1,400p' {1}",
                shell_quote(&raw),
                shell_quote(&remote),
            ))
            .await
            .map(|output| output.stdout)
            .map_err(|error| error.to_string())
    }

    async fn verify_file(&self, path: &str) -> Result<VerifiedProjectFileV4, String> {
        let raw = project_path(&self.root, path)?;
        let remote = self.resolve_existing(path).await?;
        let output = self
            .session
            .execute_checked(&format!(
                "test ! -L {0} && test -f {1} && stat -c '%s' {1} && sha256sum {1}",
                shell_quote(&raw),
                shell_quote(&remote),
            ))
            .await
            .map_err(|error| error.to_string())?;
        verified_file_from_output(&output.stdout)
    }

    async fn prompt_layers(&self, backend_id: &str) -> Result<PromptLayersV4, String> {
        load_prompt_layers(&self.session, &self.root, backend_id).await
    }
}

struct LocalEnvironmentPortV4;

#[async_trait]
impl RuntimeEnvironmentPortV4 for LocalEnvironmentPortV4 {
    async fn software_versions(
        &self,
        language: KernelLanguageV4,
        environment: &str,
        requirements: Vec<String>,
    ) -> Result<BTreeMap<String, String>, String> {
        if environment != "system" {
            return Err("local backend only supports the system environment".into());
        }
        software_versions_from_command(language, None, requirements).await
    }

    async fn ensure(
        &self,
        language: KernelLanguageV4,
        environment: &str,
    ) -> Result<String, String> {
        if environment != "system" {
            return Err("local backend only supports the system environment".into());
        }
        let versions = self
            .software_versions(language, environment, vec![])
            .await?;
        system_environment_ready(language, versions)
    }
}

struct ContainerEnvironmentPortV4 {
    program: String,
    image_id: String,
}

#[async_trait]
impl RuntimeEnvironmentPortV4 for ContainerEnvironmentPortV4 {
    async fn software_versions(
        &self,
        language: KernelLanguageV4,
        environment: &str,
        requirements: Vec<String>,
    ) -> Result<BTreeMap<String, String>, String> {
        if environment != "system" {
            return Err("container environment is frozen in the image".into());
        }
        let (executable, flag, code) = software_version_program(language, &requirements)?;
        let output = background_command(&self.program)
            .args([
                "run",
                "--rm",
                "--read-only",
                "--cap-drop=ALL",
                "--security-opt=no-new-privileges",
                "--pids-limit=64",
                "--network",
                "none",
                &self.image_id,
                executable,
                flag,
                &code,
            ])
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(parse_software_versions(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    async fn ensure(
        &self,
        language: KernelLanguageV4,
        environment: &str,
    ) -> Result<String, String> {
        if environment != "system" {
            return Err("container environment is frozen in the selected image".into());
        }
        let versions = self
            .software_versions(language, environment, vec![])
            .await?;
        system_environment_ready(language, versions)
    }
}

struct SshEnvironmentPortV4 {
    session: Arc<SshSession>,
    root: String,
}

#[async_trait]
impl RuntimeEnvironmentPortV4 for SshEnvironmentPortV4 {
    async fn software_versions(
        &self,
        language: KernelLanguageV4,
        environment: &str,
        requirements: Vec<String>,
    ) -> Result<BTreeMap<String, String>, String> {
        let prefix = (environment != "system").then(|| environment_path(&self.root, environment));
        let command = software_version_command(language, prefix.as_deref(), &requirements)?;
        let output = self
            .session
            .execute_checked(&command)
            .await
            .map_err(|error| error.to_string())?;
        Ok(parse_software_versions(&output.stdout))
    }

    async fn ensure(
        &self,
        language: KernelLanguageV4,
        environment: &str,
    ) -> Result<String, String> {
        if environment == "system" {
            let versions = self
                .software_versions(language, environment, vec![])
                .await?;
            return system_environment_ready(language, versions);
        }
        let prefix = environment_path(&self.root, environment);
        let packages = match language {
            KernelLanguageV4::Python => "python",
            KernelLanguageV4::R => "r-base r-jsonlite",
        };
        let output = self.session.execute_checked(&format!(
            "prefix={0}; mkdir -p {1}; if test -d \"$prefix/conda-meta\"; then printf 'reused %s\\n' \"$prefix\"; else tool=$(command -v micromamba) || {{ printf 'micromamba is required\\n' >&2; exit 69; }}; \"$tool\" create --yes --prefix \"$prefix\" {2}; fi; \"${{tool:-$(command -v micromamba)}}\" list --prefix \"$prefix\" --explicit",
            shell_quote(&prefix),
            shell_quote(&format!("{}/.omicsops/environments", self.root.trim_end_matches('/'))),
            packages,
        )).await.map_err(|error| error.to_string())?;
        Ok(bounded_excerpt(&output.stdout, 16 * 1024).0)
    }
}

fn system_environment_ready(
    language: KernelLanguageV4,
    versions: BTreeMap<String, String>,
) -> Result<String, String> {
    serde_json::to_string(&json!({
        "environment": "system",
        "language": language,
        "status": "ready",
        "mutated": false,
        "software_versions": versions,
    }))
    .map_err(|error| error.to_string())
}

struct DesktopToolExecutorV4 {
    repository: Store,
    mcp_sessions: McpSessionManager,
    credentials: SystemCredentialVault,
    filesystem: Arc<dyn ProjectFilesystemPortV4>,
    environment_port: Arc<dyn RuntimeEnvironmentPortV4>,
    selection: ComputeSelectionV4,
    project_id: Uuid,
    run_id: Uuid,
    conversation_id: Uuid,
    backend_id: String,
    runtime: Arc<RuntimeManagerV4>,
    browser: omicsops_browser::BrowserRuntime,
    local_project_root: PathBuf,
    browser_authorizations: Arc<std::sync::Mutex<Vec<BrowserAuthorizationV4>>>,
    forced_route: Option<AgentRequestRouteV4>,
}

impl DesktopToolExecutorV4 {
    fn key(
        &self,
        language: KernelLanguageV4,
        environment: &str,
    ) -> Result<ExecutionContextKeyV4, String> {
        validate_environment_name(environment)?;
        if environment != self.selection.environment {
            return Err(format!(
                "runtime environment {environment} does not match frozen selection {}",
                self.selection.environment
            ));
        }
        Ok(ExecutionContextKeyV4 {
            project_id: self.project_id,
            run_id: self.run_id,
            backend_id: self.backend_id.clone(),
            language,
            environment: environment.to_owned(),
        })
    }

    async fn software_versions<'a>(
        &self,
        language: KernelLanguageV4,
        environment: &str,
        requirements: impl Iterator<Item = &'a str>,
    ) -> BTreeMap<String, String> {
        let requirements = requirements
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 128
                    && name.chars().all(|character| {
                        character.is_ascii_alphanumeric() || "._-".contains(character)
                    })
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        self.environment_port
            .software_versions(language, environment, requirements)
            .await
            .unwrap_or_default()
    }

    async fn skill_documents(&self) -> Result<Vec<SkillDocumentV4>, String> {
        crate::skill_commands::agent_skill_packages(&self.repository)
            .await?
            .into_iter()
            .map(|package| {
                let markdown = std::fs::read_to_string(
                    std::path::Path::new(&package.source_path).join("SKILL.md"),
                )
                .map_err(|error| format!("cannot read Skill {}: {error}", package.name))?;
                Ok(SkillDocumentV4 {
                    skill_id: package.id,
                    name: package.name,
                    version: package.version,
                    package_sha256: package.sha256,
                    enabled: true,
                    sections: markdown_sections(&markdown),
                })
            })
            .collect()
    }

    async fn memory_documents(
        &self,
        dimension: Option<&str>,
    ) -> Result<Vec<MemoryDocumentV4>, String> {
        let facts = memory_facts(
            &self.repository,
            &MemorySearchRequest {
                project_id: self.project_id,
                conversation_id: None,
                query: String::new(),
                dimension: dimension.map(str::to_owned),
            },
        )
        .await?;
        Ok(facts
            .into_iter()
            .map(|fact| {
                let source = fact.evidence.first();
                MemoryDocumentV4 {
                    id: fact.id,
                    project_id: fact.project_id,
                    conversation_id: fact.conversation_id,
                    dimension: fact.dimension,
                    key: fact.key,
                    statement: fact.statement,
                    source_kind: source
                        .map_or("unknown", |source| source.source_kind.as_str())
                        .into(),
                    source_id: source
                        .map_or("unknown", |source| source.source_id.as_str())
                        .into(),
                    conflicted_with: fact.conflicted_with.into_iter().collect(),
                    created_at: fact.created_at,
                }
            })
            .collect())
    }

    async fn mcp_tool_index(&self) -> Result<Vec<McpToolIndexV4>, String> {
        let profiles = self
            .repository
            .list_json::<McpServerProfile>("mcp_server")
            .await
            .map_err(|error| error.to_string())?;
        let mut index = Vec::new();
        for profile in profiles {
            for tool in &profile.tools {
                let Some(name) = tool.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let input_schema = tool
                    .get("inputSchema")
                    .or_else(|| tool.get("input_schema"))
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object"}));
                index.push(McpToolIndexV4 {
                    server_id: profile.id,
                    server_name: profile.name.clone(),
                    tool_name: name.into(),
                    description: tool
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("MCP tool")
                        .into(),
                    schema_sha256: schema_digest(&input_schema),
                    tool_catalog_sha256: profile.tool_catalog_sha256.clone().unwrap_or_default(),
                    read_only_hint: tool.get("annotations").and_then(Value::as_object).and_then(
                        |annotations| annotations.get("readOnlyHint").and_then(Value::as_bool),
                    ),
                    input_schema,
                    configured: true,
                    enabled: profile.enabled,
                    launch_approved: profile.launch_approved,
                    tool_approved: profile.approved_tools.iter().any(|tool| tool == name),
                    updated_at: profile.updated_at,
                });
            }
        }
        Ok(index)
    }

    async fn run_approved_tool_call(&self, call: &ToolCallV4) -> Result<bool, String> {
        let events = self
            .repository
            .agent_events_v4(self.run_id)
            .await
            .map_err(|error| error.to_string())?;
        let record = load_record(&self.repository, self.run_id).await?;
        let Some(spec_hash) = record
            .spec
            .as_ref()
            .and_then(|spec| spec.spec_hash.as_deref())
        else {
            return Ok(false);
        };
        run_has_approved_tool_call(
            &events,
            call,
            omicsops_protocol::RunModeV4::Execute,
            Some(spec_hash),
            None,
            self.run_id,
        )
    }

    async fn run_approved_plan_tool_call(&self, call: &ToolCallV4) -> Result<bool, String> {
        let record = load_record(&self.repository, self.run_id).await?;
        let latest = self
            .repository
            .latest_proposed_plan_revision_v4(record.project_id, record.conversation_id)
            .await;
        let latest = match latest {
            Ok(Some(latest)) if latest.run_id == self.run_id => latest,
            Ok(_) => return Ok(false),
            Err(error) => return Err(error.to_string()),
        };
        let phase_ok = (record.status == "planning"
            && latest.status == PlanRevisionStatusV4::Generating)
            || (record.status == "waiting_for_approval"
                && latest.status == PlanRevisionStatusV4::Revising);
        if !phase_ok {
            return Ok(false);
        }
        let scope = PlanApprovalScopeV4 {
            project_id: record.project_id,
            conversation_id: latest.conversation_id,
            run_id: self.run_id,
            revision_id: latest.id,
            revision: latest.revision,
        };
        let scope_hash = scope.hash();
        let events = self
            .repository
            .agent_events_v4(self.run_id)
            .await
            .map_err(|error| error.to_string())?;
        run_has_approved_tool_call(
            &events,
            call,
            omicsops_protocol::RunModeV4::Plan,
            Some(scope_hash.as_str()),
            Some(ToolEffectV4::ReadOnly),
            self.run_id,
        )
    }

    fn browser_binding(&self, call: &ToolCallV4) -> Result<BrowserApprovalBindingV4, String> {
        crate::browser_commands::browser_binding_for_call(&call.tool_id, &call.arguments)
    }

    fn browser_authorization_matches(
        &self,
        authorization: &BrowserAuthorizationV4,
        binding: &BrowserApprovalBindingV4,
    ) -> bool {
        if &authorization.binding != binding {
            return false;
        }
        match authorization.scope {
            BrowserApprovalScopeV4::Once | BrowserApprovalScopeV4::Conversation => {
                authorization.project_id == Some(self.project_id)
                    && authorization.conversation_id == Some(self.conversation_id)
            }
            BrowserApprovalScopeV4::Project => {
                authorization.project_id == Some(self.project_id)
                    && authorization.conversation_id.is_none()
            }
            BrowserApprovalScopeV4::Global => {
                authorization.project_id.is_none() && authorization.conversation_id.is_none()
            }
        }
    }

    fn matching_browser_authorization(&self, call: &ToolCallV4) -> Option<BrowserAuthorizationV4> {
        let binding = self.browser_binding(call).ok()?;
        self.browser_authorizations
            .lock()
            .ok()?
            .iter()
            .find(|authorization| self.browser_authorization_matches(authorization, &binding))
            .cloned()
    }

    async fn execute_browser_tool(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
        let session = browser_session_for_arguments(&call.arguments)?;
        let binding = self.browser_binding(call)?;
        let cached_authorization = self.matching_browser_authorization(call);
        let authorization = if let Some(authorization) = cached_authorization {
            let still_authorized = self
                .repository
                .consume_browser_authorization_v4(self.project_id, self.conversation_id, &binding)
                .await
                .map_err(|error| error.to_string())?;
            if still_authorized {
                if authorization.scope == BrowserApprovalScopeV4::Once {
                    self.browser_authorizations
                        .lock()
                        .map_err(|_| "browser authorization cache unavailable".to_string())?
                        .retain(|value| value.id != authorization.id);
                }
                Some(authorization)
            } else {
                None
            }
        } else {
            None
        };
        let exact_run_approval = if authorization.is_none() {
            self.run_approved_tool_call(call).await?
        } else {
            false
        };
        if authorization.is_none() && !exact_run_approval {
            return Ok(ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: false,
                model_content: "Host browser authorization is required for this exact capability, target host, session, and protocol version.".into(),
                data: json!({
                    "error_kind":"browser_authorization_required",
                    "session":session.as_str(),
                    "protocol_version":omicsops_browser::PROTOCOL_VERSION,
                    "binding":self.browser_binding(call)?,
                }),
                provenance: vec![],
            });
        }
        let reply = if call.tool_id == "browser_setup" {
            let launch = call
                .arguments
                .get("launch_if_needed")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let setup = self
                .browser
                .setup(session, launch)
                .await
                .map(|status| json!({"status":status}));
            match setup {
                Ok(mut data) => {
                    let configured = self.browser.config().await.default_search_provider;
                    data["default_search_provider"] = json!(if configured == "default" {
                        "google".to_owned()
                    } else {
                        configured
                    });
                    Ok(data)
                }
                Err(error) => Err(error),
            }
        } else {
            self.browser
                .call(session, self.run_id, &call.tool_id, call.arguments.clone())
                .await
                .map(|reply| reply.data)
        };
        let mut data = match reply {
            Ok(data) => data,
            Err(omicsops_browser::BrowserError::NotConnected(_)) => {
                return Ok(ToolOutcomeV4 {
                    call_id: call.call_id.clone(),
                    tool_id: call.tool_id.clone(),
                    succeeded: false,
                    model_content: format!(
                        "The OmicsOps {} browser bridge is not connected. Install/enable the packaged extension, open the Browser settings connection guide, then resume this same run.",
                        session.as_str()
                    ),
                    data: json!({
                        "error_kind":"browser_connection_required",
                        "session":session.as_str(),
                        "protocol_version":omicsops_browser::PROTOCOL_VERSION,
                        "extension_id":omicsops_browser::EXTENSION_ID,
                    }),
                    provenance: vec![],
                });
            }
            Err(error) => {
                let human_intervention = error.to_string().to_ascii_lowercase().contains("captcha");
                return Ok(ToolOutcomeV4 {
                    call_id: call.call_id.clone(),
                    tool_id: call.tool_id.clone(),
                    succeeded: false,
                    model_content: format!(
                        "Browser command failed without advancing the research stage: {error}"
                    ),
                    data: json!({
                        "error_kind":if human_intervention {"human_intervention_required"} else {"browser_command_failed"},
                        "reason":if human_intervention {Some("captcha_detected")} else {None},
                        "recoverable":true,
                        "session":session.as_str(),
                    }),
                    provenance: vec![],
                });
            }
        };
        if let Some(tab_id) = call.arguments.get("tab_id").and_then(Value::as_u64) {
            if !data.is_object() {
                data = json!({"result": data});
            }
            if let Some(object) = data.as_object_mut() {
                object.insert("tab_id".into(), json!(tab_id));
            }
        }
        if matches!(call.tool_id.as_str(), "web_search" | "web_open_tab") {
            if !data.is_object() {
                data = json!({"result": data});
            }
            if let Some(object) = data.as_object_mut() {
                object
                    .entry("target_host")
                    .or_insert_with(|| Value::String(binding.target_host.clone()));
            }
        }
        if call.tool_id == "web_screenshot" {
            let data_url = data
                .get("data_url")
                .and_then(Value::as_str)
                .ok_or("browser screenshot reply did not contain data_url")?;
            let relative_path = required(&call.arguments, "relative_path")?;
            let saved = self
                .browser
                .save_screenshot(&self.local_project_root, relative_path, data_url)
                .await
                .map_err(|error| error.to_string())?;
            data = serde_json::to_value(saved).map_err(|error| error.to_string())?;
        } else if call.tool_id == "web_save_assets" {
            let mut saved = Vec::new();
            for asset in data
                .get("assets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let staged_id = required(asset, "staged_id")?;
                let relative_path = required(asset, "relative_path")?;
                saved.push(
                    self.browser
                        .save_staged_asset(&self.local_project_root, staged_id, relative_path)
                        .await
                        .map_err(|error| error.to_string())?,
                );
            }
            data = serde_json::to_value(saved).map_err(|error| error.to_string())?;
        }
        if call.tool_id == "web_scan" {
            if !data.is_object() {
                data = json!({"scan": data});
            }
            let page_kind = call
                .arguments
                .get("page_kind")
                .cloned()
                .unwrap_or_else(|| json!("source"));
            if let Some(object) = data.as_object_mut() {
                object.insert("page_kind".into(), page_kind.clone());
                if page_kind == json!("search_results") && !object.contains_key("result_count") {
                    if let Some(count) = object
                        .get("results")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                    {
                        object.insert("result_count".into(), json!(count));
                    }
                }
            }
        }
        let serialized = serde_json::to_string(&data).map_err(|error| error.to_string())?;
        let (content, _) = bounded_excerpt(&serialized, 64 * 1024);
        Ok(ToolOutcomeV4 {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            succeeded: true,
            model_content: content,
            data,
            provenance: vec![format!(
                "browser:{}:protocol-{}",
                session.as_str(),
                omicsops_browser::PROTOCOL_VERSION
            )],
        })
    }
}

fn run_has_approved_tool_call(
    events: &[AgentEventV4],
    call: &ToolCallV4,
    expected_mode: omicsops_protocol::RunModeV4,
    expected_binding_hash: Option<&str>,
    expected_effect: Option<ToolEffectV4>,
    run_id: Uuid,
) -> Result<bool, String> {
    let call_hash = call.canonical_hash().map_err(|error| error.to_string())?;
    let request = events.iter().rev().find_map(|event| match &event.event {
        AgentEventKindV4::ToolApprovalRequested { request }
            if request.call_hash == call_hash
                && request.call == *call
                && request.mode == expected_mode
                && request.scope_hash.as_deref()
                    == if expected_mode == omicsops_protocol::RunModeV4::Plan {
                        expected_binding_hash
                    } else {
                        None
                    }
                && expected_effect.is_none_or(|effect| request.effect == effect) =>
        {
            Some(request)
        }
        _ => None,
    });
    let Some(request) = request else {
        return Ok(false);
    };
    let binding = expected_binding_hash.ok_or("approval is missing its binding hash")?;
    if expected_mode == omicsops_protocol::RunModeV4::Plan {
        request
            .validate_with_scope(run_id, binding, omicsops_protocol::RunModeV4::Plan)
            .map_err(|error| error.to_string())?;
    } else {
        request
            .validate(run_id, binding)
            .map_err(|error| error.to_string())?;
    }
    let mut decision = None;
    for event in events {
        if let AgentEventKindV4::ToolApprovalDecided {
            approval_id,
            call_hash: decided_hash,
            decision: value,
        } = &event.event
        {
            if approval_id == &request.approval_id {
                if decided_hash != &request.call_hash || decision.is_some() {
                    return Err("tool approval decision is duplicated or tampered".into());
                }
                decision = Some(*value);
            }
        }
    }
    Ok(decision == Some(ToolApprovalDecisionV4::Approved))
}
impl DesktopToolExecutorV4 {
    async fn authorize_plan_target(
        &self,
        call: &ToolCallV4,
    ) -> Result<PlanToolAuthorizationV4, String> {
        if call.tool_id != "use_mcp_tool" {
            return Err(format!(
                "tool {} is not a dynamic Plan-mode target",
                call.tool_id
            ));
        }
        let server_id = required(&call.arguments, "server_id")?
            .parse::<Uuid>()
            .map_err(|_| "server_id must be a UUID")?;
        let tool = required(&call.arguments, "tool")?;
        let expected_catalog = required(&call.arguments, "catalog_sha256")?;
        let expected_schema = required(&call.arguments, "schema_sha256")?;
        let indexed = self
            .mcp_tool_index()
            .await?
            .into_iter()
            .find(|entry| entry.server_id == server_id && entry.tool_name == tool)
            .ok_or("MCP tool is not currently indexed")?;
        let run_approved = self.run_approved_plan_tool_call(call).await?;
        match authorize_mcp_read_only_target(
            &indexed,
            expected_catalog,
            expected_schema,
            run_approved,
        ) {
            Ok(()) => Ok(PlanToolAuthorizationV4::Allowed {
                effect: ToolEffectV4::ReadOnly,
            }),
            Err(KnowledgeErrorV4::McpToolNotApproved) if !run_approved => {
                Ok(PlanToolAuthorizationV4::RequiresApproval {
                    effect: ToolEffectV4::ReadOnly,
                    reason: "The third-party readOnlyHint is an unverified hint trusted by the user, not a host guarantee; approve this exact MCP read-only call only if you trust the configured server and arguments.".into(),
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }

    async fn execute_inner(
        &self,
        call: &ToolCallV4,
        require_read_only_hint: bool,
    ) -> Result<ToolOutcomeV4, String> {
        if call.tool_id == "browser_setup" || call.tool_id.starts_with("web_") {
            if require_read_only_hint {
                return Err("browser tools are forbidden in Plan mode".into());
            }
            return self.execute_browser_tool(call).await;
        }
        let (content, data, provenance) = match call.tool_id.as_str() {
            "agent.route_request" => {
                let requested_route = match required(&call.arguments, "route")? {
                    "research_retrieval" => AgentRequestRouteV4::ResearchRetrieval,
                    "adaptive" => AgentRequestRouteV4::Adaptive,
                    _ => return Err("route must be research_retrieval or adaptive".into()),
                };
                let route = self.forced_route.unwrap_or(requested_route);
                let route_name = match route {
                    AgentRequestRouteV4::ResearchRetrieval => "research_retrieval",
                    AgentRequestRouteV4::Adaptive => "adaptive",
                };
                let reason = required(&call.arguments, "reason")?;
                let requested_shape = match required(&call.arguments, "task_shape")? {
                    "fast" => "fast",
                    "multi_step" => "multi_step",
                    _ => return Err("task_shape must be fast or multi_step".into()),
                };
                let task_shape = if route == AgentRequestRouteV4::ResearchRetrieval {
                    "multi_step"
                } else {
                    requested_shape
                };
                (
                    format!(
                        "Host request route frozen as {route_name} with {task_shape} shape: {reason}"
                    ),
                    json!({"route":route_name,"task_shape":task_shape,"reason":reason,"host_classified":self.forced_route.is_some(),"host_promoted":requested_shape != task_shape}),
                    vec!["host-request-router-v4".into()],
                )
            }
            "agent.record_mcp_unavailable" => {
                let reason = required(&call.arguments, "reason")?;
                let searched_query = required(&call.arguments, "searched_query")?;
                (
                    format!("No callable professional MCP was available: {reason}"),
                    json!({
                        "reason":reason,
                        "searched_query":searched_query,
                        "candidate_count":call.arguments.get("candidate_count").and_then(Value::as_u64).unwrap_or(0)
                    }),
                    vec!["host-mcp-unavailability-observation-v4".into()],
                )
            }
            "project.list" => {
                let path = call
                    .arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                (
                    self.filesystem.list(path).await?,
                    json!({"path":path}),
                    vec![format!("project:{path}")],
                )
            }
            "project.read" => {
                let path = required(&call.arguments, "path")?;
                (
                    self.filesystem.read(path).await?,
                    json!({"path":path}),
                    vec![format!("project:{path}")],
                )
            }
            "search_skills" => {
                let query = required(&call.arguments, "query")?;
                let limit = call
                    .arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(8) as usize;
                let hits = search_skills(query, &self.skill_documents().await?, limit);
                (
                    serde_json::to_string(&hits).map_err(|error| error.to_string())?,
                    serde_json::to_value(&hits).map_err(|error| error.to_string())?,
                    vec![],
                )
            }
            "use_skill" => {
                let skill_id = required(&call.arguments, "skill_id")?
                    .parse::<Uuid>()
                    .map_err(|_| "skill_id must be a UUID")?;
                let sections = call
                    .arguments
                    .get("sections")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let document = self
                    .skill_documents()
                    .await?
                    .into_iter()
                    .find(|document| document.skill_id == skill_id)
                    .ok_or("enabled Skill was not found")?;
                let frozen =
                    freeze_skill(&document, &sections).map_err(|error| error.to_string())?;
                (
                    serde_json::to_string(&frozen).map_err(|error| error.to_string())?,
                    serde_json::to_value(&frozen).map_err(|error| error.to_string())?,
                    vec![
                        format!("skill-package-sha256:{}", frozen.package_sha256),
                        format!("skill-freeze-sha256:{}", frozen.frozen_sha256),
                    ],
                )
            }
            "search_memory" => {
                let query = required(&call.arguments, "query")?;
                let dimension = call.arguments.get("dimension").and_then(Value::as_str);
                let limit = call
                    .arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(12) as usize;
                let hits = search_memory(
                    query,
                    &self.memory_documents(dimension).await?,
                    Utc::now(),
                    limit,
                );
                (
                    serde_json::to_string(&hits).map_err(|error| error.to_string())?,
                    serde_json::to_value(&hits).map_err(|error| error.to_string())?,
                    hits.iter()
                        .map(|hit| {
                            format!(
                                "memory-source:{}:{}",
                                hit.document.source_kind, hit.document.source_id
                            )
                        })
                        .collect(),
                )
            }
            "search_mcp_tools" => {
                let query = required(&call.arguments, "query")?;
                let limit = call
                    .arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(8) as usize;
                let hits = search_mcp_tools(query, &self.mcp_tool_index().await?, limit);
                let needs_run_approval = hits.iter().any(|hit| {
                    hit.tool.configured
                        && hit.tool.enabled
                        && hit.tool.launch_approved
                        && !hit.tool.tool_approved
                });
                let guidance = if needs_run_approval {
                    "A matching MCP tool is configured and launch-approved but not persistently tool-approved. Call use_mcp_tool with its exact server_id, tool, catalog_sha256, schema_sha256, and arguments; the Host will request explicit schema+catalog-bound approval for this run. The third-party readOnlyHint is an unverified hint trusted by the user, not a Host guarantee. Do not replace literature MCP access with ad-hoc runtime HTTP code."
                } else {
                    "Call use_mcp_tool with the selected tool's exact server_id, tool, catalog_sha256, schema_sha256, and arguments. The third-party readOnlyHint is an unverified hint trusted by the user, not a Host guarantee."
                };
                let payload = json!({"tools":hits,"guidance":guidance});
                (
                    serde_json::to_string(&payload).map_err(|error| error.to_string())?,
                    payload,
                    vec![],
                )
            }
            "use_mcp_tool" => {
                let server_id = required(&call.arguments, "server_id")?
                    .parse::<Uuid>()
                    .map_err(|_| "server_id must be a UUID")?;
                let tool = required(&call.arguments, "tool")?;
                let expected_schema = required(&call.arguments, "schema_sha256")?;
                let mut indexed = self
                    .mcp_tool_index()
                    .await?
                    .into_iter()
                    .find(|entry| entry.server_id == server_id && entry.tool_name == tool)
                    .ok_or("MCP tool is not currently indexed")?;
                // Plan calls must carry the search snapshot explicitly. The
                // historical Execute path may omit catalog_sha256; in that
                // case bind it to the currently indexed profile snapshot.
                let expected_catalog = call
                    .arguments
                    .get("catalog_sha256")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        (!require_read_only_hint).then(|| indexed.tool_catalog_sha256.clone())
                    })
                    .ok_or_else(|| "catalog_sha256 is required for Plan MCP calls".to_string())?;
                let schema_bound_run_approved = if require_read_only_hint {
                    self.run_approved_plan_tool_call(call).await?
                } else {
                    self.run_approved_tool_call(call).await?
                };
                if !indexed.tool_approved && schema_bound_run_approved {
                    indexed.tool_approved = true;
                }
                if require_read_only_hint {
                    authorize_mcp_read_only_target(
                        &indexed,
                        &expected_catalog,
                        expected_schema,
                        schema_bound_run_approved,
                    )
                    .map_err(|error| error.to_string())?;
                } else {
                    authorize_mcp_use(&indexed, &expected_catalog, expected_schema)
                        .map_err(|error| error.to_string())?;
                }
                let result = invoke_configured_mcp_tool_v4(
                    &self.repository,
                    &self.mcp_sessions,
                    &self.credentials,
                    self.project_id,
                    server_id,
                    tool,
                    call.arguments
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                    expected_catalog,
                    expected_schema.into(),
                    require_read_only_hint,
                    schema_bound_run_approved,
                )
                .await?;
                let value = result.result.unwrap_or_else(|| json!({}));
                (
                    serde_json::to_string(&value).map_err(|error| error.to_string())?,
                    json!({
                        "server_id":server_id,
                        "tool":tool,
                        "catalog_sha256":indexed.tool_catalog_sha256,
                        "schema_sha256":indexed.schema_sha256,
                        "audit_id":result.audit_id,
                        "result":value
                    }),
                    vec![format!("mcp-audit:{}", result.audit_id)],
                )
            }
            "runtime.execute" => {
                let language = parse_language(required(&call.arguments, "language")?)?;
                let environment = call
                    .arguments
                    .get("environment")
                    .and_then(Value::as_str)
                    .unwrap_or("system");
                let code = required(&call.arguments, "code")?.to_owned();
                let captures = call
                    .arguments
                    .get("capture_paths")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                validate_kernel_code(&code).map_err(|e| e.to_string())?;
                validate_capture_paths(&captures).map_err(|e| e.to_string())?;
                let key = self.key(language, environment)?;
                let (running, mut result) = crate::runtime_jobs_v4::execute_reserved(
                    &self.repository,
                    &self.runtime,
                    &key,
                    &call,
                    code,
                    captures,
                )
                .await?;
                result.software_versions = self
                    .software_versions(
                        language,
                        environment,
                        call.arguments
                            .get("analysis")
                            .and_then(|analysis| analysis.get("software_requirements"))
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str),
                    )
                    .await;
                self.repository
                    .advance_runtime_job_v4(
                        &running,
                        if result.succeeded {
                            omicsops_protocol::RuntimeJobStateV4::Succeeded
                        } else {
                            omicsops_protocol::RuntimeJobStateV4::Failed
                        },
                        None,
                        Some(&result),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                return crate::runtime_jobs_v4::outcome(call, &running, &result);
            }
            "runtime.environment.ensure" => {
                let language = parse_language(required(&call.arguments, "language")?)?;
                let environment = required(&call.arguments, "environment")?;
                validate_environment_name(environment)?;
                if environment != self.selection.environment {
                    return Err("environment ensure does not match the frozen selection".into());
                }
                let excerpt = self.environment_port.ensure(language, environment).await?;
                (
                    excerpt,
                    json!({"environment":environment,"language":language}),
                    vec![format!("environment:{environment}")],
                )
            }
            "runtime.rebuild" => {
                let language = parse_language(required(&call.arguments, "language")?)?;
                let environment = call
                    .arguments
                    .get("environment")
                    .and_then(Value::as_str)
                    .unwrap_or("system");
                let key = self.key(language, environment)?;
                let session = self.runtime.rebuild(&key).await?;
                (
                    format!(
                        "rebuilt kernel session={} process={}",
                        session.session_id(),
                        session.process_identity()
                    ),
                    json!({"session_id":session.session_id(),"process_identity":session.process_identity(),"language":language,"environment":environment}),
                    vec![format!("kernel-session:{}", session.session_id())],
                )
            }
            "runtime.interrupt" => {
                let language = parse_language(required(&call.arguments, "language")?)?;
                let environment = call
                    .arguments
                    .get("environment")
                    .and_then(Value::as_str)
                    .unwrap_or("system");
                let key = self.key(language, environment)?;
                self.runtime.interrupt(&key).await?;
                (
                    format!("interrupted {language:?} kernel in {environment}"),
                    json!({"language":language,"environment":environment}),
                    vec![],
                )
            }
            "science.register_dataset" => {
                let path = required(&call.arguments, "path")?;
                let verified = self.filesystem.verify_file(path).await?;
                let size = verified.size_bytes;
                let hash = verified.sha256;
                let data = json!({
                    "path": path,
                    "size_bytes": size,
                    "sha256": &hash,
                    "modality": call.arguments.get("modality"),
                    "species": call.arguments.get("species"),
                    "sample_ids": call.arguments.get("sample_ids"),
                    "matrix_shape": call.arguments.get("matrix_shape"),
                    "stage": call.arguments.get("stage"),
                });
                (
                    format!("registered verified dataset {path}: {size} bytes sha256={hash}"),
                    data,
                    vec![format!("sha256:{hash}")],
                )
            }
            "science.record_evidence" => (
                "evidence declaration accepted for Host validation".into(),
                json!({"accepted":true}),
                vec![],
            ),
            "artifact.verify" => {
                let path = required(&call.arguments, "path")?;
                let verified = self.filesystem.verify_file(path).await?;
                let size = verified.size_bytes;
                let hash = verified.sha256;
                (
                    format!("verified {path}: {size} bytes sha256={hash}"),
                    json!({"path":path,"size_bytes":size,"sha256":&hash}),
                    vec![format!("sha256:{hash}")],
                )
            }
            _ => {
                return Err(format!(
                    "coordinator tool {} cannot execute in runtime",
                    call.tool_id
                ));
            }
        };
        Ok(ToolOutcomeV4 {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            succeeded: true,
            model_content: content,
            data,
            provenance,
        })
    }
}

#[async_trait]
impl ToolExecutorV4 for DesktopToolExecutorV4 {
    async fn recover_result(&self, call: &ToolCallV4) -> Result<Option<ToolOutcomeV4>, String> {
        if call.tool_id != "runtime.execute" { return Ok(None); }
        let language = parse_language(required(&call.arguments, "language")?)?;
        let environment = call.arguments.get("environment").and_then(Value::as_str).unwrap_or("system");
        let key = self.key(language, environment)?;
        self.repository.recover_runtime_result_v4(&key, call).await.map_err(|error| error.to_string())?
            .map(|(job, result)| crate::runtime_jobs_v4::outcome(call, &job, &result)).transpose()
    }
    async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
        self.execute_inner(call, false).await
    }

    fn has_persistent_authorization(&self, call: &ToolCallV4) -> bool {
        if call.tool_id != "browser_setup" && !call.tool_id.starts_with("web_") {
            return false;
        }
        self.matching_browser_authorization(call).is_some()
    }

    async fn authorize_plan_call(
        &self,
        call: &ToolCallV4,
    ) -> Result<PlanToolAuthorizationV4, String> {
        self.authorize_plan_target(call).await
    }

    async fn execute_plan(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
        self.execute_inner(call, true).await
    }

    async fn interrupt(&self, run_id: Uuid) -> Result<(), String> {
        self.runtime.interrupt_run(run_id).await?;
        for session in [
            omicsops_browser::BrowserSessionKind::Shared,
            omicsops_browser::BrowserSessionKind::Workspace,
        ] {
            let _ = self.browser.close_run_tabs(session, run_id).await;
        }
        Ok(())
    }
}

fn browser_session_for_arguments(
    arguments: &Value,
) -> Result<omicsops_browser::BrowserSessionKind, String> {
    match arguments
        .get("session")
        .and_then(Value::as_str)
        .unwrap_or("workspace")
    {
        "shared" => Ok(omicsops_browser::BrowserSessionKind::Shared),
        "workspace" => Ok(omicsops_browser::BrowserSessionKind::Workspace),
        _ => Err("browser session must be shared or workspace".into()),
    }
}

struct SshKernelBackendV4 {
    session: Arc<SshSession>,
    root: String,
    project_id: Uuid,
    backend_id: String,
}
#[async_trait]
impl KernelBackendV4 for SshKernelBackendV4 {
    fn descriptor(&self) -> ComputeBackendDescriptorV4 {
        ComputeBackendDescriptorV4 {
            schema_version: 4,
            backend_id: self.backend_id.clone(),
            kind: ComputeBackendKindV4::Ssh,
            isolation: IsolationStrengthV4::Process,
            available: true,
            supports_python: true,
            supports_r: true,
            supports_network_policy: false,
        }
    }

    async fn launch(
        &self,
        key: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn KernelProcessV4>, String> {
        if key.backend_id != self.backend_id {
            return Err("SSH backend received a mismatched backend id".into());
        }
        validate_environment_name(&key.environment)?;
        let session_id = Uuid::new_v4();
        let dir = format!("{}/.omicsops/kernels", self.root.trim_end_matches('/'));
        self.session
            .execute_checked(&format!("mkdir -p {}", shell_quote(&dir)))
            .await
            .map_err(|e| e.to_string())?;
        let (driver_language, extension, executable) = match key.language {
            KernelLanguageV4::Python => (KernelLanguage::Python, "py", "python -u"),
            KernelLanguageV4::R => (KernelLanguage::R, "R", "Rscript --vanilla"),
        };
        let driver = format!("{dir}/v4-{session_id}.driver.{extension}");
        self.session
            .upload_text(&driver, kernel_driver(driver_language))
            .await
            .map_err(|e| e.to_string())?;
        let executable = if key.environment == "system" {
            executable.to_owned()
        } else {
            let prefix = environment_path(&self.root, &key.environment);
            let probe = self
                .session
                .execute_checked(&format!(
                    "test -d {0}/conda-meta && command -v micromamba >/dev/null",
                    shell_quote(&prefix)
                ))
                .await;
            if probe.is_err() {
                return Err(format!(
                    "project environment {} is missing; call runtime.environment.ensure first",
                    key.environment
                ));
            }
            format!(
                "micromamba run --prefix {} {executable}",
                shell_quote(&prefix)
            )
        };
        let command = format!(
            "{executable} {} {} {} {}",
            shell_quote(&driver),
            shell_quote(&self.root),
            shell_quote(&self.project_id.to_string()),
            shell_quote(&session_id.to_string())
        );
        let process = self
            .session
            .open_jsonl_process(&command)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(SshKernelProcessV4 {
            id: session_id,
            identity: format!("ssh-jsonl:{:?}:{session_id}", key.language).to_ascii_lowercase(),
            project_id: self.project_id,
            session: self.session.clone(),
            output_dir: format!(
                "{}/.omicsops/runs/{}/outputs",
                self.root.trim_end_matches('/'),
                key.run_id
            ),
            process: Mutex::new(Some(process)),
        }))
    }
}

struct SshKernelProcessV4 {
    id: Uuid,
    identity: String,
    project_id: Uuid,
    session: Arc<SshSession>,
    output_dir: String,
    process: Mutex<Option<SshJsonlProcess>>,
}
#[async_trait]
impl KernelProcessV4 for SshKernelProcessV4 {
    fn session_id(&self) -> Uuid {
        self.id
    }
    fn process_identity(&self) -> &str {
        &self.identity
    }
    async fn execute(
        &self,
        code: String,
        capture_paths: Vec<String>,
    ) -> Result<RuntimeResultV4, String> {
        let request_id = Uuid::new_v4();
        let mut guard = self.process.lock().await;
        let process = guard.as_mut().ok_or("kernel is stopped")?;
        process
            .send(&KernelRequest::Execute {
                session_id: self.id,
                request_id,
                code,
                capture_paths,
            })
            .await
            .map_err(|e| e.to_string())?;
        let mut decoder = KernelEventDecoder::new(self.project_id, self.id, request_id);
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut artifacts = Vec::new();
        let mut succeeded = false;
        loop {
            let event: KernelEvent = process
                .receive()
                .await
                .map_err(|e| e.to_string())?
                .ok_or("kernel disconnected")?;
            let event = decoder.accept(event).map_err(|e| e.to_string())?;
            match event.event {
                KernelEventKind::Stdout(content) => stdout.push_str(&content),
                KernelEventKind::Stderr(content) => stderr.push_str(&content),
                KernelEventKind::Artifact {
                    relative_path,
                    size_bytes,
                    sha256,
                } => artifacts.push(RuntimeArtifactV4 {
                    relative_path,
                    size_bytes,
                    sha256,
                }),
                KernelEventKind::Completed => {
                    succeeded = true;
                    break;
                }
                KernelEventKind::Failed { message } => {
                    stderr.push_str(&message);
                    break;
                }
                _ => {}
            }
        }
        self.session
            .execute_checked(&format!("mkdir -p {}", shell_quote(&self.output_dir)))
            .await
            .map_err(|e| e.to_string())?;
        let stdout_capture = self.archive_output(request_id, "stdout", &stdout).await?;
        let stderr_capture = self.archive_output(request_id, "stderr", &stderr).await?;
        Ok(RuntimeResultV4 {
            request_id,
            session_id: self.id,
            process_identity: self.identity.clone(),
            stdout: stdout_capture.excerpt.clone(),
            stderr: stderr_capture.excerpt.clone(),
            stdout_capture: Some(stdout_capture),
            stderr_capture: Some(stderr_capture),
            succeeded,
            artifacts,
            software_versions: BTreeMap::new(),
        })
    }
    async fn interrupt(&self) -> Result<(), String> {
        if let Some(process) = self.process.lock().await.take() {
            process.shutdown().await.map_err(|e| e.to_string())?
        }
        Ok(())
    }
}

impl SshKernelProcessV4 {
    async fn archive_output(
        &self,
        request_id: Uuid,
        stream: &str,
        contents: &str,
    ) -> Result<OutputCaptureV4, String> {
        let remote_path = format!("{}/{}.{}.txt", self.output_dir, request_id, stream);
        self.session
            .upload_text(&remote_path, contents)
            .await
            .map_err(|error| error.to_string())?;
        let (excerpt, truncated) = bounded_excerpt(contents, 16 * 1024);
        let marker = "/.omicsops/";
        let archive_path = remote_path
            .find(marker)
            .map(|index| remote_path[index + 1..].to_owned())
            .unwrap_or(remote_path);
        Ok(OutputCaptureV4 {
            excerpt,
            total_bytes: contents.len() as u64,
            sha256: hex::encode(sha2::Sha256::digest(contents.as_bytes())),
            archive_path,
            truncated,
        })
    }
}

struct RepositoryEventStoreV4 {
    repository: Store,
    app: AppHandle,
}
#[tauri::command]
pub async fn agent_v4_submit_guidance(
    app: AppHandle,
    state: State<'_, AppState>,
    request: omicsops_dto::SubmitGuidanceV4Request,
) -> Result<omicsops_dto::GuidanceRecordV4, String> {
    let record = state
        .repository
        .accept_guidance_v4(&request)
        .await
        .map_err(|error| error.to_string())?;
    let active = state
        .active_runs
        .lock()
        .map_err(|_| "active run registry unavailable")?
        .contains_key(&request.run_id);
    if !active && record.consumed_at.is_none() {
        // A restart can leave a durable running record without a live driver.
        // Resume uses the same idempotent active slot as explicit UI resumes.
        if let Err(error) = agent_v4_resume(app, state, request.run_id).await {
            eprintln!("accepted guidance retained; automatic resume failed: {error}");
        }
    }
    Ok(record)
}

#[tauri::command]
pub async fn agent_v4_list_guidance(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
) -> Result<Vec<omicsops_dto::GuidanceRecordV4>, String> {
    state
        .repository
        .list_guidance_v4(project_id, conversation_id, run_id)
        .await
        .map_err(|error| error.to_string())
}
impl RepositoryEventStoreV4 {
    fn publish(&self, event: &AgentEventV4, message: Option<omicsops_core::workspace::Message>) {
        // Persistence is the source of truth. A closed/stale Tauri listener
        // must not make the agent retry a committed event or report a command
        // failure; the next reconciliation/hydration reads it from Store.
        if let Err(error) = self.app.emit(AGENT_V4_EVENT_CHANNEL, event) {
            eprintln!("failed to broadcast committed V4 event: {error}");
        }
        if let Some(message) = message {
            if let Err(error) = self.app.emit(
                "conversation-event",
                crate::agent_commands::ConversationEvent {
                    project_id: message.project_id,
                    conversation_id: message.conversation_id,
                    message,
                },
            ) {
                eprintln!("failed to broadcast committed conversation event: {error}");
            }
        }
    }
}
#[async_trait]
impl EventStoreV4 for RepositoryEventStoreV4 {
    async fn append(&self, event: &AgentEventV4) -> Result<(), String> {
        let message = self
            .repository
            .append_agent_event_v4_with_conversation(event)
            .await
            .map_err(|error| error.to_string())?;
        self.publish(event, message);
        Ok(())
    }
    async fn append_completion(&self, event: &AgentEventV4) -> Result<bool, String> {
        match self
            .repository
            .append_agent_event_v4_with_conversation(event)
            .await
        {
            Ok(message) => {
                self.publish(event, message);
                Ok(true)
            }
            Err(omicsops_store::StoreError::GuidancePending) => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }
    async fn consume_guidance(&self, spec: &RunSpecV4) -> Result<bool, String> {
        let events = self
            .repository
            .consume_guidance_v4(spec)
            .await
            .map_err(|error| error.to_string())?;
        for event in &events {
            self.publish(event, None);
        }
        Ok(!events.is_empty())
    }
    async fn has_pending_guidance(&self, run_id: Uuid) -> Result<bool, String> {
        self.repository
            .has_pending_guidance_v4(run_id)
            .await
            .map_err(|error| error.to_string())
    }
    async fn load(&self, run_id: Uuid) -> Result<Vec<AgentEventV4>, String> {
        self.repository
            .agent_events_v4(run_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn archive_context(
        &self,
        run_id: Uuid,
        transcript: &str,
        checkpoint: &ContextCheckpointV4,
    ) -> Result<ContextArchiveV4, String> {
        self.repository
            .archive_agent_context_v4(run_id, transcript, checkpoint)
            .await
            .map_err(|error| error.to_string())
    }
}

fn scientific_state_lock_v4(project_id: Uuid) -> Arc<Mutex<()>> {
    let locks = SCIENTIFIC_STATE_LOCKS_V4.get_or_init(Default::default);
    let mut locks = locks.lock().expect("V4 scientific state lock registry");
    if let Some(lock) = locks.get(&project_id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(project_id, Arc::downgrade(&lock));
    lock
}

struct RepositoryScientificStateStoreV4 {
    repository: Store,
    backend_id: String,
    mutation_lock: Arc<Mutex<()>>,
}

impl RepositoryScientificStateStoreV4 {
    async fn load(&self, project_id: Uuid) -> Result<ScientificStateV4, String> {
        Ok(self
            .repository
            .scientific_state_v4(project_id)
            .await
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| ScientificStateV4::new(project_id)))
    }

    async fn save_update(
        &self,
        state: &ScientificStateV4,
        changes: Vec<String>,
    ) -> Result<Option<ScientificUpdateV4>, String> {
        self.repository
            .save_scientific_state_v4(state)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Some(ScientificUpdateV4 {
            revision: state.revision,
            state_sha256: state.digest(),
            changes,
        }))
    }
}

#[async_trait]
impl ScientificStateStoreV4 for RepositoryScientificStateStoreV4 {
    async fn snapshot(&self, project_id: Uuid) -> Result<ScientificStateV4, String> {
        self.load(project_id).await
    }

    async fn before_tool(
        &self,
        project_id: Uuid,
        run_id: Uuid,
        call: &ToolCallV4,
    ) -> Result<Option<ScientificUpdateV4>, String> {
        if call.tool_id != "runtime.execute" || call.arguments.get("analysis").is_none() {
            return Ok(None);
        }
        let _guard = self.mutation_lock.lock().await;
        let mut state = self.load(project_id).await?;
        if state
            .analyses
            .values()
            .any(|analysis| analysis.source_call_id == call.call_id)
        {
            return Ok(None);
        }
        let declaration: AnalysisDeclarationV4 = serde_json::from_value(
            call.arguments
                .get("analysis")
                .cloned()
                .ok_or("analysis declaration is missing")?,
        )
        .map_err(|error| format!("invalid analysis declaration: {error}"))?;
        let language = required(&call.arguments, "language")?.to_ascii_lowercase();
        let environment = call
            .arguments
            .get("environment")
            .and_then(Value::as_str)
            .unwrap_or("system")
            .to_owned();
        let analysis = state
            .start_analysis(
                run_id,
                call.call_id.clone(),
                declaration,
                RuntimeIdentityV4 {
                    backend_id: self.backend_id.clone(),
                    language,
                    environment,
                    session_id: None,
                    process_identity: None,
                },
                Utc::now(),
            )
            .map_err(|error| error.to_string())?;
        self.save_update(&state, vec![format!("analysis_started:{}", analysis.id)])
            .await
    }

    async fn after_tool(
        &self,
        project_id: Uuid,
        _run_id: Uuid,
        call: &ToolCallV4,
        outcome: &ToolOutcomeV4,
    ) -> Result<Option<ScientificUpdateV4>, String> {
        if !matches!(
            call.tool_id.as_str(),
            "runtime.execute" | "science.register_dataset" | "science.record_evidence"
        ) {
            return Ok(None);
        }
        let _guard = self.mutation_lock.lock().await;
        let mut state = self.load(project_id).await?;
        match call.tool_id.as_str() {
            "science.register_dataset" if outcome.succeeded => {
                let path = required(&outcome.data, "path")?;
                let hash = required(&outcome.data, "sha256")?;
                if state.datasets.values().any(|dataset| {
                    dataset.active && dataset.relative_path == path && dataset.sha256 == hash
                }) {
                    return Ok(None);
                }
                let stage: DatasetStageV4 = serde_json::from_value(
                    outcome
                        .data
                        .get("stage")
                        .cloned()
                        .ok_or("dataset stage missing")?,
                )
                .map_err(|error| error.to_string())?;
                let samples = serde_json::from_value::<BTreeSet<String>>(
                    outcome
                        .data
                        .get("sample_ids")
                        .cloned()
                        .ok_or("sample_ids missing")?,
                )
                .map_err(|error| error.to_string())?;
                let matrix_shape = serde_json::from_value::<Vec<u64>>(
                    outcome
                        .data
                        .get("matrix_shape")
                        .cloned()
                        .ok_or("matrix_shape missing")?,
                )
                .map_err(|error| error.to_string())?;
                let dataset = state.register_dataset(
                    VerifiedDatasetFactV4 {
                        modality: required(&outcome.data, "modality")?.into(),
                        species: required(&outcome.data, "species")?.into(),
                        sample_ids: samples,
                        matrix_shape,
                        stage,
                        relative_path: path.into(),
                        size_bytes: outcome
                            .data
                            .get("size_bytes")
                            .and_then(Value::as_u64)
                            .ok_or("dataset size missing")?,
                        sha256: hash.into(),
                    },
                    Utc::now(),
                );
                self.save_update(&state, vec![format!("dataset_registered:{}", dataset.id)])
                    .await
            }
            "runtime.execute" if call.arguments.get("analysis").is_some() => {
                if !state.analyses.values().any(|analysis| {
                    analysis.source_call_id == call.call_id
                        && analysis.status == AnalysisStatusV4::Running
                }) {
                    return Ok(None);
                }
                let result: RuntimeResultV4 = serde_json::from_value(outcome.data.clone())
                    .map_err(|error| format!("invalid runtime result: {error}"))?;
                let artifacts = result
                    .artifacts
                    .iter()
                    .map(|artifact| VerifiedArtifactFactV4 {
                        artifact_type: artifact_type_for_path(&artifact.relative_path),
                        relative_path: artifact.relative_path.clone(),
                        size_bytes: artifact.size_bytes,
                        sha256: artifact.sha256.clone(),
                        preview: None,
                        metadata: json!({"request_id":result.request_id}),
                    })
                    .collect();
                let (analysis, created, manifest) = state
                    .finish_analysis(
                        &call.call_id,
                        outcome.succeeded && result.succeeded,
                        Some(result.session_id),
                        Some(result.process_identity),
                        artifacts,
                        result.software_versions,
                        required(&call.arguments, "code")?.into(),
                        Utc::now(),
                    )
                    .map_err(|error| error.to_string())?;
                self.save_update(
                    &state,
                    vec![
                        format!("analysis_finished:{}:{:?}", analysis.id, analysis.status),
                        format!("artifacts_registered:{}", created.len()),
                        format!("provenance_recorded:{}", manifest.id),
                    ],
                )
                .await
            }
            "science.record_evidence" if outcome.succeeded => {
                if state
                    .evidence
                    .values()
                    .any(|evidence| evidence.source_call_id == call.call_id)
                {
                    return Ok(None);
                }
                let declaration: EvidenceDeclarationV4 =
                    serde_json::from_value(call.arguments.clone())
                        .map_err(|error| format!("invalid evidence declaration: {error}"))?;
                let evidence = state
                    .record_evidence(call.call_id.clone(), declaration, Utc::now())
                    .map_err(|error| error.to_string())?;
                self.save_update(&state, vec![format!("evidence_recorded:{}", evidence.id)])
                    .await
            }
            _ => Ok(None),
        }
    }
}

fn artifact_type_for_path(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_else(|| "file".into())
}

async fn append_next(
    store: &RepositoryEventStoreV4,
    run_id: Uuid,
    event: AgentEventKindV4,
) -> Result<(), String> {
    let events = store.load(run_id).await?;
    let previous = events.last().ok_or("V4 run has no event")?;
    store
        .append(&AgentEventV4::next(previous, Utc::now(), event))
        .await
}
async fn save_record(repository: &Store, record: &RunRecordV4) -> Result<(), String> {
    repository
        .save_agent_run_v4(
            record.run_id,
            record.project_id,
            record.conversation_id,
            &record.status,
            &serde_json::to_value(record).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())
}
async fn load_record(repository: &Store, run_id: Uuid) -> Result<RunRecordV4, String> {
    serde_json::from_value(
        repository
            .agent_run_v4(run_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("V4 run not found")?,
    )
    .map_err(|e| e.to_string())
}

async fn workspace_project(repository: &Store, project_id: Uuid) -> Result<Project, String> {
    repository
        .get_project(project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project does not exist".to_string())
}

fn legacy_ssh_selection(project: &Project) -> Result<ComputeSelectionV4, String> {
    let connection_id = project
        .connection_id
        .ok_or("legacy V4 run has no SSH project binding")?;
    Ok(ComputeSelectionV4 {
        schema_version: 4,
        backend_id: format!("ssh:{connection_id}"),
        backend_kind: ComputeBackendKindV4::Ssh,
        autonomy_mode: AutonomyModeV4::Supervised,
        approval_policy: ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: NetworkPolicyV4::HostInherited,
        container_image: None,
    })
}

async fn program_available(program: &str) -> bool {
    background_command(program)
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .is_ok_and(|output| output.status.success())
}

async fn inspect_container_image(program: &str, image: &str) -> (Option<String>, Option<String>) {
    if image.trim().is_empty() || image.chars().any(char::is_whitespace) {
        return (None, Some("container image reference is invalid".into()));
    }
    match background_command(program)
        .args(container_image_inspect_args(image))
        .kill_on_drop(true)
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if id.is_empty() {
                (
                    None,
                    Some("container image inspect returned no image ID".into()),
                )
            } else {
                (Some(id), None)
            }
        }
        Ok(output) => (
            None,
            Some(format!(
                "container image is not available locally: {}",
                bounded_excerpt(&String::from_utf8_lossy(&output.stderr), 2048)
                    .0
                    .trim()
            )),
        ),
        Err(error) => (
            None,
            Some(format!("container engine probe failed: {error}")),
        ),
    }
}

fn container_image_inspect_args(image: &str) -> [&str; 5] {
    ["image", "inspect", "--format", "{{.Id}}", image]
}

async fn validate_compute_selection(
    state: &AppState,
    project: &Project,
    selection: &ComputeSelectionV4,
) -> Result<(), String> {
    selection.validate().map_err(|error| error.to_string())?;
    match selection.backend_kind {
        ComputeBackendKindV4::Local => {
            std::fs::canonicalize(&project.local_root)
                .map_err(|error| format!("local project root is unavailable: {error}"))?;
            if !program_available("python").await && !program_available("Rscript").await {
                return Err("local backend requires Python or R".into());
            }
        }
        ComputeBackendKindV4::Ssh => {
            let connection_id = project
                .connection_id
                .ok_or("project has no remote connection")?;
            if selection.backend_id != format!("ssh:{connection_id}") {
                return Err("SSH selection does not match the project's trusted binding".into());
            }
            let profile = find_profile(&state.repository, connection_id).await?;
            require_trusted_host(&profile)?;
            if project.remote_root.is_none() {
                return Err("project has no remote root".into());
            }
        }
        ComputeBackendKindV4::Docker | ComputeBackendKindV4::Podman => {
            let image = selection
                .container_image
                .as_ref()
                .ok_or("container image is missing")?;
            let program = if selection.backend_kind == ComputeBackendKindV4::Docker {
                "docker"
            } else {
                "podman"
            };
            let (actual, error) = inspect_container_image(program, &image.reference).await;
            if actual.as_deref() != Some(image.image_id.as_str()) {
                return Err(error.unwrap_or_else(|| {
                    "container image tag no longer resolves to the frozen image ID".into()
                }));
            }
        }
    }
    Ok(())
}

async fn validate_frozen_spec(repository: &Store, spec: &RunSpecV4) -> Result<(), String> {
    spec.validate_integrity()
        .map_err(|error| error.to_string())?;
    let expected = spec.spec_hash.as_deref().ok_or("V4 spec hash is missing")?;
    let anchored = repository
        .agent_events_v4(spec.run_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .rev()
        .find_map(|event| match event.event {
            AgentEventKindV4::RunSpecFrozen { spec_hash, .. } => Some(spec_hash),
            _ => None,
        })
        .ok_or("V4 spec hash is not anchored in the event chain")?;
    if anchored != expected {
        return Err("V4 spec differs from the event-chain approval anchor".into());
    }
    Ok(())
}

fn verified_file_from_output(output: &str) -> Result<VerifiedProjectFileV4, String> {
    let mut lines = output.lines();
    let size_bytes = lines
        .next()
        .ok_or("missing verified file size")?
        .parse::<u64>()
        .map_err(|_| "invalid verified file size")?;
    let sha256 = lines
        .next()
        .and_then(|line| line.split_whitespace().next())
        .ok_or("missing verified file hash")?
        .to_owned();
    Ok(VerifiedProjectFileV4 { size_bytes, sha256 })
}

fn software_version_program(
    language: KernelLanguageV4,
    requirements: &[String],
) -> Result<(&'static str, &'static str, String), String> {
    match language {
        KernelLanguageV4::Python => {
            let names = serde_json::to_string(requirements).map_err(|error| error.to_string())?;
            Ok((
                "python",
                "-c",
                format!(
                    "import importlib.metadata as m,platform\nnames={names}\nprint('python\\t'+platform.python_version())\nfor n in names:\n try: print(n+'\\t'+m.version(n))\n except m.PackageNotFoundError: pass"
                ),
            ))
        }
        KernelLanguageV4::R => {
            let names = requirements
                .iter()
                .map(|name| serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into()))
                .collect::<Vec<_>>()
                .join(",");
            Ok((
                "Rscript",
                "-e",
                format!(
                    "cat('R\\t',R.version.string,'\\n',sep=''); for (n in c({names})) if (requireNamespace(n,quietly=TRUE)) cat(n,'\\t',as.character(packageVersion(n)),'\\n',sep='')"
                ),
            ))
        }
    }
}

fn software_version_command(
    language: KernelLanguageV4,
    environment_prefix: Option<&str>,
    requirements: &[String],
) -> Result<String, String> {
    let executable_prefix = environment_prefix
        .map(|prefix| format!("micromamba run --prefix {} ", shell_quote(prefix)))
        .unwrap_or_default();
    match language {
        KernelLanguageV4::Python => {
            let names = serde_json::to_string(requirements).map_err(|error| error.to_string())?;
            let code = format!(
                "import importlib.metadata as m,platform\nnames={names}\nprint('python\\t'+platform.python_version())\nfor n in names:\n try: print(n+'\\t'+m.version(n))\n except m.PackageNotFoundError: pass"
            );
            Ok(format!(
                "{executable_prefix}python -c {}",
                shell_quote(&code)
            ))
        }
        KernelLanguageV4::R => {
            let names = requirements
                .iter()
                .map(|name| serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into()))
                .collect::<Vec<_>>()
                .join(",");
            let code = format!(
                "cat('R\\t',R.version.string,'\\n',sep=''); for (n in c({names})) if (requireNamespace(n,quietly=TRUE)) cat(n,'\\t',as.character(packageVersion(n)),'\\n',sep='')"
            );
            Ok(format!(
                "{executable_prefix}Rscript -e {}",
                shell_quote(&code)
            ))
        }
    }
}

async fn software_versions_from_command(
    language: KernelLanguageV4,
    _: Option<&str>,
    requirements: Vec<String>,
) -> Result<BTreeMap<String, String>, String> {
    let (program, flag, code) = software_version_program(language, &requirements)?;
    let output = background_command(program)
        .args([flag, &code])
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(parse_software_versions(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_software_versions(output: &str) -> BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, version)| (name.to_owned(), version.trim().to_owned()))
        .collect()
}
async fn resolve_root(session: &SshSession, configured: &str) -> Result<String, String> {
    let out = session
        .execute_checked(&format!(
            "root=$(realpath -- {}) && test -d \"$root\" && printf '%s' \"$root\"",
            shell_quote(configured)
        ))
        .await
        .map_err(|e| e.to_string())?;
    let root = out.stdout.trim().to_owned();
    if root.is_empty() {
        Err("remote root did not resolve".into())
    } else {
        Ok(root)
    }
}

async fn load_prompt_layers(
    session: &SshSession,
    root: &str,
    backend_id: &str,
) -> Result<PromptLayersV4, String> {
    async fn read_optional(session: &SshSession, path: &str) -> Result<String, String> {
        let output = session
            .execute_checked(&format!(
                "if test -f {0}; then sed -n '1,800p' {0}; fi",
                shell_quote(path)
            ))
            .await
            .map_err(|error| error.to_string())?;
        let (excerpt, _) = bounded_excerpt(&output.stdout, 32 * 1024);
        Ok(excerpt)
    }

    let agents = read_optional(
        session,
        &format!("{}/AGENTS.md", root.trim_end_matches('/')),
    )
    .await?;
    let override_rules = read_optional(
        session,
        &format!("{}/.omicsops/AGENT.md", root.trim_end_matches('/')),
    )
    .await?;
    let mut layers = PromptLayersV4::default();
    layers.project_rules = project_rules_layer(&agents, &override_rules);
    layers.environment = format!(
        "backend={backend_id}; persistent Python/R kernels; project-relative paths only; credentials remain Host references"
    );
    Ok(layers)
}

fn project_rules_layer(agents: &str, override_rules: &str) -> String {
    match (agents.trim().is_empty(), override_rules.trim().is_empty()) {
        (true, true) => "No project-specific rules were found.".into(),
        (false, true) => format!("[BASE AGENTS.md]\n{}", agents.trim()),
        (true, false) => format!(
            "[HIGHER PRIORITY .omicsops/AGENT.md]\n{}",
            override_rules.trim()
        ),
        (false, false) => format!(
            "[BASE AGENTS.md]\n{}\n\n[HIGHER PRIORITY .omicsops/AGENT.md; wins on conflict]\n{}",
            agents.trim(),
            override_rules.trim()
        ),
    }
}

fn parse_language(value: &str) -> Result<KernelLanguageV4, String> {
    match value.to_ascii_lowercase().as_str() {
        "python" | "py" => Ok(KernelLanguageV4::Python),
        "r" => Ok(KernelLanguageV4::R),
        _ => Err(format!("unsupported kernel language: {value}")),
    }
}

fn validate_environment_name(value: &str) -> Result<(), String> {
    if value == "system"
        || (!value.is_empty()
            && value.len() <= 64
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character)))
    {
        Ok(())
    } else {
        Err("environment must be 'system' or a safe project environment name".into())
    }
}

fn environment_path(root: &str, environment: &str) -> String {
    format!(
        "{}/.omicsops/environments/{environment}",
        root.trim_end_matches('/')
    )
}

fn bounded_excerpt(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_owned(), false);
    }
    let half = max_bytes / 2;
    let mut head_end = half.min(value.len());
    while !value.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = value.len().saturating_sub(half);
    while !value.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    (
        format!(
            "{}\n… <{} bytes archived; middle omitted> …\n{}",
            &value[..head_end],
            value.len(),
            &value[tail_start..]
        ),
        true,
    )
}

fn project_path(root: &str, relative: &str) -> Result<String, String> {
    let raw = relative.trim().replace('\\', "/");
    if raw.starts_with('/') || raw.split('/').any(|p| p == "..") {
        return Err("path must remain project-relative".into());
    }
    let path = if raw.is_empty() || raw == "." {
        root.trim_end_matches('/').into()
    } else {
        format!(
            "{}/{}",
            root.trim_end_matches('/'),
            raw.trim_start_matches("./")
        )
    };
    if path != root.trim_end_matches('/') {
        require_remote_descendant(root, &path).map_err(|e| e.to_string())?
    }
    Ok(path)
}
fn relative_listing(stdout: &str, root: &str) -> String {
    stdout.replace(&format!("{}/", root.trim_end_matches('/')), "")
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_adapters::{llm::ProviderProtocol, ssh::SshAuthentication};
    use omicsops_core::domain::{AuthenticationMethod, ConnectionProfile};
    use omicsops_protocol::{
        AgentInputReasonV4, RunModeV4, ToolApprovalDecisionV4, ToolApprovalRequestV4,
        ToolDescriptorV4, ToolEffectV4,
    };
    use url::Url;

    fn budget_test_model(supports_vision: bool, window: u32) -> DesktopModelPortV4 {
        DesktopModelPortV4 {
            client: UnifiedModelClient::new(
                Uuid::new_v4(),
                ProviderProtocol::Ollama,
                Url::parse("http://127.0.0.1:1").unwrap(),
                "test-model",
                None,
            )
            .unwrap()
            .with_request_budget(RequestBudget {
                context_window_tokens: window,
                reserved_output_tokens: 100,
                safety_margin_tokens: 10,
            }),
            prompt: PromptLayersV4::default(),
            project_root: PathBuf::from("nonexistent-budget-test-root"),
            supports_vision,
            delegated: None,
        }
    }

    #[tokio::test]
    async fn delegated_profile_freezes_exact_configuration_and_missing_profiles_fail() {
        let repository = Store::open_in_memory().await.unwrap();
        let mut main: omicsops_core::workspace::ModelProfile = serde_json::from_value(json!({
            "id":Uuid::new_v4(),"label":"main","provider":"ollama","base_url":"http://127.0.0.1:11434",
            "model":"main-exact","credential_reference":null,"supports_tools":true,"supports_vision":false,
        })).unwrap();
        repository.save_model_profile(&main).await.unwrap();
        assert_eq!(
            freeze_delegated_model(&repository, main.id).await.unwrap(),
            None
        );
        let mut child = main.clone();
        child.id = Uuid::new_v4();
        child.model = "child-exact".into();
        main.delegated_model_profile_id = Some(child.id);
        repository.save_model_profile(&main).await.unwrap();
        assert!(
            freeze_delegated_model(&repository, main.id)
                .await
                .unwrap_err()
                .contains("not found")
        );
        repository.save_model_profile(&child).await.unwrap();
        let binding = freeze_delegated_model(&repository, main.id)
            .await
            .unwrap()
            .unwrap();
        validate_delegated_profile(&child, &binding).unwrap();
        child.label = "renamed".into();
        child.credential_reference = Some("safe-keyring-reference".into());
        validate_delegated_profile(&child, &binding).unwrap();
        child.model = "child-exact-sibling".into();
        assert!(validate_delegated_profile(&child, &binding).is_err());
        child.model = "child-exact".into();
        child.base_url = "http://127.0.0.1:11435".into();
        assert!(validate_delegated_profile(&child, &binding).is_err());
        let mut parent_port = budget_test_model(false, 100_000);
        assert!(parent_port.delegated_model(Some(&binding)).is_err());
        parent_port.delegated = Some((binding.clone(), Box::new(budget_test_model(false, 500))));
        let child_port = parent_port
            .delegated_model(Some(&binding))
            .unwrap()
            .unwrap();
        let request = ModelRequestV4 {
            system: "system".into(),
            context: "large".repeat(500),
            tools: vec![],
            image_refs: vec![],
        };
        assert!(child_port.validate_request(&request).is_err());
        assert!(parent_port.validate_request(&request).is_ok());
        assert!(parent_port.delegated_model(None).unwrap().is_none());
    }

    #[test]
    fn desktop_preflight_counts_the_actual_nonvision_notice_and_tools() {
        let model = budget_test_model(false, 100_000);
        let request = ModelRequestV4 {
            system: "system".into(),
            context: "user context".into(),
            tools: vec![ToolDescriptorV4 {
                id: "read".into(),
                description: "description".into(),
                input_schema: json!({"type":"object"}),
                effect: ToolEffectV4::ReadOnly,
            }],
            image_refs: vec![ModelImageRefV4 {
                relative_path: "missing.png".into(),
                media_type: "image/png".into(),
                size_bytes: 1,
                sha256: "unused".into(),
            }],
        };
        let preview = model.prepare_request(request.clone(), false).unwrap();
        let actual = model.prepare_request(request.clone(), true).unwrap();
        assert_eq!(preview, actual);
        assert_eq!(preview.tools.len(), 1);
        assert!(
            serde_json::to_string(&preview)
                .unwrap()
                .contains("HOST IMAGE NOTICE")
        );
        model.validate_request(&request).unwrap();
    }

    #[test]
    fn provider_overflow_codes_are_distinct_from_generic_invalid_requests() {
        for message in ["400: context_length_exceeded", "context_window_exceeded"] {
            let failure = classify_model_failure(message);
            assert_eq!(failure.class, ModelErrorClassV4::ContextOverflow);
            assert!(!failure.retryable);
        }
        assert_eq!(
            classify_model_failure("400 invalid tool schema").class,
            ModelErrorClassV4::InvalidRequest
        );
    }

    #[tokio::test]
    async fn desktop_budget_rejects_unknown_images_before_disk_or_network_access() {
        let model = budget_test_model(true, 100_000);
        let request = ModelRequestV4 {
            system: "system".into(),
            context: "context".into(),
            tools: vec![],
            image_refs: vec![ModelImageRefV4 {
                relative_path: "missing.png".into(),
                media_type: "image/png".into(),
                size_bytes: 1,
                sha256: "unused".into(),
            }],
        };
        let preflight = model.validate_request(&request).unwrap_err();
        assert!(preflight.message.to_ascii_lowercase().contains("image"));
        let actual = model.stream(request, &mut |_| {}).await.unwrap_err();
        assert_eq!(actual, preflight);
        assert!(!actual.retryable);
    }

    #[test]
    fn host_route_classifier_forces_research_only_for_external_evidence_signals() {
        for objective in [
            "帮我寻找肝癌相关的单细胞和空间转录组论文",
            "帮我寻找肝癌相关的单细胞和空转文章",
            "Find the latest PubMed literature and cross-source evidence",
            "Find articles about spatial transcriptomics",
            "Search the web for an independent source",
            "请上网查询当前资料",
            "核验 https://example.org/paper 的 DOI",
        ] {
            assert_eq!(
                classify_direct_request(objective),
                AgentRequestRouteV4::ResearchRetrieval
            );
        }
        for objective in [
            "修改本地项目中的 Rust 文件并运行测试",
            "Analyze the attached count matrix without external sources",
            "Update the local wallpaper component without web research",
        ] {
            assert_eq!(
                classify_direct_request(objective),
                AgentRequestRouteV4::Adaptive
            );
        }
    }

    #[test]
    fn duplicate_active_run_registration_preserves_the_original_token() {
        let runs = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let run_id = Uuid::new_v4();
        let original = Arc::new(AtomicBool::new(false));
        let duplicate = Arc::new(AtomicBool::new(false));

        assert!(register_active_run(&runs, run_id, original.clone()).unwrap());
        assert!(!register_active_run(&runs, run_id, duplicate.clone()).unwrap());
        let registered = runs.lock().unwrap().get(&run_id).unwrap().clone();
        assert!(Arc::ptr_eq(&registered, &original));

        registered.store(true, Ordering::SeqCst);
        assert!(original.load(Ordering::SeqCst));
        assert!(!duplicate.load(Ordering::SeqCst));

        remove_active_run(&runs, run_id, &duplicate);
        assert!(runs.lock().unwrap().contains_key(&run_id));
        remove_active_run(&runs, run_id, &original);
        assert!(!runs.lock().unwrap().contains_key(&run_id));
    }

    #[tokio::test]
    async fn resume_waits_for_a_finishing_execution_to_release_its_slot() {
        let runs = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let run_id = Uuid::new_v4();
        let token = Arc::new(AtomicBool::new(false));
        assert!(register_active_run(&runs, run_id, token.clone()).unwrap());

        let releasing_runs = runs.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            remove_active_run(&releasing_runs, run_id, &token);
        });

        wait_for_active_run_to_yield(&runs, run_id).await.unwrap();
        assert!(!runs.lock().unwrap().contains_key(&run_id));
    }

    #[tokio::test]
    async fn resume_wait_timeout_is_an_error_and_keeps_the_active_slot() {
        let runs = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let run_id = Uuid::new_v4();
        let token = Arc::new(AtomicBool::new(false));
        assert!(register_active_run(&runs, run_id, token).unwrap());

        let error = wait_for_active_run_to_yield_with(
            &runs,
            run_id,
            1,
            std::time::Duration::from_millis(0),
        )
        .await
        .unwrap_err();
        assert!(error.contains("resume wait timeout"));
        assert!(runs.lock().unwrap().contains_key(&run_id));
    }

    fn plan_approval_event_fixture(
        decision: Option<ToolApprovalDecisionV4>,
        duplicate: bool,
    ) -> (PlanApprovalScopeV4, ToolCallV4, Vec<AgentEventV4>) {
        let run_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let scope = PlanApprovalScopeV4 {
            project_id,
            conversation_id,
            run_id,
            revision_id: Uuid::new_v4(),
            revision: 1,
        };
        let call = ToolCallV4 {
            call_id: "plan-approval".into(),
            tool_id: "use_mcp_tool".into(),
            arguments: json!({
                "server_id": Uuid::new_v4(),
                "tool": "search",
                "catalog_sha256": "catalog",
                "schema_sha256": "schema",
                "arguments": {"query":"fixture"}
            }),
        };
        let scope_hash = scope.hash();
        let request = ToolApprovalRequestV4::new_with_scope(
            run_id,
            &scope_hash,
            call.clone(),
            ToolEffectV4::ReadOnly,
            "fixture approval",
        )
        .unwrap();
        let first = AgentEventV4::first(
            run_id,
            project_id,
            conversation_id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Plan,
            },
        );
        let requested = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ToolRequested { call: call.clone() },
        );
        let approval = AgentEventV4::next(
            &requested,
            Utc::now(),
            AgentEventKindV4::ToolApprovalRequested {
                request: request.clone(),
            },
        );
        let mut events = vec![first, requested, approval];
        if let Some(decision) = decision {
            let decided = AgentEventV4::next(
                events.last().unwrap(),
                Utc::now(),
                AgentEventKindV4::ToolApprovalDecided {
                    approval_id: request.approval_id.clone(),
                    call_hash: request.call_hash.clone(),
                    decision,
                },
            );
            events.push(decided);
            if duplicate {
                events.push(AgentEventV4::next(
                    events.last().unwrap(),
                    Utc::now(),
                    AgentEventKindV4::ToolApprovalDecided {
                        approval_id: request.approval_id,
                        call_hash: request.call_hash,
                        decision,
                    },
                ));
            }
        }
        (scope, call, events)
    }

    #[test]
    fn plan_approval_resume_decision_requires_one_current_valid_decision() {
        let (scope, _call, events) = plan_approval_event_fixture(None, false);
        let undecided = plan_approval_resume_decision(&events, scope).unwrap_err();
        assert!(undecided.contains("undecided"));

        let (scope, _call, events) =
            plan_approval_event_fixture(Some(ToolApprovalDecisionV4::Approved), false);
        assert_eq!(
            plan_approval_resume_decision(&events, scope).unwrap(),
            ToolApprovalDecisionV4::Approved
        );
        let (scope, _call, events) =
            plan_approval_event_fixture(Some(ToolApprovalDecisionV4::Denied), false);
        assert_eq!(
            plan_approval_resume_decision(&events, scope).unwrap(),
            ToolApprovalDecisionV4::Denied
        );

        let (scope, _call, events) =
            plan_approval_event_fixture(Some(ToolApprovalDecisionV4::Approved), true);
        let duplicate = plan_approval_resume_decision(&events, scope).unwrap_err();
        assert!(duplicate.contains("duplicated") || duplicate.contains("tampered"));

        let (scope, call, mut events) =
            plan_approval_event_fixture(Some(ToolApprovalDecisionV4::Approved), false);
        events.push(AgentEventV4::next(
            events.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::ToolDispatchStarted {
                call_id: call.call_id,
                tool_id: call.tool_id,
                effect: ToolEffectV4::ReadOnly,
                idempotency_key: "already-dispatched".into(),
            },
        ));
        let dispatched = plan_approval_resume_decision(&events, scope).unwrap_err();
        assert!(dispatched.contains("no pending") || dispatched.contains("current"));

        let (mut old_scope, _call, events) =
            plan_approval_event_fixture(Some(ToolApprovalDecisionV4::Approved), false);
        old_scope.revision_id = Uuid::new_v4();
        let old = plan_approval_resume_decision(&events, old_scope).unwrap_err();
        assert!(old.contains("no pending") || old.contains("current"));

        // Keep the first fixture call live in this test so its scope/call
        // construction is also checked by the helper's exact-call predicate.
    }

    #[test]
    fn missing_terminal_events_are_repaired_without_interrupting_live_runs() {
        let now = Utc::now();
        let run_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let mut record = RunRecordV4 {
            run_id,
            project_id,
            conversation_id,
            model_profile_id: Uuid::new_v4(),
            objective: "search literature".into(),
            status: "running".into(),
            plan: None,
            plan_hash: None,
            compute_selection: None,
            approval_hash: None,
            plan_revision: None,
            spec: None,
        };
        let recent = vec![AgentEventV4::first(
            run_id,
            project_id,
            conversation_id,
            now - chrono::Duration::seconds(30),
            AgentEventKindV4::RunCreated {
                mode: omicsops_protocol::RunModeV4::Execute,
            },
        )];
        assert!(missing_terminal_event(&record, &recent, false, now).is_none());

        let stale = vec![AgentEventV4::first(
            run_id,
            project_id,
            conversation_id,
            now - chrono::Duration::minutes(3),
            AgentEventKindV4::RunCreated {
                mode: omicsops_protocol::RunModeV4::Execute,
            },
        )];
        assert!(matches!(
            missing_terminal_event(&record, &stale, false, now),
            Some(AgentEventKindV4::RunFailed { .. })
        ));
        assert!(missing_terminal_event(&record, &stale, true, now).is_none());

        record.status = "failed".into();
        assert!(matches!(
            missing_terminal_event(&record, &recent, false, now),
            Some(AgentEventKindV4::RunFailed { .. })
        ));
    }

    #[test]
    fn schema_bound_run_approval_can_authorize_one_exact_mcp_call() {
        let run_id = Uuid::new_v4();
        let call = ToolCallV4 {
            call_id: "pubmed".into(),
            tool_id: "use_mcp_tool".into(),
            arguments: json!({
                "server_id":Uuid::new_v4(),
                "tool":"pubmed_search",
                "catalog_sha256":"catalog",
                "schema_sha256":"schema",
                "arguments":{"query":"HCC single-cell"}
            }),
        };
        let request = ToolApprovalRequestV4::new(
            run_id,
            "frozen-spec",
            call.clone(),
            ToolEffectV4::Network,
            "network access",
        )
        .unwrap();
        let first = AgentEventV4::first(
            run_id,
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::ToolApprovalRequested {
                request: request.clone(),
            },
        );
        assert!(
            !run_has_approved_tool_call(
                &[first.clone()],
                &call,
                RunModeV4::Execute,
                Some("frozen-spec"),
                None,
                run_id,
            )
            .unwrap()
        );
        let approved = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ToolApprovalDecided {
                approval_id: request.approval_id,
                call_hash: request.call_hash,
                decision: ToolApprovalDecisionV4::Approved,
            },
        );
        let events = [first, approved];
        assert!(
            run_has_approved_tool_call(
                &events,
                &call,
                RunModeV4::Execute,
                Some("frozen-spec"),
                None,
                run_id,
            )
            .unwrap()
        );

        let mut changed = call.clone();
        changed.arguments["arguments"]["query"] = json!("different query");
        assert!(
            !run_has_approved_tool_call(
                &events,
                &changed,
                RunModeV4::Execute,
                Some("frozen-spec"),
                None,
                run_id,
            )
            .unwrap()
        );

        let plan_scope = PlanApprovalScopeV4 {
            project_id: Uuid::new_v4(),
            conversation_id: Uuid::new_v4(),
            run_id,
            revision_id: Uuid::new_v4(),
            revision: 1,
        };
        let plan_scope_hash = plan_scope.hash();
        let plan_request = ToolApprovalRequestV4::new_with_scope(
            run_id,
            &plan_scope_hash,
            call.clone(),
            ToolEffectV4::ReadOnly,
            "plan read-only approval",
        )
        .unwrap();
        let plan_first = AgentEventV4::first(
            run_id,
            plan_scope.project_id,
            plan_scope.conversation_id,
            Utc::now(),
            AgentEventKindV4::ToolApprovalRequested {
                request: plan_request.clone(),
            },
        );
        let plan_decided = AgentEventV4::next(
            &plan_first,
            Utc::now(),
            AgentEventKindV4::ToolApprovalDecided {
                approval_id: plan_request.approval_id,
                call_hash: plan_request.call_hash,
                decision: ToolApprovalDecisionV4::Approved,
            },
        );
        assert!(
            run_has_approved_tool_call(
                &[plan_first.clone(), plan_decided.clone()],
                &call,
                RunModeV4::Plan,
                Some(&plan_scope_hash),
                Some(ToolEffectV4::ReadOnly),
                run_id,
            )
            .unwrap()
        );
        assert!(
            !run_has_approved_tool_call(
                &[plan_first.clone(), plan_decided.clone()],
                &call,
                RunModeV4::Plan,
                Some("old-plan-scope"),
                Some(ToolEffectV4::ReadOnly),
                run_id,
            )
            .unwrap()
        );
        assert!(
            !run_has_approved_tool_call(
                &[plan_first, plan_decided],
                &call,
                RunModeV4::Execute,
                Some(&plan_scope_hash),
                None,
                run_id,
            )
            .unwrap()
        );
    }

    #[test]
    fn direct_mode_builds_an_execution_contract_without_a_model_generated_plan() {
        let plan = direct_execution_plan(
            "run QC now",
            "[{\"role\":\"user\",\"markdown\":\"use hg19\"}]",
            BTreeSet::from(["project.read".into(), "runtime.execute".into()]),
        );
        assert!(plan.objective.contains("run QC now"));
        assert!(plan.objective.contains("use hg19"));
        assert!(plan.requested_capabilities.contains("runtime.execute"));
        assert!(!plan.requested_capabilities.contains("agent.propose_plan"));
    }

    #[test]
    fn v4_paths_never_escape_the_project() {
        assert_eq!(
            project_path("/srv/project", "results/a.txt").unwrap(),
            "/srv/project/results/a.txt"
        );
        assert!(project_path("/srv/project", "../secret").is_err());
        assert!(project_path("/srv/project", "/etc/passwd").is_err());
    }

    #[test]
    fn v4_output_excerpt_is_bounded_and_preserves_both_ends() {
        let output = format!("BEGIN{}END", "x".repeat(100 * 1024));
        let (excerpt, truncated) = bounded_excerpt(&output, 16 * 1024);
        assert!(truncated);
        assert!(excerpt.starts_with("BEGIN"));
        assert!(excerpt.ends_with("END"));
        assert!(excerpt.len() < 17 * 1024);
    }

    #[test]
    fn v4_provider_errors_are_classified_for_host_retry() {
        assert!(classify_model_failure("429 rate limit").retryable);
        assert!(classify_model_failure("error decoding response body").retryable);
        assert!(
            classify_model_failure("stream ended after partial output: unexpected EOF").retryable
        );
        assert_eq!(
            classify_model_failure("request timed out").class,
            ModelErrorClassV4::Timeout
        );
        assert!(!classify_model_failure("401 unauthorized").retryable);
    }

    #[test]
    fn v4_answers_reject_empty_unknown_stale_and_duplicate_questions() {
        let first = AgentEventV4::first(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::InputRequested {
                question_id: "first".into(),
                question: "First question?".into(),
                reason: AgentInputReasonV4::Decision,
            },
        );
        let second = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::InputRequested {
                question_id: "second".into(),
                question: "Second question?".into(),
                reason: AgentInputReasonV4::Decision,
            },
        );
        let events = vec![first, second.clone()];
        assert!(validate_answer_v4(&events, "second", "  ").is_err());
        assert!(validate_answer_v4(&events, "missing", "answer").is_err());
        assert!(validate_answer_v4(&events, "first", "answer").is_err());
        assert_eq!(
            validate_answer_v4(&events, "second", "  answer  ").unwrap(),
            "answer"
        );

        let answered = AgentEventV4::next(
            &second,
            Utc::now(),
            AgentEventKindV4::UserInputAnswered {
                question_id: "second".into(),
                answer: "answer".into(),
            },
        );
        assert!(validate_answer_v4(&[events, vec![answered]].concat(), "second", "again").is_err());
    }

    #[test]
    fn system_environment_ensure_reports_ready_without_mutation() {
        let content = system_environment_ready(
            KernelLanguageV4::Python,
            BTreeMap::from([("python".into(), "3.12.0".into())]),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["environment"], "system");
        assert_eq!(value["status"], "ready");
        assert_eq!(value["mutated"], false);
        assert_eq!(value["software_versions"]["python"], "3.12.0");
    }

    #[test]
    fn failed_historical_system_ensure_is_safe_to_resume_once() {
        let first = AgentEventV4::first(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        let dispatch = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ToolDispatchStarted {
                call_id: "ensure-system".into(),
                tool_id: "runtime.environment.ensure".into(),
                effect: ToolEffectV4::Runtime,
                idempotency_key: "ensure-system".into(),
            },
        );
        let failed = AgentEventV4::next(
            &dispatch,
            Utc::now(),
            AgentEventKindV4::RunFailed {
                message: "tool error: system environment cannot be created or changed".into(),
            },
        );
        let mut events = vec![first, dispatch, failed];
        assert_eq!(
            recoverable_system_environment_ensure(&events, Some("system")).as_deref(),
            Some("ensure-system")
        );
        let resolved = AgentEventV4::next(
            events.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::ToolDispatchResolved {
                call_id: "ensure-system".into(),
                resolution: UncertainResolutionV4::SideEffectNotObserved,
                evidence: "known pre-mutation failure".into(),
            },
        );
        events.push(resolved);
        assert_eq!(
            recoverable_system_environment_ensure(&events, Some("system")),
            None
        );
    }

    #[test]
    fn backend_discovery_only_inspects_and_never_pulls_or_runs_images() {
        assert_eq!(
            container_image_inspect_args("omicsops/test:latest"),
            [
                "image",
                "inspect",
                "--format",
                "{{.Id}}",
                "omicsops/test:latest"
            ]
        );
    }

    #[test]
    fn historical_v4_projects_map_to_supervised_system_ssh_selection() {
        let connection_id = Uuid::new_v4();
        let mut project = Project::new(
            Uuid::new_v4(),
            "legacy",
            ".",
            omicsops_core::workspace::ProjectTemplate::Blank,
            Utc::now(),
        );
        project.connection_id = Some(connection_id);
        project.remote_root = Some("/srv/project".into());
        let selection = legacy_ssh_selection(&project).unwrap();
        assert_eq!(selection.backend_id, format!("ssh:{connection_id}"));
        assert_eq!(selection.autonomy_mode, AutonomyModeV4::Supervised);
        assert_eq!(selection.environment, "system");
        assert_eq!(selection.network_policy, NetworkPolicyV4::HostInherited);
    }

    #[tokio::test]
    async fn local_project_filesystem_lists_reads_and_hashes_inside_root() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("results")).unwrap();
        std::fs::write(
            directory.path().join("results/qc.tsv"),
            b"metric\tvalue\nrows\t1\n",
        )
        .unwrap();
        let filesystem = LocalProjectFilesystemV4::new(directory.path()).unwrap();
        let listing = filesystem.list(".").await.unwrap();
        assert!(listing.contains("results/qc.tsv"));
        assert!(
            filesystem
                .read("results/qc.tsv")
                .await
                .unwrap()
                .contains("rows")
        );
        let fact = filesystem.verify_file("results/qc.tsv").await.unwrap();
        assert_eq!(fact.size_bytes, b"metric\tvalue\nrows\t1\n".len() as u64);
        assert_eq!(fact.sha256.len(), 64);
        assert!(filesystem.read("../outside.txt").await.is_err());
    }

    #[test]
    fn v4_environment_names_cannot_escape_project_scope() {
        assert!(validate_environment_name("analysis-r").is_ok());
        assert!(validate_environment_name("system").is_ok());
        assert!(validate_environment_name("../outside").is_err());
        assert!(validate_environment_name("bad/name").is_err());
    }

    #[test]
    fn v4_project_agent_override_is_rendered_after_base_rules() {
        let rendered = project_rules_layer("base-rule", "override-rule");
        assert!(rendered.find("base-rule") < rendered.find("override-rule"));
        assert!(rendered.contains("wins on conflict"));
    }

    #[tokio::test]
    async fn v4_scientific_hooks_build_verified_lineage_and_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let project_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let repository = Store::open(directory.path().join("science.sqlite"))
            .await
            .unwrap();
        let project = Project::new(
            project_id,
            "scientific test",
            directory.path().display().to_string(),
            omicsops_core::workspace::ProjectTemplate::Blank,
            Utc::now(),
        );
        repository.save_project(&project).await.unwrap();
        let store = RepositoryScientificStateStoreV4 {
            repository,
            backend_id: "ssh:test".into(),
            mutation_lock: Arc::new(Mutex::new(())),
        };
        let dataset_call = ToolCallV4 {
            call_id: "dataset-call".into(),
            tool_id: "science.register_dataset".into(),
            arguments: json!({}),
        };
        store
            .after_tool(
                project_id,
                run_id,
                &dataset_call,
                &ToolOutcomeV4 {
                    call_id: dataset_call.call_id.clone(),
                    tool_id: dataset_call.tool_id.clone(),
                    succeeded: true,
                    model_content: "verified".into(),
                    data: json!({
                        "path":"data/input.h5ad",
                        "size_bytes":42,
                        "sha256":"host-dataset-hash",
                        "modality":"single_cell_rna",
                        "species":"human",
                        "sample_ids":["sample-a","sample-b"],
                        "matrix_shape":[100,20000],
                        "stage":"raw"
                    }),
                    provenance: vec![],
                },
            )
            .await
            .unwrap();
        let dataset_id = *store
            .snapshot(project_id)
            .await
            .unwrap()
            .datasets
            .keys()
            .next()
            .unwrap();
        let runtime_call = ToolCallV4 {
            call_id: "runtime-call".into(),
            tool_id: "runtime.execute".into(),
            arguments: json!({
                "language":"python",
                "environment":"analysis",
                "code":"print('done')",
                "analysis": {
                    "analysis_type":"qc",
                    "input_dataset_ids":[dataset_id],
                    "sample_ids":["sample-a","sample-b"],
                    "method":"dynamic-python",
                    "parameters":{"min_genes":200},
                    "software_requirements":["scanpy"],
                    "database_versions":{},
                    "random_seed":7
                }
            }),
        };
        store
            .before_tool(project_id, run_id, &runtime_call)
            .await
            .unwrap();
        let runtime_result = RuntimeResultV4 {
            request_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            process_identity: "ssh-jsonl:python:123".into(),
            stdout: "done".into(),
            stderr: String::new(),
            stdout_capture: None,
            stderr_capture: None,
            succeeded: true,
            artifacts: vec![RuntimeArtifactV4 {
                relative_path: "results/qc.tsv".into(),
                size_bytes: 99,
                sha256: "host-artifact-hash".into(),
            }],
            software_versions: BTreeMap::from([("scanpy".into(), "1.11.0".into())]),
        };
        store
            .after_tool(
                project_id,
                run_id,
                &runtime_call,
                &ToolOutcomeV4 {
                    call_id: runtime_call.call_id.clone(),
                    tool_id: runtime_call.tool_id.clone(),
                    succeeded: true,
                    model_content: "done".into(),
                    data: serde_json::to_value(runtime_result).unwrap(),
                    provenance: vec![],
                },
            )
            .await
            .unwrap();
        let state = store.snapshot(project_id).await.unwrap();
        let analysis = state.analyses.values().next().unwrap();
        let artifact = state.artifacts.values().next().unwrap();
        let manifest = state.provenance.values().next().unwrap();
        assert_eq!(analysis.status, AnalysisStatusV4::Succeeded);
        assert_eq!(artifact.producer_analysis_id, analysis.id);
        assert_eq!(artifact.sha256, "host-artifact-hash");
        assert_eq!(manifest.runtime_session_id, analysis.runtime.session_id);
        assert_eq!(manifest.code, "print('done')");
        assert!(manifest.complete);
    }

    #[tokio::test]
    #[ignore = "requires explicit live model/SSH credentials and an empty disposable OMICSOPS_LIVE_PBMC_ROOT"]
    async fn live_v4_model_plan_and_persistent_ssh_python_kernel() {
        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            label: "V4 acceptance".into(),
            host: std::env::var("OMICSOPS_LIVE_SSH_HOST").expect("live host"),
            port: std::env::var("OMICSOPS_LIVE_SSH_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(22),
            username: std::env::var("OMICSOPS_LIVE_SSH_USER").expect("live user"),
            authentication: AuthenticationMethod::Password,
            authentication_reference: "acceptance".into(),
            host_key_fingerprint: Some(
                std::env::var("OMICSOPS_LIVE_SSH_FINGERPRINT").expect("fingerprint"),
            ),
        };
        let session = Arc::new(
            SshSession::connect(
                &profile,
                SshAuthentication::Password(
                    std::env::var("OMICSOPS_LIVE_SSH_PASSWORD").expect("password"),
                ),
            )
            .await
            .unwrap(),
        );
        let root = std::env::var("OMICSOPS_LIVE_PBMC_ROOT").expect("disposable root");
        let root = resolve_root(&session, &root).await.unwrap();
        let protocol = match std::env::var("OMICSOPS_LIVE_MODEL_PROTOCOL").as_deref() {
            Ok("anthropic") => ProviderProtocol::Anthropic,
            Ok("ollama") => ProviderProtocol::Ollama,
            _ => ProviderProtocol::OpenAiCompatible,
        };
        let model = DesktopModelPortV4 {
            client: UnifiedModelClient::new(
                Uuid::new_v4(),
                protocol,
                Url::parse(&std::env::var("OMICSOPS_LIVE_MODEL_BASE_URL").expect("model url"))
                    .unwrap(),
                std::env::var("OMICSOPS_LIVE_MODEL_NAME").expect("model name"),
                std::env::var("OMICSOPS_LIVE_MODEL_CREDENTIAL").ok(),
            )
            .unwrap(),
            prompt: PromptLayersV4::default(),
            project_root: std::env::current_dir().unwrap(),
            supports_vision: false,
            delegated: None,
        };
        let mut streamed = String::new();
        let turn = model
            .stream(
                ModelRequestV4 {
                    system: "Return exactly one native agent.propose_plan tool call. Do not return prose.".into(),
                    context: "Create schema_version 4 plan for a two-cell Python persistence probe with nonempty steps and completion_criteria; requested_capabilities must contain runtime.execute.".into(),
                    tools: vec![ToolDescriptorV4 {
                        id: "agent.propose_plan".into(),
                        description: "submit plan".into(),
                        input_schema: json!({"type":"object","required":["schema_version","objective","steps","completion_criteria","requested_capabilities"],"properties":{}}),
                        effect: ToolEffectV4::ReadOnly,
                    }],
                    image_refs: vec![],
                },
                &mut |event| {
                    if let ModelStreamEventV4::TextDelta(delta) = event {
                        streamed.push_str(&delta);
                    }
                },
            )
            .await
            .unwrap();
        assert_eq!(streamed, turn.public_text);
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].tool_id, "agent.propose_plan");
        let plan: ExecutionPlanV4 =
            serde_json::from_value(turn.tool_calls[0].arguments.clone()).unwrap();
        assert!(plan.canonical_hash().is_ok());
        let project_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let backend = Arc::new(SshKernelBackendV4 {
            session,
            root,
            project_id,
            backend_id: "ssh:live".into(),
        });
        let runtime = RuntimeManagerV4::new(backend);
        let key = ExecutionContextKeyV4 {
            project_id,
            run_id,
            backend_id: "ssh:live".into(),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        };
        let first = runtime
            .execute(&key, "v4_probe = 'persistent-ok'".into(), vec![])
            .await
            .unwrap();
        let second = runtime
            .execute(&key, "print(v4_probe)".into(), vec![])
            .await
            .unwrap();
        assert_eq!(first.session_id, second.session_id);
        assert_eq!(first.process_identity, second.process_identity);
        assert!(second.stdout.contains("persistent-ok"));
        let _ = runtime.interrupt(&key).await;
        let _ = RunModeV4::Execute;
    }

    #[tokio::test]
    #[ignore = "requires explicit live SSH credentials, R, Micromamba, and an empty disposable OMICSOPS_LIVE_PBMC_ROOT"]
    async fn live_v4_stage2_r_output_cancel_rebuild_and_project_environment() {
        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            label: "V4 stage 2 acceptance".into(),
            host: std::env::var("OMICSOPS_LIVE_SSH_HOST").expect("live host"),
            port: std::env::var("OMICSOPS_LIVE_SSH_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(22),
            username: std::env::var("OMICSOPS_LIVE_SSH_USER").expect("live user"),
            authentication: AuthenticationMethod::Password,
            authentication_reference: "acceptance".into(),
            host_key_fingerprint: Some(
                std::env::var("OMICSOPS_LIVE_SSH_FINGERPRINT").expect("fingerprint"),
            ),
        };
        let session = Arc::new(
            SshSession::connect(
                &profile,
                SshAuthentication::Password(
                    std::env::var("OMICSOPS_LIVE_SSH_PASSWORD").expect("password"),
                ),
            )
            .await
            .unwrap(),
        );
        let root = resolve_root(
            &session,
            &std::env::var("OMICSOPS_LIVE_PBMC_ROOT").expect("disposable root"),
        )
        .await
        .unwrap();
        let project_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let backend = Arc::new(SshKernelBackendV4 {
            session: session.clone(),
            root: root.clone(),
            project_id,
            backend_id: "ssh:live-stage2".into(),
        });
        let runtime = Arc::new(RuntimeManagerV4::new(backend));
        let python = ExecutionContextKeyV4 {
            project_id,
            run_id,
            backend_id: "ssh:live-stage2".into(),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        };

        let large = runtime
            .execute(&python, "print('x' * (100 * 1024))".into(), vec![])
            .await
            .unwrap();
        let capture = large.stdout_capture.as_ref().unwrap();
        assert!(capture.total_bytes >= 100 * 1024);
        assert!(capture.truncated);
        assert_eq!(capture.sha256.len(), 64);
        assert!(large.stdout.len() < 17 * 1024);

        let original_session = large.session_id;
        let rebuilt = runtime.rebuild(&python).await.unwrap();
        assert_ne!(original_session, rebuilt.session_id());

        let long_runtime = runtime.clone();
        let long_key = python.clone();
        let long = tokio::spawn(async move {
            long_runtime
                .execute(&long_key, "import time; time.sleep(60)".into(), vec![])
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        long.abort();
        runtime.interrupt(&python).await.unwrap();
        let after_interrupt = runtime
            .execute(&python, "print('after-interrupt')".into(), vec![])
            .await
            .unwrap();
        assert!(after_interrupt.stdout.contains("after-interrupt"));

        let executor = DesktopToolExecutorV4 {
            repository: Store::open_in_memory().await.unwrap(),
            mcp_sessions: McpSessionManager::new(),
            credentials: SystemCredentialVault,
            filesystem: Arc::new(SshProjectFilesystemV4 {
                session: session.clone(),
                root: root.clone(),
            }),
            environment_port: Arc::new(SshEnvironmentPortV4 {
                session: session.clone(),
                root: root.clone(),
            }),
            selection: ComputeSelectionV4 {
                schema_version: 4,
                backend_id: "ssh:live-stage2".into(),
                backend_kind: ComputeBackendKindV4::Ssh,
                autonomy_mode: AutonomyModeV4::Supervised,
                approval_policy: ApprovalPolicyV4::RiskBased,
                environment: "stage2-r".into(),
                network_policy: NetworkPolicyV4::HostInherited,
                container_image: None,
            },
            project_id,
            run_id,
            conversation_id: Uuid::new_v4(),
            backend_id: "ssh:live-stage2".into(),
            runtime: runtime.clone(),
            browser: omicsops_browser::BrowserRuntime::new(
                std::env::temp_dir().join("omicsops-live-stage2-browser"),
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../browser-extension"),
            ),
            local_project_root: std::env::temp_dir(),
            browser_authorizations: Arc::new(std::sync::Mutex::new(Vec::new())),
            forced_route: None,
        };
        executor
            .execute(&ToolCallV4 {
                call_id: "ensure-r".into(),
                tool_id: "runtime.environment.ensure".into(),
                arguments: json!({"environment":"stage2-r","language":"r"}),
            })
            .await
            .unwrap();
        let r_key = ExecutionContextKeyV4 {
            language: KernelLanguageV4::R,
            environment: "stage2-r".into(),
            ..python
        };
        let first = runtime
            .execute(&r_key, "v4_r_probe <- 'persistent-r'".into(), vec![])
            .await
            .unwrap();
        let second = runtime
            .execute(&r_key, "cat(v4_r_probe)".into(), vec![])
            .await
            .unwrap();
        assert_eq!(first.session_id, second.session_id);
        assert!(second.stdout.contains("persistent-r"));
        runtime.interrupt_run(run_id).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires explicit live SSH credentials and an empty disposable OMICSOPS_LIVE_PBMC_ROOT"]
    async fn live_v4_stage3_host_verified_scientific_lineage() {
        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            label: "V4 stage 3 acceptance".into(),
            host: std::env::var("OMICSOPS_LIVE_SSH_HOST").expect("live host"),
            port: std::env::var("OMICSOPS_LIVE_SSH_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(22),
            username: std::env::var("OMICSOPS_LIVE_SSH_USER").expect("live user"),
            authentication: AuthenticationMethod::Password,
            authentication_reference: "acceptance".into(),
            host_key_fingerprint: Some(
                std::env::var("OMICSOPS_LIVE_SSH_FINGERPRINT").expect("fingerprint"),
            ),
        };
        let session = Arc::new(
            SshSession::connect(
                &profile,
                SshAuthentication::Password(
                    std::env::var("OMICSOPS_LIVE_SSH_PASSWORD").expect("password"),
                ),
            )
            .await
            .unwrap(),
        );
        let root = resolve_root(
            &session,
            &std::env::var("OMICSOPS_LIVE_PBMC_ROOT").expect("disposable root"),
        )
        .await
        .unwrap();
        session
            .execute_checked(&format!(
                "mkdir -p {0}/data {0}/results && printf 'gene\\tsample-a\\ns1\\t1\\n' > {0}/data/input.tsv",
                shell_quote(&root)
            ))
            .await
            .unwrap();
        let project_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let backend_id = "ssh:live-stage3".to_string();
        let runtime = Arc::new(RuntimeManagerV4::new(Arc::new(SshKernelBackendV4 {
            session: session.clone(),
            root: root.clone(),
            project_id,
            backend_id: backend_id.clone(),
        })));
        let executor = DesktopToolExecutorV4 {
            repository: Store::open_in_memory().await.unwrap(),
            mcp_sessions: McpSessionManager::new(),
            credentials: SystemCredentialVault,
            filesystem: Arc::new(SshProjectFilesystemV4 {
                session: session.clone(),
                root: root.clone(),
            }),
            environment_port: Arc::new(SshEnvironmentPortV4 {
                session,
                root: root.clone(),
            }),
            selection: ComputeSelectionV4 {
                schema_version: 4,
                backend_id: backend_id.clone(),
                backend_kind: ComputeBackendKindV4::Ssh,
                autonomy_mode: AutonomyModeV4::Supervised,
                approval_policy: ApprovalPolicyV4::RiskBased,
                environment: "system".into(),
                network_policy: NetworkPolicyV4::HostInherited,
                container_image: None,
            },
            project_id,
            run_id,
            conversation_id: Uuid::new_v4(),
            backend_id: backend_id.clone(),
            runtime: runtime.clone(),
            browser: omicsops_browser::BrowserRuntime::new(
                std::env::temp_dir().join("omicsops-live-stage3-browser"),
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../browser-extension"),
            ),
            local_project_root: std::env::temp_dir(),
            browser_authorizations: Arc::new(std::sync::Mutex::new(Vec::new())),
            forced_route: None,
        };
        let local_state = tempfile::tempdir().unwrap();
        let repository = Store::open(local_state.path().join("stage3.sqlite"))
            .await
            .unwrap();
        let project = Project::new(
            project_id,
            "live scientific test",
            root.clone(),
            omicsops_core::workspace::ProjectTemplate::Blank,
            Utc::now(),
        );
        repository.save_project(&project).await.unwrap();
        let science = RepositoryScientificStateStoreV4 {
            repository,
            backend_id,
            mutation_lock: Arc::new(Mutex::new(())),
        };
        let dataset_call = ToolCallV4 {
            call_id: "live-dataset".into(),
            tool_id: "science.register_dataset".into(),
            arguments: json!({
                "modality":"expression_matrix",
                "species":"human",
                "sample_ids":["sample-a"],
                "matrix_shape":[1,1],
                "stage":"raw",
                "path":"data/input.tsv"
            }),
        };
        let dataset_outcome = executor.execute(&dataset_call).await.unwrap();
        science
            .after_tool(project_id, run_id, &dataset_call, &dataset_outcome)
            .await
            .unwrap();
        let dataset_id = *science
            .snapshot(project_id)
            .await
            .unwrap()
            .datasets
            .keys()
            .next()
            .unwrap();
        let analysis_call = ToolCallV4 {
            call_id: "live-analysis".into(),
            tool_id: "runtime.execute".into(),
            arguments: json!({
                "language":"python",
                "code":"open('results/qc.tsv','w').write('metric\\tvalue\\nrows\\t1\\n')",
                "capture_paths":["results/qc.tsv"],
                "analysis":{
                    "analysis_type":"qc",
                    "input_dataset_ids":[dataset_id],
                    "sample_ids":["sample-a"],
                    "method":"dynamic-python",
                    "parameters":{},
                    "software_requirements":[],
                    "database_versions":{},
                    "random_seed":7
                }
            }),
        };
        science
            .before_tool(project_id, run_id, &analysis_call)
            .await
            .unwrap();
        let analysis_outcome = executor.execute(&analysis_call).await.unwrap();
        science
            .after_tool(project_id, run_id, &analysis_call, &analysis_outcome)
            .await
            .unwrap();
        let state = science.snapshot(project_id).await.unwrap();
        let dataset = state.datasets.get(&dataset_id).unwrap();
        let artifact = state.artifacts.values().next().unwrap();
        let manifest = state.provenance.values().next().unwrap();
        assert_eq!(dataset.sha256.len(), 64);
        assert_eq!(artifact.sha256.len(), 64);
        assert_eq!(artifact.producer_analysis_id, manifest.analysis_id);
        assert!(manifest.complete);
        runtime.interrupt_run(run_id).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires explicit live SSH credentials and an empty disposable OMICSOPS_LIVE_PBMC_ROOT"]
    async fn live_v4_stage4_project_rule_priority() {
        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            label: "V4 stage 4 acceptance".into(),
            host: std::env::var("OMICSOPS_LIVE_SSH_HOST").expect("live host"),
            port: std::env::var("OMICSOPS_LIVE_SSH_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(22),
            username: std::env::var("OMICSOPS_LIVE_SSH_USER").expect("live user"),
            authentication: AuthenticationMethod::Password,
            authentication_reference: "acceptance".into(),
            host_key_fingerprint: Some(
                std::env::var("OMICSOPS_LIVE_SSH_FINGERPRINT").expect("fingerprint"),
            ),
        };
        let session = Arc::new(
            SshSession::connect(
                &profile,
                SshAuthentication::Password(
                    std::env::var("OMICSOPS_LIVE_SSH_PASSWORD").expect("password"),
                ),
            )
            .await
            .unwrap(),
        );
        let root = resolve_root(
            &session,
            &std::env::var("OMICSOPS_LIVE_PBMC_ROOT").expect("disposable root"),
        )
        .await
        .unwrap();
        session
            .execute_checked(&format!(
                "mkdir -p {0}/.omicsops && printf 'base-live-rule\\n' > {0}/AGENTS.md && printf 'override-live-rule\\n' > {0}/.omicsops/AGENT.md",
                shell_quote(&root)
            ))
            .await
            .unwrap();
        let prompt = load_prompt_layers(&session, &root, "ssh:live-stage4")
            .await
            .unwrap();
        let rendered = prompt.render(RunModeV4::Plan);
        assert!(rendered.find("base-live-rule") < rendered.find("override-live-rule"));
        assert!(rendered.contains("HIGHER PRIORITY"));
        assert!(!rendered.contains("SKILL.md contents"));
    }

    #[test]
    fn approval_event_broadcast_failure_is_best_effort() {
        let event = AgentEventV4::first(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Utc::now(),
            AgentEventKindV4::PlanApproved {
                plan_hash: "hash".into(),
            },
        );
        let mut attempts = 0;
        let error = broadcast_events_best_effort(std::slice::from_ref(&event), |_event| {
            attempts += 1;
            Err("emit failed".into())
        });
        assert_eq!(attempts, 1);
        assert_eq!(error.as_deref(), Some("emit failed"));
    }
}

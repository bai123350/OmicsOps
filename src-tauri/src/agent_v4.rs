use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use chrono::Utc;
use omicsops_adapters::{
    kernel::{kernel_driver, validate_capture_paths, validate_kernel_code},
    llm::UnifiedModelClient,
    persistence::Repository,
    ssh::{SshJsonlProcess, SshSession},
};
use omicsops_agent::harness_v3::{
    ModelRequestV2, ModelStreamEventV2, ModelToolSpec, ToolCallAccumulatorV2,
};
use omicsops_agent::{
    KernelEvent, KernelEventDecoder, KernelEventKind, KernelLanguage, KernelRequest,
};
use omicsops_agent_core::{
    AgentCoreErrorV4, AgentCoreV4, AgentLimitsV4, EventStoreV4, ModelPortV4, ModelRequestV4,
    ModelStreamEventV4, ModelTurnV4, PromptLayersV4, ReviewerRequestV4, ScientificStateStoreV4,
    ScientificUpdateV4,
};
use omicsops_core::{
    domain::ProjectSpec,
    project::{require_remote_descendant, shell_quote},
};
use omicsops_knowledge::{
    McpToolIndexV4, MemoryDocumentV4, SkillDocumentV4, authorize_mcp_use, freeze_skill,
    markdown_sections, schema_digest, search_mcp_tools, search_memory, search_skills,
};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ComputeBackendDescriptorV4, ComputeBackendKindV4,
    ContextArchiveV4, ContextCheckpointV4, ExecutionContextKeyV4, ExecutionPlanV4,
    IsolationStrengthV4, KernelLanguageV4, ModelErrorClassV4, ModelFailureV4, OutputCaptureV4,
    ReviewerReportV4, RunSpecV4, RuntimeArtifactV4, RuntimeResultV4, ToolCallV4, ToolDescriptorV4,
    ToolEffectV4, ToolOutcomeV4, UncertainResolutionV4,
};
use omicsops_runtime::{KernelBackendV4, KernelProcessV4, RuntimeManagerV4};
use omicsops_science::{
    AnalysisDeclarationV4, AnalysisStatusV4, DatasetStageV4, EvidenceDeclarationV4,
    RuntimeIdentityV4, ScientificStateV4, VerifiedArtifactFactV4, VerifiedDatasetFactV4,
};
use omicsops_tools::{ToolExecutorV4, ToolRegistryV4, builtin_tool_definitions_v4};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::commands::{
    AppState, authentication_for_profile, current_project_spec, find_profile, require_trusted_host,
    unified_model_client,
};
use crate::p1_commands::{
    McpServerProfile, MemorySearchRequest, invoke_configured_mcp_tool_v4, memory_facts,
};

static PROJECT_SIDE_EFFECT_LOCKS_V4: OnceLock<std::sync::Mutex<HashMap<Uuid, Weak<Mutex<()>>>>> =
    OnceLock::new();
static SCIENTIFIC_STATE_LOCKS_V4: OnceLock<
    std::sync::Mutex<HashMap<Uuid, Weak<std::sync::Mutex<()>>>>,
> = OnceLock::new();

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovePlanV4Request {
    pub run_id: Uuid,
    pub plan_hash: String,
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
pub struct RunSummaryV4 {
    pub run_id: Uuid,
    pub status: String,
    pub plan: Option<ExecutionPlanV4>,
    pub plan_hash: Option<String>,
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
    spec: Option<RunSpecV4>,
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
    let project = current_project_spec(&state.repository, request.project_id)?;
    let (model, tools) =
        compose(&state, &project, request.model_profile_id, Uuid::new_v4()).await?;
    let run_id = tools.run_id();
    let mut record = RunRecordV4 {
        run_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        model_profile_id: request.model_profile_id,
        objective: request.objective.clone(),
        status: "planning".into(),
        plan: None,
        plan_hash: None,
        spec: None,
    };
    save_record(&state.repository, &record)?;
    let event_store = RepositoryEventStoreV4 {
        repository: state.repository.clone(),
        app: app.clone(),
    };
    let science_store = RepositoryScientificStateStoreV4 {
        repository: state.repository.clone(),
        backend_id: format!("ssh:{}", project.connection_id),
        mutation_lock: scientific_state_lock_v4(project.id),
    };
    let core = AgentCoreV4 {
        model: model.as_ref(),
        tools: tools.registry.as_ref(),
        events: &event_store,
        science: Some(&science_store),
    };
    let plan = match core
        .plan(
            run_id,
            request.project_id,
            request.conversation_id,
            &request.objective,
        )
        .await
    {
        Ok(plan) => plan,
        Err(AgentCoreErrorV4::WaitingForInput) => {
            record.status = "waiting_for_input".into();
            save_record(&state.repository, &record)?;
            return Ok(RunSummaryV4 {
                run_id,
                status: record.status,
                plan: None,
                plan_hash: None,
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let hash = plan.canonical_hash().map_err(|error| error.to_string())?;
    record.status = "awaiting_approval".into();
    record.plan = Some(plan.clone());
    record.plan_hash = Some(hash.clone());
    save_record(&state.repository, &record)?;
    Ok(RunSummaryV4 {
        run_id,
        status: record.status,
        plan: Some(plan),
        plan_hash: Some(hash),
    })
}

#[tauri::command]
pub async fn agent_v4_approve_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ApprovePlanV4Request,
) -> Result<RunSummaryV4, String> {
    let mut record = load_record(&state.repository, request.run_id)?;
    if record.status != "awaiting_approval" {
        return Err("V4 run is not awaiting approval".into());
    }
    let plan = record
        .plan
        .clone()
        .ok_or_else(|| "V4 plan is missing".to_string())?;
    let spec = RunSpecV4::freeze(
        record.run_id,
        record.project_id,
        record.conversation_id,
        record.model_profile_id,
        plan.clone(),
        &request.plan_hash,
        Utc::now(),
    )
    .map_err(|error| error.to_string())?;
    let store = RepositoryEventStoreV4 {
        repository: state.repository.clone(),
        app: app.clone(),
    };
    append_next(
        &store,
        record.run_id,
        AgentEventKindV4::PlanApproved {
            plan_hash: request.plan_hash.clone(),
        },
    )?;
    append_next(
        &store,
        record.run_id,
        AgentEventKindV4::ModeChanged {
            mode: omicsops_protocol::RunModeV4::Execute,
        },
    )?;
    record.status = "running".into();
    record.spec = Some(spec.clone());
    save_record(&state.repository, &record)?;
    spawn_execution(app, &state, record.clone(), spec)?;
    Ok(RunSummaryV4 {
        run_id: record.run_id,
        status: "running".into(),
        plan: Some(plan),
        plan_hash: Some(request.plan_hash),
    })
}

#[tauri::command]
pub async fn agent_v4_resume(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<(), String> {
    let record = load_record(&state.repository, run_id)?;
    let Some(spec) = record.spec.clone() else {
        let project = current_project_spec(&state.repository, record.project_id)?;
        let (model, tools) =
            compose(&state, &project, record.model_profile_id, record.run_id).await?;
        let store = RepositoryEventStoreV4 {
            repository: state.repository.clone(),
            app: app.clone(),
        };
        let science_store = RepositoryScientificStateStoreV4 {
            repository: state.repository.clone(),
            backend_id: format!("ssh:{}", project.connection_id),
            mutation_lock: scientific_state_lock_v4(project.id),
        };
        let core = AgentCoreV4 {
            model: model.as_ref(),
            tools: tools.registry.as_ref(),
            events: &store,
            science: Some(&science_store),
        };
        match core
            .plan(
                record.run_id,
                record.project_id,
                record.conversation_id,
                &record.objective,
            )
            .await
        {
            Ok(plan) => {
                let mut updated = record;
                let hash = plan.canonical_hash().map_err(|e| e.to_string())?;
                updated.status = "awaiting_approval".into();
                updated.plan = Some(plan);
                updated.plan_hash = Some(hash);
                save_record(&state.repository, &updated)?;
                return Ok(());
            }
            Err(AgentCoreErrorV4::WaitingForInput) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    };
    if matches!(record.status.as_str(), "completed" | "cancelled") {
        return Err("V4 run is terminal".into());
    }
    spawn_execution(app, &state, record, spec)
}

#[tauri::command]
pub fn agent_v4_cancel(state: State<'_, AppState>, run_id: Uuid) -> Result<(), String> {
    let active = state
        .active_runs
        .lock()
        .map_err(|_| "active run registry unavailable".to_string())?;
    active
        .get(&run_id)
        .ok_or_else(|| "V4 run is not active".to_string())?
        .store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn agent_v4_answer(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AnswerV4Request,
) -> Result<(), String> {
    let store = RepositoryEventStoreV4 {
        repository: state.repository.clone(),
        app,
    };
    append_next(
        &store,
        request.run_id,
        AgentEventKindV4::UserInputAnswered {
            question_id: request.question_id,
            answer: request.answer,
        },
    )
}

#[tauri::command]
pub fn agent_v4_resolve_uncertain(
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
}

#[tauri::command]
pub fn agent_v4_events(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<Vec<AgentEventV4>, String> {
    state
        .repository
        .agent_events_v4(run_id)
        .map_err(|error| error.to_string())
}

fn spawn_execution(
    app: AppHandle,
    state: &AppState,
    mut record: RunRecordV4,
    spec: RunSpecV4,
) -> Result<(), String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    if state
        .active_runs
        .lock()
        .map_err(|_| "active run registry unavailable".to_string())?
        .insert(spec.run_id, cancelled.clone())
        .is_some()
    {
        return Err("V4 run is already active".into());
    }
    let repository = state.repository.clone();
    let active = state.active_runs.clone();
    let project = current_project_spec(&repository, spec.project_id)?;
    let profile = find_profile(&repository, project.connection_id)?;
    require_trusted_host(&profile)?;
    let authentication = authentication_for_profile(state, &profile)?;
    let model = unified_model_client(state, spec.model_profile_id)?;
    tauri::async_runtime::spawn(async move {
        let outcome = async {
            let session = Arc::new(
                SshSession::connect(&profile, authentication)
                    .await
                    .map_err(|error| error.to_string())?,
            );
            let root = resolve_root(&session, &project.remote_root).await?;
            let prompt =
                load_prompt_layers(&session, &root, &format!("ssh:{}", profile.id)).await?;
            let backend = Arc::new(SshKernelBackendV4 {
                session: session.clone(),
                root: root.clone(),
                project_id: project.id,
                backend_id: format!("ssh:{}", profile.id),
            });
            let runtime = Arc::new(RuntimeManagerV4::new(backend));
            let executor = Arc::new(DesktopToolExecutorV4 {
                repository: repository.clone(),
                session,
                root,
                project_id: project.id,
                run_id: spec.run_id,
                backend_id: format!("ssh:{}", profile.id),
                runtime,
            });
            let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), executor)
                .map_err(|error| error.to_string())?
                .with_side_effect_lock(project_side_effect_lock_v4(project.id))
                .with_execute_capabilities(spec.plan.requested_capabilities.clone());
            let model = DesktopModelPortV4 {
                client: model,
                prompt,
            };
            let store = RepositoryEventStoreV4 {
                repository: repository.clone(),
                app: app.clone(),
            };
            let science_store = RepositoryScientificStateStoreV4 {
                repository: repository.clone(),
                backend_id: format!("ssh:{}", profile.id),
                mutation_lock: scientific_state_lock_v4(project.id),
            };
            AgentCoreV4 {
                model: &model,
                tools: &registry,
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
        let uncertain = outcome
            .as_ref()
            .is_err_and(|error| error.contains("side-effect dispatch is uncertain"));
        let verifier_attention = outcome
            .as_ref()
            .is_err_and(|error| error.starts_with("run needs attention:"));
        record.status = if outcome.is_ok() {
            "completed"
        } else if waiting {
            "waiting_for_input"
        } else if uncertain || verifier_attention {
            "needs_attention"
        } else if cancelled.load(Ordering::SeqCst) {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        if let Err(error) = &outcome {
            if !cancelled.load(Ordering::SeqCst) && !waiting {
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
                        .ok()
                        .and_then(|events| events.last().cloned())
                        .is_some_and(|event| {
                            matches!(event.event, AgentEventKindV4::RunNeedsAttention { .. })
                        });
                if !already_recorded {
                    let _ = append_next(&store, spec.run_id, event);
                }
            }
        }
        let _ = save_record(&repository, &record);
        active.lock().expect("active runs").remove(&spec.run_id);
    });
    Ok(())
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

async fn compose(
    state: &AppState,
    project: &ProjectSpec,
    model_profile_id: Uuid,
    run_id: Uuid,
) -> Result<(Arc<DesktopModelPortV4>, ComposedToolsV4), String> {
    let profile = find_profile(&state.repository, project.connection_id)?;
    require_trusted_host(&profile)?;
    let auth = authentication_for_profile(state, &profile)?;
    let session = Arc::new(
        SshSession::connect(&profile, auth)
            .await
            .map_err(|error| error.to_string())?,
    );
    let root = resolve_root(&session, &project.remote_root).await?;
    let prompt = load_prompt_layers(&session, &root, &format!("ssh:{}", profile.id)).await?;
    let backend = Arc::new(SshKernelBackendV4 {
        session: session.clone(),
        root: root.clone(),
        project_id: project.id,
        backend_id: format!("ssh:{}", profile.id),
    });
    let runtime = Arc::new(RuntimeManagerV4::new(backend));
    let executor = Arc::new(DesktopToolExecutorV4 {
        repository: state.repository.clone(),
        session,
        root,
        project_id: project.id,
        run_id,
        backend_id: format!("ssh:{}", profile.id),
        runtime,
    });
    let registry = Arc::new(
        ToolRegistryV4::new(builtin_tool_definitions_v4(), executor)
            .map_err(|error| error.to_string())?
            .with_side_effect_lock(project_side_effect_lock_v4(project.id)),
    );
    Ok((
        Arc::new(DesktopModelPortV4 {
            client: unified_model_client(state, model_profile_id)?,
            prompt,
        }),
        ComposedToolsV4 { run_id, registry },
    ))
}

struct DesktopModelPortV4 {
    client: UnifiedModelClient,
    prompt: PromptLayersV4,
}
#[async_trait]
impl ModelPortV4 for DesktopModelPortV4 {
    fn prompt_layers(&self) -> PromptLayersV4 {
        self.prompt.clone()
    }

    async fn stream(
        &self,
        request: ModelRequestV4,
        on_event: &mut (dyn FnMut(ModelStreamEventV4) + Send),
    ) -> Result<ModelTurnV4, ModelFailureV4> {
        let tools = request
            .tools
            .into_iter()
            .map(|tool| ModelToolSpec {
                id: tool.id,
                description: tool.description,
                input_schema: tool.input_schema,
            })
            .collect();
        let mut text = String::new();
        let mut calls = ToolCallAccumulatorV2::default();
        let mut provider_error = None;
        let mut accumulator_error = None;
        self.client
            .stream_with_v2(
                ModelRequestV2 {
                    system: request.system,
                    messages: vec![omicsops_agent::ModelMessage {
                        role: "user".into(),
                        content: request.context,
                    }],
                    tools,
                    require_strict_json_fallback: true,
                },
                |event| match event {
                    ModelStreamEventV2::TextDelta { text: delta } => {
                        text.push_str(&delta);
                        on_event(ModelStreamEventV4::TextDelta(delta));
                    }
                    ModelStreamEventV2::Retrying {
                        attempt,
                        delay_ms,
                        message,
                    } => on_event(ModelStreamEventV4::ProviderRetrying {
                        attempt,
                        delay_ms,
                        message,
                    }),
                    ModelStreamEventV2::Error { code, message, .. } => {
                        provider_error =
                            Some(classify_model_failure(&format!("{code}: {message}")));
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
                },
            )
            .await
            .map_err(|error| classify_model_failure(&error.to_string()))?;
        if let Some(error) = provider_error.or(accumulator_error) {
            return Err(error);
        }
        let tool_calls = calls
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

fn classify_model_failure(message: &str) -> ModelFailureV4 {
    let lower = message.to_ascii_lowercase();
    if lower.contains("429") || lower.contains("rate limit") {
        ModelFailureV4::transient(ModelErrorClassV4::RateLimited, message)
    } else if ["500", "502", "503", "504"]
        .iter()
        .any(|status| lower.contains(status))
    {
        ModelFailureV4::transient(ModelErrorClassV4::Server, message)
    } else if lower.contains("timed out") || lower.contains("timeout") {
        ModelFailureV4::transient(ModelErrorClassV4::Timeout, message)
    } else if lower.contains("connect") || lower.contains("transport") {
        ModelFailureV4::transient(ModelErrorClassV4::Transport, message)
    } else if lower.contains("401") || lower.contains("403") || lower.contains("credential") {
        ModelFailureV4::permanent(ModelErrorClassV4::Authentication, message)
    } else if lower.contains("400") || lower.contains("422") {
        ModelFailureV4::permanent(ModelErrorClassV4::InvalidRequest, message)
    } else {
        ModelFailureV4::permanent(ModelErrorClassV4::InvalidResponse, message)
    }
}

struct DesktopToolExecutorV4 {
    repository: Repository,
    session: Arc<SshSession>,
    root: String,
    project_id: Uuid,
    run_id: Uuid,
    backend_id: String,
    runtime: Arc<RuntimeManagerV4>,
}

impl DesktopToolExecutorV4 {
    fn key(
        &self,
        language: KernelLanguageV4,
        environment: &str,
    ) -> Result<ExecutionContextKeyV4, String> {
        validate_environment_name(environment)?;
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
            .collect::<Vec<_>>();
        let executable_prefix = if environment == "system" {
            String::new()
        } else {
            format!(
                "micromamba run --prefix {} ",
                shell_quote(&environment_path(&self.root, environment))
            )
        };
        let command = match language {
            KernelLanguageV4::Python => {
                let names = serde_json::to_string(&requirements).unwrap_or_else(|_| "[]".into());
                let code = format!(
                    "import importlib.metadata as m,platform\nnames={names}\nprint('python\\t'+platform.python_version())\nfor n in names:\n try: print(n+'\\t'+m.version(n))\n except m.PackageNotFoundError: pass"
                );
                format!("{executable_prefix}python -c {}", shell_quote(&code))
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
                format!("{executable_prefix}Rscript -e {}", shell_quote(&code))
            }
        };
        let Ok(output) = self.session.execute_checked(&command).await else {
            return BTreeMap::new();
        };
        output
            .stdout
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(name, version)| (name.to_owned(), version.trim().to_owned()))
            .collect()
    }

    fn skill_documents(&self) -> Result<Vec<SkillDocumentV4>, String> {
        crate::skill_commands::agent_skill_packages(&self.repository)?
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

    fn memory_documents(&self, dimension: Option<&str>) -> Result<Vec<MemoryDocumentV4>, String> {
        let facts = memory_facts(
            &self.repository,
            &MemorySearchRequest {
                project_id: self.project_id,
                conversation_id: None,
                query: String::new(),
                dimension: dimension.map(str::to_owned),
            },
        )?;
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

    fn mcp_tool_index(&self) -> Result<Vec<McpToolIndexV4>, String> {
        let profiles = self
            .repository
            .list_json::<McpServerProfile>("mcp_server")
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
}
#[async_trait]
impl ToolExecutorV4 for DesktopToolExecutorV4 {
    async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
        let (content, data, provenance) = match call.tool_id.as_str() {
            "project.list" => {
                let path = call
                    .arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                let remote = project_path(&self.root, path)?;
                let out = self
                    .session
                    .execute_checked(&format!(
                        "find {} -maxdepth 2 -printf '%y\\t%s\\t%p\\n' | head -400",
                        shell_quote(&remote)
                    ))
                    .await
                    .map_err(|e| e.to_string())?;
                (
                    relative_listing(&out.stdout, &self.root),
                    json!({"path":path}),
                    vec![format!("remote:{path}")],
                )
            }
            "project.read" => {
                let path = required(&call.arguments, "path")?;
                let remote = project_path(&self.root, path)?;
                let out = self
                    .session
                    .execute_checked(&format!(
                        "test -f {0} && sed -n '1,400p' {0}",
                        shell_quote(&remote)
                    ))
                    .await
                    .map_err(|e| e.to_string())?;
                (
                    out.stdout,
                    json!({"path":path}),
                    vec![format!("remote:{path}")],
                )
            }
            "search_skills" => {
                let query = required(&call.arguments, "query")?;
                let limit = call
                    .arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(8) as usize;
                let hits = search_skills(query, &self.skill_documents()?, limit);
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
                    .skill_documents()?
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
                let hits =
                    search_memory(query, &self.memory_documents(dimension)?, Utc::now(), limit);
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
                let hits = search_mcp_tools(query, &self.mcp_tool_index()?, limit);
                (
                    serde_json::to_string(&hits).map_err(|error| error.to_string())?,
                    serde_json::to_value(&hits).map_err(|error| error.to_string())?,
                    vec![],
                )
            }
            "use_mcp_tool" => {
                let server_id = required(&call.arguments, "server_id")?
                    .parse::<Uuid>()
                    .map_err(|_| "server_id must be a UUID")?;
                let tool = required(&call.arguments, "tool")?;
                let expected_schema = required(&call.arguments, "schema_sha256")?;
                let indexed = self
                    .mcp_tool_index()?
                    .into_iter()
                    .find(|entry| entry.server_id == server_id && entry.tool_name == tool)
                    .ok_or("MCP tool is not currently indexed")?;
                authorize_mcp_use(&indexed, expected_schema).map_err(|error| error.to_string())?;
                let result = invoke_configured_mcp_tool_v4(
                    &self.repository,
                    self.project_id,
                    server_id,
                    tool,
                    call.arguments
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                    expected_schema.into(),
                )
                .await?;
                let value = result.result.unwrap_or_else(|| json!({}));
                (
                    serde_json::to_string(&value).map_err(|error| error.to_string())?,
                    json!({
                        "server_id":server_id,
                        "tool":tool,
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
                let mut result = self.runtime.execute(&key, code, captures).await?;
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
                let content = format!(
                    "session={} process={} request={}\nstdout ({} bytes, sha256={}):\n{}\nstderr ({} bytes, sha256={}):\n{}",
                    result.session_id,
                    result.process_identity,
                    result.request_id,
                    result
                        .stdout_capture
                        .as_ref()
                        .map_or(0, |capture| capture.total_bytes),
                    result
                        .stdout_capture
                        .as_ref()
                        .map_or("", |capture| capture.sha256.as_str()),
                    result.stdout,
                    result
                        .stderr_capture
                        .as_ref()
                        .map_or(0, |capture| capture.total_bytes),
                    result
                        .stderr_capture
                        .as_ref()
                        .map_or("", |capture| capture.sha256.as_str()),
                    result.stderr
                );
                return Ok(ToolOutcomeV4 {
                    call_id: call.call_id.clone(),
                    tool_id: call.tool_id.clone(),
                    succeeded: result.succeeded,
                    model_content: content,
                    data: serde_json::to_value(&result).map_err(|e| e.to_string())?,
                    provenance: vec![format!("kernel-session:{}", result.session_id)],
                });
            }
            "runtime.environment.ensure" => {
                let language = parse_language(required(&call.arguments, "language")?)?;
                let environment = required(&call.arguments, "environment")?;
                validate_environment_name(environment)?;
                if environment == "system" {
                    return Err("system environment cannot be created or changed".into());
                }
                let prefix = environment_path(&self.root, environment);
                let packages = match language {
                    KernelLanguageV4::Python => "python",
                    KernelLanguageV4::R => "r-base r-jsonlite",
                };
                let out = self
                    .session
                    .execute_checked(&format!(
                        "prefix={0}; mkdir -p {1}; if test -d \"$prefix/conda-meta\"; then printf 'reused %s\\n' \"$prefix\"; else tool=$(command -v micromamba) || {{ printf 'micromamba is required\\n' >&2; exit 69; }}; \"$tool\" create --yes --prefix \"$prefix\" {2}; fi; \"${{tool:-$(command -v micromamba)}}\" list --prefix \"$prefix\" --explicit",
                        shell_quote(&prefix),
                        shell_quote(&format!("{}/.omicsops/environments", self.root.trim_end_matches('/'))),
                        packages,
                    ))
                    .await
                    .map_err(|e| e.to_string())?;
                let (excerpt, _) = bounded_excerpt(&out.stdout, 16 * 1024);
                (
                    excerpt,
                    json!({"environment":environment,"prefix":prefix,"language":language}),
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
                let remote = project_path(&self.root, path)?;
                let out = self
                    .session
                    .execute_checked(&format!(
                        "test -f {0} && stat -c '%s' {0} && sha256sum {0}",
                        shell_quote(&remote)
                    ))
                    .await
                    .map_err(|error| error.to_string())?;
                let mut lines = out.stdout.lines();
                let size = lines
                    .next()
                    .ok_or("missing dataset size")?
                    .parse::<u64>()
                    .map_err(|_| "invalid dataset size")?;
                let hash = lines
                    .next()
                    .and_then(|line| line.split_whitespace().next())
                    .ok_or("missing dataset hash")?;
                let data = json!({
                    "path": path,
                    "size_bytes": size,
                    "sha256": hash,
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
                let remote = project_path(&self.root, path)?;
                let out = self
                    .session
                    .execute_checked(&format!(
                        "test -f {0} && stat -c '%s' {0} && sha256sum {0}",
                        shell_quote(&remote)
                    ))
                    .await
                    .map_err(|e| e.to_string())?;
                let mut lines = out.stdout.lines();
                let size = lines
                    .next()
                    .ok_or("missing size")?
                    .parse::<u64>()
                    .map_err(|_| "invalid size")?;
                let hash = lines
                    .next()
                    .and_then(|v| v.split_whitespace().next())
                    .ok_or("missing hash")?;
                (
                    format!("verified {path}: {size} bytes sha256={hash}"),
                    json!({"path":path,"size_bytes":size,"sha256":hash}),
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

    async fn interrupt(&self, run_id: Uuid) -> Result<(), String> {
        self.runtime.interrupt_run(run_id).await
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
    repository: Repository,
    app: AppHandle,
}
impl EventStoreV4 for RepositoryEventStoreV4 {
    fn append(&self, event: &AgentEventV4) -> Result<(), String> {
        self.repository
            .append_agent_event_v4(event)
            .map_err(|e| e.to_string())?;
        self.app
            .emit(AGENT_V4_EVENT_CHANNEL, event)
            .map_err(|e| e.to_string())
    }
    fn load(&self, run_id: Uuid) -> Result<Vec<AgentEventV4>, String> {
        self.repository
            .agent_events_v4(run_id)
            .map_err(|e| e.to_string())
    }

    fn archive_context(
        &self,
        run_id: Uuid,
        transcript: &str,
        checkpoint: &ContextCheckpointV4,
    ) -> Result<ContextArchiveV4, String> {
        self.repository
            .archive_agent_context_v4(run_id, transcript, checkpoint)
            .map_err(|error| error.to_string())
    }
}

fn scientific_state_lock_v4(project_id: Uuid) -> Arc<std::sync::Mutex<()>> {
    let locks = SCIENTIFIC_STATE_LOCKS_V4.get_or_init(Default::default);
    let mut locks = locks.lock().expect("V4 scientific state lock registry");
    if let Some(lock) = locks.get(&project_id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(std::sync::Mutex::new(()));
    locks.insert(project_id, Arc::downgrade(&lock));
    lock
}

struct RepositoryScientificStateStoreV4 {
    repository: Repository,
    backend_id: String,
    mutation_lock: Arc<std::sync::Mutex<()>>,
}

impl RepositoryScientificStateStoreV4 {
    fn load(&self, project_id: Uuid) -> Result<ScientificStateV4, String> {
        Ok(self
            .repository
            .scientific_state_v4(project_id)
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| ScientificStateV4::new(project_id)))
    }

    fn save_update(
        &self,
        state: &ScientificStateV4,
        changes: Vec<String>,
    ) -> Result<Option<ScientificUpdateV4>, String> {
        self.repository
            .save_scientific_state_v4(state)
            .map_err(|error| error.to_string())?;
        Ok(Some(ScientificUpdateV4 {
            revision: state.revision,
            state_sha256: state.digest(),
            changes,
        }))
    }
}

impl ScientificStateStoreV4 for RepositoryScientificStateStoreV4 {
    fn snapshot(&self, project_id: Uuid) -> Result<ScientificStateV4, String> {
        self.load(project_id)
    }

    fn before_tool(
        &self,
        project_id: Uuid,
        run_id: Uuid,
        call: &ToolCallV4,
    ) -> Result<Option<ScientificUpdateV4>, String> {
        if call.tool_id != "runtime.execute" || call.arguments.get("analysis").is_none() {
            return Ok(None);
        }
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| "scientific state lock is poisoned".to_string())?;
        let mut state = self.load(project_id)?;
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
    }

    fn after_tool(
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
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| "scientific state lock is poisoned".to_string())?;
        let mut state = self.load(project_id)?;
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

fn append_next(
    store: &RepositoryEventStoreV4,
    run_id: Uuid,
    event: AgentEventKindV4,
) -> Result<(), String> {
    let events = store.load(run_id)?;
    let previous = events.last().ok_or("V4 run has no event")?;
    store.append(&AgentEventV4::next(previous, Utc::now(), event))
}
fn save_record(repository: &Repository, record: &RunRecordV4) -> Result<(), String> {
    repository
        .save_agent_run_v4(
            record.run_id,
            record.project_id,
            record.conversation_id,
            &record.status,
            &serde_json::to_value(record).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
}
fn load_record(repository: &Repository, run_id: Uuid) -> Result<RunRecordV4, String> {
    serde_json::from_value(
        repository
            .agent_run_v4(run_id)
            .map_err(|e| e.to_string())?
            .ok_or("V4 run not found")?,
    )
    .map_err(|e| e.to_string())
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
    use omicsops_protocol::{RunModeV4, ToolDescriptorV4, ToolEffectV4};
    use url::Url;

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
        assert_eq!(
            classify_model_failure("request timed out").class,
            ModelErrorClassV4::Timeout
        );
        assert!(!classify_model_failure("401 unauthorized").retryable);
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

    #[test]
    fn v4_scientific_hooks_build_verified_lineage_and_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::open(directory.path().join("science.sqlite")).unwrap();
        let project_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let store = RepositoryScientificStateStoreV4 {
            repository,
            backend_id: "ssh:test".into(),
            mutation_lock: Arc::new(std::sync::Mutex::new(())),
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
            .unwrap();
        let dataset_id = *store
            .snapshot(project_id)
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
            .unwrap();
        let state = store.snapshot(project_id).unwrap();
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
            repository: Repository::open_in_memory().unwrap(),
            session: session.clone(),
            root: root.clone(),
            project_id,
            run_id,
            backend_id: "ssh:live-stage2".into(),
            runtime: runtime.clone(),
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
            repository: Repository::open_in_memory().unwrap(),
            session,
            root,
            project_id,
            run_id,
            backend_id: backend_id.clone(),
            runtime: runtime.clone(),
        };
        let local_state = tempfile::tempdir().unwrap();
        let repository = Repository::open(local_state.path().join("stage3.sqlite")).unwrap();
        let science = RepositoryScientificStateStoreV4 {
            repository,
            backend_id,
            mutation_lock: Arc::new(std::sync::Mutex::new(())),
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
            .unwrap();
        let dataset_id = *science
            .snapshot(project_id)
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
            .unwrap();
        let analysis_outcome = executor.execute(&analysis_call).await.unwrap();
        science
            .after_tool(project_id, run_id, &analysis_call, &analysis_outcome)
            .unwrap();
        let state = science.snapshot(project_id).unwrap();
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
}

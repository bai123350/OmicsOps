use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
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
    ModelProviderV2, ModelRequestV2, ModelStreamEventV2, ModelToolSpec, ToolCallAccumulatorV2,
};
use omicsops_agent::{
    KernelEvent, KernelEventDecoder, KernelEventKind, KernelLanguage, KernelRequest,
};
use omicsops_agent_core::{
    AgentCoreErrorV4, AgentCoreV4, EventStoreV4, ModelPortV4, ModelRequestV4, ModelTurnV4,
};
use omicsops_core::{
    domain::ProjectSpec,
    project::{require_remote_descendant, shell_quote},
};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ExecutionContextKeyV4, ExecutionPlanV4, KernelLanguageV4,
    RunSpecV4, RuntimeResultV4, ToolCallV4, ToolOutcomeV4,
};
use omicsops_runtime::{KernelBackendV4, KernelProcessV4, RuntimeManagerV4};
use omicsops_tools::{ToolExecutorV4, ToolRegistryV4, builtin_tool_definitions_v4};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::commands::{
    AppState, authentication_for_profile, current_project_spec, find_profile, require_trusted_host,
    unified_model_client,
};

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
    let core = AgentCoreV4 {
        model: model.as_ref(),
        tools: tools.registry.as_ref(),
        events: &event_store,
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
        let core = AgentCoreV4 {
            model: model.as_ref(),
            tools: tools.registry.as_ref(),
            events: &store,
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
            let backend = Arc::new(SshKernelBackendV4 {
                session: session.clone(),
                root: root.clone(),
                project_id: project.id,
            });
            let runtime = Arc::new(RuntimeManagerV4::new(backend));
            let executor = Arc::new(DesktopToolExecutorV4 {
                session,
                root,
                key: ExecutionContextKeyV4 {
                    project_id: project.id,
                    run_id: spec.run_id,
                    backend_id: format!("ssh:{}", profile.id),
                    language: KernelLanguageV4::Python,
                    environment: "system".into(),
                },
                runtime,
            });
            let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), executor)
                .map_err(|error| error.to_string())?
                .with_execute_capabilities(spec.plan.requested_capabilities.clone());
            let model = DesktopModelPortV4(model);
            let store = RepositoryEventStoreV4 {
                repository: repository.clone(),
                app: app.clone(),
            };
            AgentCoreV4 {
                model: &model,
                tools: &registry,
                events: &store,
            }
            .execute_with_cancellation(&spec, 32, &cancelled)
            .await
            .map_err(|error| error.to_string())
        }
        .await;
        let waiting = outcome
            .as_ref()
            .is_err_and(|error| error == "run is waiting for user input");
        record.status = if outcome.is_ok() {
            "completed"
        } else if waiting {
            "waiting_for_input"
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
                let _ = append_next(
                    &store,
                    spec.run_id,
                    AgentEventKindV4::RunFailed {
                        message: error.clone(),
                    },
                );
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
    let backend = Arc::new(SshKernelBackendV4 {
        session: session.clone(),
        root: root.clone(),
        project_id: project.id,
    });
    let runtime = Arc::new(RuntimeManagerV4::new(backend));
    let executor = Arc::new(DesktopToolExecutorV4 {
        session,
        root,
        key: ExecutionContextKeyV4 {
            project_id: project.id,
            run_id,
            backend_id: format!("ssh:{}", profile.id),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        },
        runtime,
    });
    let registry = Arc::new(
        ToolRegistryV4::new(builtin_tool_definitions_v4(), executor)
            .map_err(|error| error.to_string())?,
    );
    Ok((
        Arc::new(DesktopModelPortV4(unified_model_client(
            state,
            model_profile_id,
        )?)),
        ComposedToolsV4 { run_id, registry },
    ))
}

struct DesktopModelPortV4(UnifiedModelClient);
#[async_trait]
impl ModelPortV4 for DesktopModelPortV4 {
    async fn complete(&self, request: ModelRequestV4) -> Result<ModelTurnV4, String> {
        let tools = request
            .tools
            .into_iter()
            .map(|tool| ModelToolSpec {
                id: tool.id,
                description: tool.description,
                input_schema: tool.input_schema,
            })
            .collect();
        let events = self
            .0
            .stream_v2(ModelRequestV2 {
                system: request.system,
                messages: vec![omicsops_agent::ModelMessage {
                    role: "user".into(),
                    content: request.context,
                }],
                tools,
                require_strict_json_fallback: true,
            })
            .await
            .map_err(|error| error.to_string())?;
        let mut text = String::new();
        let mut calls = ToolCallAccumulatorV2::default();
        for event in events {
            match event {
                ModelStreamEventV2::TextDelta { text: delta } => text.push_str(&delta),
                ModelStreamEventV2::Error { code, message, .. } => {
                    return Err(format!("{code}: {message}"));
                }
                other => calls.push(&other).map_err(|error| error.to_string())?,
            }
        }
        let tool_calls = calls
            .finish()
            .map_err(|error| error.to_string())?
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
}

struct DesktopToolExecutorV4 {
    session: Arc<SshSession>,
    root: String,
    key: ExecutionContextKeyV4,
    runtime: Arc<RuntimeManagerV4>,
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
            "runtime.execute" => {
                let language = required(&call.arguments, "language")?;
                if language != "python" {
                    return Err("stage 1 V4 runtime supports python only".into());
                }
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
                let result = self.runtime.execute(&self.key, code, captures).await?;
                let content = format!(
                    "session={} process={}\nstdout:\n{}\nstderr:\n{}",
                    result.session_id, result.process_identity, result.stdout, result.stderr
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
}

struct SshKernelBackendV4 {
    session: Arc<SshSession>,
    root: String,
    project_id: Uuid,
}
#[async_trait]
impl KernelBackendV4 for SshKernelBackendV4 {
    async fn launch(
        &self,
        key: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn KernelProcessV4>, String> {
        if key.language != KernelLanguageV4::Python {
            return Err("stage 1 supports python only".into());
        }
        let session_id = Uuid::new_v4();
        let dir = format!("{}/.omicsops/kernels", self.root.trim_end_matches('/'));
        self.session
            .execute_checked(&format!("mkdir -p {}", shell_quote(&dir)))
            .await
            .map_err(|e| e.to_string())?;
        let driver = format!("{dir}/v4-{session_id}.driver.py");
        self.session
            .upload_text(&driver, kernel_driver(KernelLanguage::Python))
            .await
            .map_err(|e| e.to_string())?;
        let command = format!(
            "python3 -u {} {} {} {}",
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
            identity: format!("ssh-jsonl:{session_id}"),
            project_id: self.project_id,
            process: Mutex::new(Some(process)),
        }))
    }
}

struct SshKernelProcessV4 {
    id: Uuid,
    identity: String,
    project_id: Uuid,
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
                KernelEventKind::Artifact { relative_path, .. } => artifacts.push(relative_path),
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
        Ok(RuntimeResultV4 {
            session_id: self.id,
            process_identity: self.identity.clone(),
            stdout,
            stderr,
            succeeded,
            artifacts,
        })
    }
    async fn interrupt(&self) -> Result<(), String> {
        if let Some(process) = self.process.lock().await.take() {
            process.shutdown().await.map_err(|e| e.to_string())?
        }
        Ok(())
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
        let model = DesktopModelPortV4(
            UnifiedModelClient::new(
                Uuid::new_v4(),
                protocol,
                Url::parse(&std::env::var("OMICSOPS_LIVE_MODEL_BASE_URL").expect("model url"))
                    .unwrap(),
                std::env::var("OMICSOPS_LIVE_MODEL_NAME").expect("model name"),
                std::env::var("OMICSOPS_LIVE_MODEL_CREDENTIAL").ok(),
            )
            .unwrap(),
        );
        let turn=model.complete(ModelRequestV4{system:"Return exactly one native agent.propose_plan tool call. Do not return prose.".into(),context:"Create schema_version 4 plan for a two-cell Python persistence probe with nonempty steps and completion_criteria; requested_capabilities must contain runtime.execute.".into(),tools:vec![ToolDescriptorV4{id:"agent.propose_plan".into(),description:"submit plan".into(),input_schema:json!({"type":"object","required":["schema_version","objective","steps","completion_criteria","requested_capabilities"],"properties":{}}),effect:ToolEffectV4::ReadOnly}]}).await.unwrap();
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
}

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use omicsops_adapters::{
    llm::UnifiedModelClient,
    persistence::Repository,
    ssh::{SshAuthentication, SshSession},
};
use omicsops_agent::harness_v3::{
    AgentRunEventKindV3, AgentRunEventV3, AgentRunSpecV3, AgentRunStateV3, AgentSnapshotV3,
    AuthorityDecisionV3, CancellationTokenV3, CompletionCriterionV3, EventSinkV3, HarnessV3Engine,
    NoopToolAuditSinkV3, RunStatusV3, SkillReferenceV3, ToolAuthorityV3, ToolCallRequestV3,
    ToolDefinitionV3, ToolOutcomeStatusV3, ToolOutcomeV3, ToolRouterV3, ToolRuntimeV3,
};
use omicsops_core::{
    domain::{ConnectionProfile, ProjectSpec},
    plan_v2::{AnalysisPlanV2, StepAction},
    project::{require_remote_descendant, shell_quote},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::{
    commands::{AppState, validate_agent_command},
    p1_commands::{approved_mcp_tool_definitions_v3, invoke_configured_mcp_tool_v3},
};

pub const AGENT_RUN_EVENT_V3_CHANNEL: &str = "agent-run-v3-event";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRuntimeBindingV3 {
    pub run_id: Uuid,
    pub profile_id: Uuid,
    pub project_id: Uuid,
    pub approved_plan_id: Uuid,
    pub model_profile_id: Uuid,
}

pub fn run_spec_from_plan(
    run_id: Uuid,
    project_id: Uuid,
    plan: &AnalysisPlanV2,
) -> Result<AgentRunSpecV3, String> {
    if plan.metadata.get("harness_id").map(String::as_str) != Some("agent.harness_v3@3.0.0") {
        return Err("plan is not a Harness v3 plan".into());
    }
    let conversation_id = plan
        .metadata
        .get("conversation_id")
        .ok_or_else(|| "Harness v3 plan has no conversation id".to_string())?
        .parse::<Uuid>()
        .map_err(|_| "Harness v3 plan has an invalid conversation id".to_string())?;
    let arguments = plan
        .stages
        .iter()
        .flat_map(|stage| &stage.steps)
        .find_map(|step| match &step.action {
            StepAction::Tool {
                tool_id,
                version,
                arguments,
            } if tool_id == "agent.harness_v3" && version == "3.0.0" => Some(arguments),
            _ => None,
        })
        .ok_or_else(|| "Harness v3 plan has no v3 tool step".to_string())?;
    let objective = arguments
        .get("goal")
        .and_then(Value::as_str)
        .ok_or_else(|| "Harness v3 plan has no goal".to_string())?;
    let criteria = arguments
        .get("completion_criteria")
        .and_then(Value::as_array)
        .ok_or_else(|| "Harness v3 plan has no completion criteria".to_string())?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if let Some(description) = value.as_str() {
                CompletionCriterionV3::new(format!("criterion-{}", index + 1), description)
            } else {
                CompletionCriterionV3::new(
                    value
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("criterion-{}", index + 1)),
                    value
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("Complete the approved criterion"),
                )
            }
        })
        .collect::<Vec<_>>();
    let tool_snapshot = serde_json::from_value(
        arguments
            .get("tool_snapshot")
            .cloned()
            .ok_or_else(|| "Harness v3 plan has no tool snapshot".to_string())?,
    )
    .map_err(|error| format!("invalid Harness v3 tool snapshot: {error}"))?;
    let skill_references = arguments
        .get("skill_references")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|citation| {
            Some(SkillReferenceV3 {
                package_id: citation
                    .get("name")
                    .or_else(|| citation.get("skill_id"))?
                    .as_str()?
                    .into(),
                version: citation.get("version")?.as_str()?.into(),
                sha256: citation.get("package_sha256")?.as_str()?.into(),
                references: vec![
                    citation
                        .get("section")
                        .and_then(Value::as_str)
                        .unwrap_or("SKILL.md")
                        .into(),
                ],
            })
        })
        .collect::<Vec<_>>();
    Ok(AgentRunSpecV3::new(
        run_id,
        project_id,
        conversation_id,
        objective,
        criteria,
        tool_snapshot,
        skill_references,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn start(
    app: AppHandle,
    state: &AppState,
    profile: ConnectionProfile,
    authentication: SshAuthentication,
    project: ProjectSpec,
    approved_plan_id: Uuid,
    plan: AnalysisPlanV2,
    model_profile_id: Uuid,
    model: UnifiedModelClient,
) -> Result<Uuid, String> {
    let run_id = Uuid::new_v4();
    let spec = run_spec_from_plan(run_id, project.id, &plan)?;
    let binding = AgentRunRuntimeBindingV3 {
        run_id,
        profile_id: profile.id,
        project_id: project.id,
        approved_plan_id,
        model_profile_id,
    };
    state
        .repository
        .put_json("agent_run_spec_v3", &run_id.to_string(), &spec)
        .map_err(|error| error.to_string())?;
    state
        .repository
        .put_json("agent_run_runtime_v3", &run_id.to_string(), &binding)
        .map_err(|error| error.to_string())?;
    spawn(
        app,
        state.repository.clone(),
        state.active_runs.clone(),
        profile,
        authentication,
        project,
        spec,
        model,
        Vec::new(),
    )?;
    Ok(run_id)
}

pub fn resume(app: AppHandle, state: &AppState, run_id: Uuid) -> Result<(), String> {
    let spec: AgentRunSpecV3 = state
        .repository
        .get_json("agent_run_spec_v3", &run_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Harness v3 run does not exist".to_string())?;
    let binding: AgentRunRuntimeBindingV3 = state
        .repository
        .get_json("agent_run_runtime_v3", &run_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Harness v3 runtime binding is missing".to_string())?;
    let events = state
        .repository
        .agent_run_events_v3(run_id)
        .map_err(|error| error.to_string())?;
    let current = AgentRunStateV3::replay(&spec, &events).map_err(|error| error.to_string())?;
    if matches!(
        current.status,
        RunStatusV3::Completed | RunStatusV3::Cancelled | RunStatusV3::Failed
    ) {
        return Err("Harness v3 run is already terminal".into());
    }
    let profile = crate::commands::find_profile(&state.repository, binding.profile_id)?;
    crate::commands::require_trusted_host(&profile)?;
    let authentication = crate::commands::authentication_for_profile(state, &profile)?;
    let project = crate::commands::current_project_spec(&state.repository, binding.project_id)?;
    let model = crate::commands::unified_model_client(state, binding.model_profile_id)?;
    spawn(
        app,
        state.repository.clone(),
        state.active_runs.clone(),
        profile,
        authentication,
        project,
        spec,
        model,
        events,
    )
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    app: AppHandle,
    repository: Repository,
    active_runs: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    profile: ConnectionProfile,
    authentication: SshAuthentication,
    project: ProjectSpec,
    spec: AgentRunSpecV3,
    model: UnifiedModelClient,
    existing_events: Vec<AgentRunEventV3>,
) -> Result<(), String> {
    let run_id = spec.run_id;
    let requested = Arc::new(AtomicBool::new(false));
    {
        let mut active = active_runs
            .lock()
            .map_err(|_| "active run registry is unavailable".to_string())?;
        if active.insert(run_id, requested.clone()).is_some() {
            return Err(format!("Harness v3 run {run_id} is already active"));
        }
    }
    tauri::async_runtime::spawn(async move {
        let cancellation = CancellationTokenV3::new();
        let cancellation_bridge = cancellation.clone();
        let requested_bridge = requested.clone();
        let bridge = tokio::spawn(async move {
            while !requested_bridge.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            cancellation_bridge.cancel();
        });
        let outcome = async {
            let session = Arc::new(
                SshSession::connect(&profile, authentication)
                    .await
                    .map_err(|error| error.to_string())?,
            );
            let authority = Arc::new(DesktopToolAuthorityV3 {
                repository: repository.clone(),
            });
            let runtime = Arc::new(DesktopToolRuntimeV3 {
                repository: repository.clone(),
                session,
                project_id: project.id,
                project_root: project.remote_root.clone(),
                run_id,
            });
            let router = Arc::new(
                ToolRouterV3::new(
                    spec.tool_snapshot.clone(),
                    authority,
                    runtime,
                    Arc::new(NoopToolAuditSinkV3),
                    spec.limits.max_parallel_read_only as usize,
                )
                .map_err(|error| error.to_string())?,
            );
            let model = Arc::new(model);
            let sink = Arc::new(RepositoryEventSinkV3 {
                repository: repository.clone(),
                app: app.clone(),
            });
            let engine = if existing_events.is_empty() {
                HarnessV3Engine::new(spec.clone(), model.clone(), model, router, sink)
            } else {
                HarnessV3Engine::resume(
                    spec.clone(),
                    model.clone(),
                    model,
                    router,
                    sink,
                    existing_events,
                )
                .map_err(|error| error.to_string())?
            };
            engine
                .run(cancellation)
                .await
                .map_err(|error| error.to_string())
        }
        .await;
        if let Err(error) = outcome {
            append_failure(&repository, &app, &spec, &error);
        }
        if let Ok(events) = repository.agent_run_events_v3(run_id)
            && let Ok(state) = AgentRunStateV3::replay(&spec, &events)
        {
            let snapshot = AgentSnapshotV3 {
                run_id,
                project_id: spec.project_id,
                conversation_id: spec.conversation_id,
                last_sequence: state.last_sequence,
                last_event_hash: state.last_event_hash.clone(),
                state,
            };
            let _ = repository.save_agent_snapshot_v3(&snapshot);
        }
        active_runs
            .lock()
            .expect("active run registry")
            .remove(&run_id);
        bridge.abort();
    });
    Ok(())
}

pub fn list_events(repository: &Repository, run_id: Uuid) -> Result<Vec<AgentRunEventV3>, String> {
    repository
        .agent_run_events_v3(run_id)
        .map_err(|error| error.to_string())
}

pub fn list_events_for_context(
    repository: &Repository,
    project_id: Uuid,
    conversation_id: Option<Uuid>,
) -> Result<Vec<AgentRunEventV3>, String> {
    repository
        .agent_run_events_for_context_v3(project_id, conversation_id)
        .map_err(|error| error.to_string())
}

pub fn answer_question(
    repository: &Repository,
    app: &AppHandle,
    run_id: Uuid,
    question_id: String,
    answer: String,
) -> Result<(), String> {
    if answer.trim().is_empty() {
        return Err("answer is required".into());
    }
    let events = repository
        .agent_run_events_v3(run_id)
        .map_err(|error| error.to_string())?;
    let previous = events
        .last()
        .ok_or_else(|| "Harness v3 run has no events".to_string())?;
    if !events.iter().any(|event| {
        matches!(
            &event.event,
            AgentRunEventKindV3::UserInputRequested { question_id: expected, .. }
                if expected == &question_id
        )
    }) {
        return Err("question does not belong to this run".into());
    }
    let event = AgentRunEventV3::next(
        previous,
        chrono::Utc::now(),
        AgentRunEventKindV3::UserInputAnswered {
            question_id,
            answer: answer.trim().into(),
        },
    )
    .map_err(|error| error.to_string())?;
    repository
        .append_agent_run_event_v3(&event)
        .map_err(|error| error.to_string())?;
    let _ = app.emit(AGENT_RUN_EVENT_V3_CHANNEL, &event);
    Ok(())
}

pub fn cancel_persisted(
    repository: &Repository,
    app: &AppHandle,
    run_id: Uuid,
) -> Result<(), String> {
    let spec: AgentRunSpecV3 = repository
        .get_json("agent_run_spec_v3", &run_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Harness v3 run does not exist".to_string())?;
    let events = repository
        .agent_run_events_v3(run_id)
        .map_err(|error| error.to_string())?;
    let state = AgentRunStateV3::replay(&spec, &events).map_err(|error| error.to_string())?;
    if matches!(
        state.status,
        RunStatusV3::Completed | RunStatusV3::Cancelled | RunStatusV3::Failed
    ) {
        return Ok(());
    }
    let previous = events
        .last()
        .ok_or_else(|| "Harness v3 run has no events".to_string())?;
    let event = AgentRunEventV3::next(
        previous,
        chrono::Utc::now(),
        AgentRunEventKindV3::RunCancelled,
    )
    .map_err(|error| error.to_string())?;
    repository
        .append_agent_run_event_v3(&event)
        .map_err(|error| error.to_string())?;
    let _ = app.emit(AGENT_RUN_EVENT_V3_CHANNEL, event);
    Ok(())
}

#[tauri::command]
pub fn list_agent_run_events_v3(
    state: State<'_, AppState>,
    run_id: Option<Uuid>,
    project_id: Option<Uuid>,
    conversation_id: Option<Uuid>,
) -> Result<Vec<AgentRunEventV3>, String> {
    match (run_id, project_id) {
        (Some(run_id), _) => list_events(&state.repository, run_id),
        (None, Some(project_id)) => {
            list_events_for_context(&state.repository, project_id, conversation_id)
        }
        (None, None) => Err("run_id or project_id is required".into()),
    }
}

#[tauri::command]
pub fn answer_agent_run_question_v3(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
    question_id: String,
    answer: String,
) -> Result<(), String> {
    answer_question(&state.repository, &app, run_id, question_id, answer)
}

struct RepositoryEventSinkV3 {
    repository: Repository,
    app: AppHandle,
}

impl EventSinkV3 for RepositoryEventSinkV3 {
    fn append(&self, event: &AgentRunEventV3) -> Result<(), String> {
        self.repository
            .append_agent_run_event_v3(event)
            .map_err(|error| error.to_string())?;
        let _ = self.app.emit(AGENT_RUN_EVENT_V3_CHANNEL, event);
        Ok(())
    }
}

struct DesktopToolAuthorityV3 {
    repository: Repository,
}

impl ToolAuthorityV3 for DesktopToolAuthorityV3 {
    fn authorize(
        &self,
        definition: &ToolDefinitionV3,
        _request: &ToolCallRequestV3,
    ) -> AuthorityDecisionV3 {
        if definition.id.starts_with("mcp::") {
            return match approved_mcp_tool_definitions_v3(&self.repository) {
                Ok(current) if current.iter().any(|entry| entry.id == definition.id) => {
                    AuthorityDecisionV3::Allowed
                }
                Ok(_) => AuthorityDecisionV3::Denied {
                    reason: "MCP enabled/declaration/tool approval changed".into(),
                },
                Err(error) => AuthorityDecisionV3::Denied { reason: error },
            };
        }
        AuthorityDecisionV3::Allowed
    }
}

struct DesktopToolRuntimeV3 {
    repository: Repository,
    session: Arc<SshSession>,
    project_id: Uuid,
    project_root: String,
    run_id: Uuid,
}

#[async_trait]
impl ToolRuntimeV3 for DesktopToolRuntimeV3 {
    async fn execute(
        &self,
        _definition: &ToolDefinitionV3,
        request: &ToolCallRequestV3,
        cancellation: &CancellationTokenV3,
    ) -> ToolOutcomeV3 {
        if cancellation.is_cancelled() {
            return failed_outcome(request, ToolOutcomeStatusV3::Cancelled, "run cancelled");
        }
        let result = match request.tool_id.as_str() {
            "remote.list" => self.remote_list(request).await,
            "remote.read" => self.remote_read(request).await,
            "remote.write" => self.remote_write(request).await,
            "remote.exec" => self.remote_exec(request, None).await,
            "kernel.execute" => self.kernel_execute(request).await,
            "artifact.verify" => self.artifact_verify(request).await,
            id if id.starts_with("mcp::") => self.mcp(request).await,
            _ => Err(format!(
                "tool {} is handled by the coordinator",
                request.tool_id
            )),
        };
        match result {
            Ok((content, structured, provenance)) => ToolOutcomeV3 {
                call_id: request.call_id.clone(),
                status: ToolOutcomeStatusV3::Succeeded,
                model_content: content,
                structured_result: Some(structured),
                error: None,
                truncated: false,
                provenance,
            },
            Err(error) => failed_outcome(request, ToolOutcomeStatusV3::Failed, &error),
        }
    }
}

impl DesktopToolRuntimeV3 {
    async fn remote_list(
        &self,
        request: &ToolCallRequestV3,
    ) -> Result<(String, Value, Vec<String>), String> {
        let relative = request
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(".");
        let max_entries = request
            .arguments
            .get("max_entries")
            .and_then(Value::as_u64)
            .unwrap_or(200)
            .clamp(1, 2_000);
        let remote = project_path(&self.project_root, relative)?;
        let command = format!(
            "find {} -maxdepth 2 -mindepth 1 -printf '%y\\t%s\\t%p\\n' | head -n {}",
            shell_quote(&remote),
            max_entries
        );
        let output = self
            .session
            .execute(&command)
            .await
            .map_err(|error| error.to_string())?;
        if output.status != 0 {
            return Err(output.stderr);
        }
        Ok((
            output.stdout.clone(),
            json!({"path":relative,"entries":output.stdout.lines().count()}),
            vec![format!("remote:{relative}")],
        ))
    }

    async fn remote_read(
        &self,
        request: &ToolCallRequestV3,
    ) -> Result<(String, Value, Vec<String>), String> {
        let relative = required_string(&request.arguments, "path")?;
        let max_bytes = request
            .arguments
            .get("max_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(64 * 1024)
            .clamp(1, 1024 * 1024);
        let remote = project_path(&self.project_root, relative)?;
        let bytes = self
            .session
            .read_file_limited(&remote, max_bytes)
            .await
            .map_err(|error| error.to_string())?;
        let sha256 = sha256_bytes(&bytes);
        let content = String::from_utf8_lossy(&bytes).into_owned();
        Ok((
            content,
            json!({"path":relative,"size_bytes":bytes.len(),"sha256":sha256}),
            vec![format!("remote:{relative}"), format!("sha256:{sha256}")],
        ))
    }

    async fn remote_write(
        &self,
        request: &ToolCallRequestV3,
    ) -> Result<(String, Value, Vec<String>), String> {
        let relative = required_string(&request.arguments, "path")?;
        let content = required_string(&request.arguments, "content")?;
        let remote = project_path(&self.project_root, relative)?;
        let temporary = format!("{remote}.{}.tmp", safe_call_id(&request.call_id));
        if let Some(parent) = remote.rsplit_once('/').map(|(parent, _)| parent) {
            self.session
                .execute_checked(&format!("mkdir -p {}", shell_quote(parent)))
                .await
                .map_err(|error| error.to_string())?;
        }
        self.session
            .upload_text(&temporary, content)
            .await
            .map_err(|error| error.to_string())?;
        self.session
            .execute_checked(&format!(
                "mv -- {} {}",
                shell_quote(&temporary),
                shell_quote(&remote)
            ))
            .await
            .map_err(|error| error.to_string())?;
        let sha256 = sha256_bytes(content.as_bytes());
        Ok((
            format!("atomically wrote {relative}"),
            json!({"path":relative,"size_bytes":content.len(),"sha256":sha256}),
            vec![format!("remote:{relative}"), format!("sha256:{sha256}")],
        ))
    }

    async fn remote_exec(
        &self,
        request: &ToolCallRequestV3,
        override_command: Option<String>,
    ) -> Result<(String, Value, Vec<String>), String> {
        let command =
            override_command.unwrap_or(required_string(&request.arguments, "command")?.to_owned());
        validate_agent_command(&command, &self.project_root)?;
        let wrapped = format!(
            "cd {} && /bin/sh -c {}",
            shell_quote(&self.project_root),
            shell_quote(&command)
        );
        let output = self
            .session
            .execute(&wrapped)
            .await
            .map_err(|error| error.to_string())?;
        let log_root = format!("{}/.omicsops/runs/{}/logs", self.project_root, self.run_id);
        self.session
            .execute_checked(&format!("mkdir -p {}", shell_quote(&log_root)))
            .await
            .map_err(|error| error.to_string())?;
        let base = safe_call_id(&request.call_id);
        let stdout_path = format!("{log_root}/{base}.stdout.log");
        let stderr_path = format!("{log_root}/{base}.stderr.log");
        self.session
            .upload_text(&stdout_path, &output.stdout)
            .await
            .map_err(|error| error.to_string())?;
        self.session
            .upload_text(&stderr_path, &output.stderr)
            .await
            .map_err(|error| error.to_string())?;
        let (stdout_preview, stdout_truncated) = preview(&output.stdout, 32 * 1024);
        let (stderr_preview, stderr_truncated) = preview(&output.stderr, 32 * 1024);
        let structured = json!({
            "status":output.status,
            "stdout_log":{"path":stdout_path,"size_bytes":output.stdout.len(),"sha256":sha256_bytes(output.stdout.as_bytes())},
            "stderr_log":{"path":stderr_path,"size_bytes":output.stderr.len(),"sha256":sha256_bytes(output.stderr.as_bytes())},
            "stdout_truncated":stdout_truncated,
            "stderr_truncated":stderr_truncated
        });
        let content = format!(
            "exit_status={}\nSTDOUT\n{}\nSTDERR\n{}",
            output.status, stdout_preview, stderr_preview
        );
        if output.status == 0 {
            Ok((
                content,
                structured,
                vec![format!("remote-command:{}", request.call_id)],
            ))
        } else {
            Err(content)
        }
    }

    async fn kernel_execute(
        &self,
        request: &ToolCallRequestV3,
    ) -> Result<(String, Value, Vec<String>), String> {
        let language = required_string(&request.arguments, "language")?;
        let code = required_string(&request.arguments, "code")?;
        let (extension, executable) = match language {
            "python" => ("py", "python3"),
            "r" | "R" => ("R", "Rscript"),
            _ => return Err("kernel language must be python or r".into()),
        };
        let relative = format!(
            ".omicsops/runs/{}/kernel/{}.{}",
            self.run_id,
            safe_call_id(&request.call_id),
            extension
        );
        let remote = project_path(&self.project_root, &relative)?;
        if let Some(parent) = remote.rsplit_once('/').map(|(parent, _)| parent) {
            self.session
                .execute_checked(&format!("mkdir -p {}", shell_quote(parent)))
                .await
                .map_err(|error| error.to_string())?;
        }
        self.session
            .upload_text(&remote, code)
            .await
            .map_err(|error| error.to_string())?;
        self.remote_exec(
            request,
            Some(format!("{executable} {}", shell_quote(&remote))),
        )
        .await
    }

    async fn artifact_verify(
        &self,
        request: &ToolCallRequestV3,
    ) -> Result<(String, Value, Vec<String>), String> {
        let relative = required_string(&request.arguments, "path")?;
        let remote = project_path(&self.project_root, relative)?;
        let output = self
            .session
            .execute_checked(&format!(
                "test -f {0} && stat -c '%s' {0} && sha256sum {0}",
                shell_quote(&remote)
            ))
            .await
            .map_err(|error| error.to_string())?;
        let mut lines = output.stdout.lines();
        let size_bytes = lines
            .next()
            .ok_or_else(|| "artifact size is missing".to_string())?
            .trim()
            .parse::<u64>()
            .map_err(|_| "artifact size is invalid".to_string())?;
        let sha256 = lines
            .next()
            .and_then(|line| line.split_whitespace().next())
            .ok_or_else(|| "artifact SHA-256 is missing".to_string())?
            .to_ascii_lowercase();
        if let Some(expected) = request
            .arguments
            .get("expected_sha256")
            .and_then(Value::as_str)
            && !sha256.eq_ignore_ascii_case(expected)
        {
            return Err(format!(
                "artifact checksum mismatch: expected {expected}, got {sha256}"
            ));
        }
        if let Some(minimum) = request
            .arguments
            .get("minimum_bytes")
            .and_then(Value::as_u64)
            && size_bytes < minimum
        {
            return Err(format!("artifact is {size_bytes} bytes, below {minimum}"));
        }
        Ok((
            format!("verified {relative}: {size_bytes} bytes sha256={sha256}"),
            json!({"path":relative,"size_bytes":size_bytes,"sha256":sha256}),
            vec![format!("remote:{relative}"), format!("sha256:{sha256}")],
        ))
    }

    async fn mcp(
        &self,
        request: &ToolCallRequestV3,
    ) -> Result<(String, Value, Vec<String>), String> {
        let parts = request.tool_id.splitn(3, "::").collect::<Vec<_>>();
        if parts.len() != 3 || parts[0] != "mcp" {
            return Err("invalid MCP tool id".into());
        }
        let server_id = parts[1]
            .parse::<Uuid>()
            .map_err(|_| "invalid MCP server id".to_string())?;
        let result = invoke_configured_mcp_tool_v3(
            &self.repository,
            self.project_id,
            server_id,
            parts[2],
            request.arguments.clone(),
        )
        .await?;
        let value = result.result.unwrap_or_else(|| json!({}));
        Ok((
            serde_json::to_string(&value).map_err(|error| error.to_string())?,
            json!({"server_id":server_id,"tool":parts[2],"result":value,"audit_id":result.audit_id}),
            vec![format!("mcp-audit:{}", result.audit_id)],
        ))
    }
}

fn project_path(root: &str, relative: &str) -> Result<String, String> {
    let relative = relative.trim().replace('\\', "/");
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.split('/').any(|part| part == "..")
    {
        return Err("tool path must be a non-empty project-relative path".into());
    }
    let candidate = if relative == "." {
        root.trim_end_matches('/').to_owned()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), relative)
    };
    if candidate != root.trim_end_matches('/') {
        require_remote_descendant(root, &candidate).map_err(|error| error.to_string())?;
    }
    Ok(candidate)
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{field} is required"))
}

fn safe_call_id(call_id: &str) -> String {
    call_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(120)
        .collect()
}

fn preview(value: &str, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value.into(), false);
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), true)
}

fn sha256_bytes(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hex::encode(hasher.finalize())
}

fn failed_outcome(
    request: &ToolCallRequestV3,
    status: ToolOutcomeStatusV3,
    error: &str,
) -> ToolOutcomeV3 {
    ToolOutcomeV3 {
        call_id: request.call_id.clone(),
        status,
        model_content: format!("{} failed: {error}", request.tool_id),
        structured_result: Some(json!({"tool":request.tool_id,"error":error})),
        error: Some(error.into()),
        truncated: false,
        provenance: Vec::new(),
    }
}

fn append_failure(repository: &Repository, app: &AppHandle, spec: &AgentRunSpecV3, message: &str) {
    let Ok(events) = repository.agent_run_events_v3(spec.run_id) else {
        return;
    };
    let event = match events.last() {
        Some(previous) => AgentRunEventV3::next(
            previous,
            chrono::Utc::now(),
            AgentRunEventKindV3::RunFailed {
                message: message.into(),
            },
        ),
        None => AgentRunEventV3::first(
            spec,
            chrono::Utc::now(),
            AgentRunEventKindV3::RunFailed {
                message: message.into(),
            },
        ),
    };
    if let Ok(event) = event
        && repository.append_agent_run_event_v3(&event).is_ok()
    {
        let _ = app.emit(AGENT_RUN_EVENT_V3_CHANNEL, event);
    }
}

#[cfg(test)]
mod tests {
    use super::{DesktopToolRuntimeV3, project_path, run_spec_from_plan};
    use omicsops_adapters::{
        llm::{ProviderProtocol, UnifiedModelClient},
        persistence::Repository,
        ssh::{SshAuthentication, SshSession},
    };
    use omicsops_agent::harness_v3::{
        AgentRunEventV3, AgentRunSpecV3, AuthorityDecisionV3, CancellationTokenV3,
        CompletionCriterionV3, EventSinkV3, HarnessV3Engine, NoopToolAuditSinkV3, RunStatusV3,
        StaticToolAuthorityV3, ToolRouterV3, builtin_tool_definitions_v3,
    };
    use omicsops_core::{
        domain::{AuthenticationMethod, ConnectionProfile, ResourceLimits, StepRisk},
        plan_v2::{
            AnalysisPlanV2, PlanEnvironment, PlanStageV2, PolicyEnvelope, StepAction, StepSpecV2,
            VerificationSpec,
        },
    };
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use uuid::Uuid;

    #[test]
    fn project_paths_reject_absolute_and_parent_escape() {
        assert_eq!(
            project_path("/srv/project", "results/a.txt").unwrap(),
            "/srv/project/results/a.txt"
        );
        assert_eq!(project_path("/srv/project", ".").unwrap(), "/srv/project");
        assert!(project_path("/srv/project", "../secret").is_err());
        assert!(project_path("/srv/project", "/etc/passwd").is_err());
    }

    #[test]
    fn approved_v3_plan_builds_a_frozen_run_spec() {
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let plan = AnalysisPlanV2 {
            schema_version: 2,
            id: Uuid::new_v4(),
            title: "v3".into(),
            summary: "v3".into(),
            environment: PlanEnvironment::Micromamba {
                channels: vec![],
                dependencies: vec![],
            },
            stages: vec![PlanStageV2 {
                id: "agent".into(),
                goal: "Analyze".into(),
                dependencies: vec![],
                steps: vec![StepSpecV2 {
                    id: "run".into(),
                    title: "run".into(),
                    rationale: "test".into(),
                    dependencies: vec![],
                    action: StepAction::Tool {
                        tool_id: "agent.harness_v3".into(),
                        version: "3.0.0".into(),
                        arguments: json!({
                            "goal":"Analyze dynamically",
                            "completion_criteria":["verified h5ad"],
                            "tool_snapshot":builtin_tool_definitions_v3(),
                            "skill_references":[]
                        }),
                    },
                    working_directory: ".".into(),
                    resources: ResourceLimits::default(),
                    risk: StepRisk::Medium,
                    verifications: vec![VerificationSpec::ExitCode { expected: 0 }],
                    expected_artifacts: vec![],
                }],
            }],
            resource_budget: ResourceLimits::default(),
            policy: PolicyEnvelope {
                allowed_tools: vec!["agent.harness_v3".into()],
                allowed_domains: vec![],
                max_risk: StepRisk::Medium,
                allow_legacy_shell: false,
            },
            metadata: BTreeMap::from([
                ("harness_id".into(), "agent.harness_v3@3.0.0".into()),
                ("conversation_id".into(), conversation_id.to_string()),
            ]),
        };

        let spec = run_spec_from_plan(Uuid::new_v4(), project_id, &plan).unwrap();
        assert_eq!(spec.project_id, project_id);
        assert_eq!(spec.conversation_id, conversation_id);
        assert_eq!(spec.completion_criteria[0].id, "criterion-1");
        assert_eq!(spec.tool_snapshot.len(), 8);
    }

    #[derive(Default)]
    struct LiveEventSink(Mutex<Vec<AgentRunEventV3>>);

    impl EventSinkV3 for LiveEventSink {
        fn append(&self, event: &AgentRunEventV3) -> Result<(), String> {
            self.0.lock().expect("live event sink").push(event.clone());
            Ok(())
        }
    }

    #[tokio::test]
    #[ignore = "requires explicit live model/SSH credentials and an empty disposable OMICSOPS_LIVE_PBMC_ROOT"]
    async fn live_harness_v3_dynamically_delivers_pbmc3k_without_repository_workflow() {
        let remote_root = std::env::var("OMICSOPS_LIVE_PBMC_ROOT")
            .expect("OMICSOPS_LIVE_PBMC_ROOT must name an empty disposable remote directory");
        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            label: "Harness v3 PBMC acceptance".into(),
            host: std::env::var("OMICSOPS_LIVE_SSH_HOST").expect("live SSH host is required"),
            port: std::env::var("OMICSOPS_LIVE_SSH_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(22),
            username: std::env::var("OMICSOPS_LIVE_SSH_USER").expect("live SSH user is required"),
            authentication: AuthenticationMethod::Password,
            authentication_reference: "acceptance/environment".into(),
            host_key_fingerprint: Some(
                std::env::var("OMICSOPS_LIVE_SSH_FINGERPRINT")
                    .expect("pinned SSH fingerprint is required"),
            ),
        };
        let session = Arc::new(
            SshSession::connect(
                &profile,
                SshAuthentication::Password(
                    std::env::var("OMICSOPS_LIVE_SSH_PASSWORD")
                        .expect("live SSH password is required"),
                ),
            )
            .await
            .unwrap(),
        );
        let emptiness = session
            .execute_checked(&format!(
                "test -d {} && test -z \"$(find {} -mindepth 1 -maxdepth 1 -print -quit)\"",
                omicsops_core::project::shell_quote(&remote_root),
                omicsops_core::project::shell_quote(&remote_root),
            ))
            .await;
        assert!(
            emptiness.is_ok(),
            "the acceptance root must exist and be empty before the run"
        );

        let protocol = match std::env::var("OMICSOPS_LIVE_MODEL_PROTOCOL").as_deref() {
            Ok("anthropic") => ProviderProtocol::Anthropic,
            Ok("ollama") => ProviderProtocol::Ollama,
            Ok("openai") | Ok("open_ai_compatible") => ProviderProtocol::OpenAiCompatible,
            _ => panic!("OMICSOPS_LIVE_MODEL_PROTOCOL must be anthropic, openai, or ollama"),
        };
        let credential = std::env::var("OMICSOPS_LIVE_MODEL_CREDENTIAL").ok();
        let model = Arc::new(
            UnifiedModelClient::new(
                Uuid::new_v4(),
                protocol,
                std::env::var("OMICSOPS_LIVE_MODEL_BASE_URL")
                    .expect("model base URL is required")
                    .parse()
                    .unwrap(),
                std::env::var("OMICSOPS_LIVE_MODEL_NAME").expect("model name is required"),
                credential,
            )
            .unwrap(),
        );
        let project_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let tools = builtin_tool_definitions_v3();
        let spec = AgentRunSpecV3::new(
            run_id,
            project_id,
            Uuid::new_v4(),
            format!(
                "Starting from the empty approved remote project {remote_root}, dynamically inspect and obtain the public 10x PBMC3k input, create a project-local isolated environment, write task-specific analysis code, execute and repair it, and verify all outputs. Do not invoke or reproduce any repository workflow or bundled PBMC script. Deliver and artifact.verify: results/counts_summary.json, results/qc_metrics.tsv, results/reproducibility.json (seed and package versions), results/pbmc3k.h5ad, results/cell_qc.tsv, results/umap.png, results/report.html, and results/hashes.sha256. Preserve raw counts, state statistical assumptions and filtered-only ambient-RNA limitations truthfully, and make report numbers agree with tables."
            ),
            vec![
                CompletionCriterionV3::new(
                    "dynamic",
                    "input inspected and task-specific code generated without repository PBMC workflow assets",
                ),
                CompletionCriterionV3::new(
                    "environment",
                    "project-local isolated environment and package versions recorded",
                ),
                CompletionCriterionV3::new(
                    "science",
                    "counts, QC, seed, assumptions, and limitations recorded",
                ),
                CompletionCriterionV3::new(
                    "artifacts",
                    "h5ad, tables, figure, HTML report, and hashes verified",
                ),
            ],
            tools.clone(),
            vec![],
        );
        let repository = Repository::open_in_memory().unwrap();
        let runtime = Arc::new(DesktopToolRuntimeV3 {
            repository,
            session,
            project_id,
            project_root: remote_root,
            run_id,
        });
        let router = Arc::new(
            ToolRouterV3::new(
                tools,
                Arc::new(StaticToolAuthorityV3(AuthorityDecisionV3::Allowed)),
                runtime,
                Arc::new(NoopToolAuditSinkV3),
                4,
            )
            .unwrap(),
        );
        let sink = Arc::new(LiveEventSink::default());
        let result = tokio::time::timeout(
            Duration::from_secs(45 * 60),
            HarnessV3Engine::new(spec, model.clone(), model, router, sink.clone())
                .run(CancellationTokenV3::new()),
        )
        .await
        .expect("live PBMC acceptance timed out")
        .unwrap();
        assert_eq!(result.status, RunStatusV3::Completed);
        let forbidden = [
            "workflows/scrna-pbmc",
            "analyze_pbmc.py",
            "pbmc_reference_plan",
        ];
        let serialized = serde_json::to_string(&*sink.0.lock().unwrap()).unwrap();
        assert!(
            forbidden.iter().all(|needle| !serialized.contains(needle)),
            "repository PBMC workflow was referenced"
        );
        for required in [
            "counts_summary.json",
            "qc_metrics.tsv",
            "reproducibility.json",
            "pbmc3k.h5ad",
            "cell_qc.tsv",
            "umap.png",
            "report.html",
            "hashes.sha256",
        ] {
            assert!(
                result
                    .ledger
                    .verified_artifacts
                    .iter()
                    .any(|artifact| artifact.path.ends_with(required)),
                "missing verified artifact {required}"
            );
        }
    }
}

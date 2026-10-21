use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use omicsops_adapters::credentials::{CredentialVault, SystemCredentialVault};
use omicsops_agent_core::{AgentCoreV4, AgentLimitsV4, EventStoreV4};
use omicsops_core::workspace::{
    Conversation, ModelProfile, ModelProviderKind, Project, ProjectTemplate,
};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ApprovalPolicyV4, AutonomyModeV4, ComputeBackendKindV4,
    ComputeSelectionV4, ContextArchiveV4, ContextCheckpointV4, ConversationAgentPreferencesV4,
    NetworkPolicyV4, RunModeV4, RunServiceTierV4, ToolEffectV4,
};
use omicsops_store::Store;
use sqlx::{Connection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

use super::{AppState, DirectRunSnapshotV4, StartDirectV4Request, compose, prepare_direct_run_v4};

const GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
const GO_MODELS: [&str; 3] = ["glm-5.3", "glm-5.3-flash", "deepseek-v4.1-flash"];

fn go_acceptance_model_allowed(model: &str) -> bool {
    GO_MODELS.contains(&model)
}

#[test]
fn live_go_acceptance_model_allowlist_is_exact() {
    assert!(go_acceptance_model_allowed("glm-5.3"));
    assert!(go_acceptance_model_allowed("glm-5.3-flash"));
    assert!(go_acceptance_model_allowed("deepseek-v4.1-flash"));
    assert!(!go_acceptance_model_allowed("glm-5.3-flash-preview"));
    assert!(!go_acceptance_model_allowed("vendor/glm-5.3"));
    assert!(!go_acceptance_model_allowed("deepseek-v4.1"));
    assert!(!go_acceptance_model_allowed(
        "deepseek-v4.1-flash-vision-exp"
    ));
    assert!(!go_acceptance_model_allowed("vendor/deepseek-v4.1-flash"));
}

struct TemporaryEventStore {
    repository: Store,
    run_id: Uuid,
    reasoning_observation: Mutex<ReasoningObservation>,
    inspection_guard: bool,
    rejected_dispatches: AtomicUsize,
    model_request_count: AtomicUsize,
}

#[derive(Debug, Default)]
struct ReasoningObservation {
    started_at: Option<Instant>,
    active_attempt: Option<Uuid>,
    attempts_with_content: BTreeSet<Uuid>,
    nonempty_snapshots: usize,
    snapshots_before_result: usize,
    clear_updates: usize,
    max_snapshot_bytes: usize,
    first_nonempty_ms: Option<u128>,
    last_nonempty_ms: Option<u128>,
}

impl ReasoningObservation {
    fn observe_event(&mut self, kind: &AgentEventKindV4) {
        match kind {
            AgentEventKindV4::ModelRequestStarted { request } => {
                self.started_at.get_or_insert_with(Instant::now);
                self.active_attempt = Some(request.attempt_id);
            }
            AgentEventKindV4::ModelText { .. }
            | AgentEventKindV4::ToolRequested { .. }
            | AgentEventKindV4::RunCompleted
            | AgentEventKindV4::RunFailed { .. }
            | AgentEventKindV4::RunNeedsAttention { .. } => {
                self.active_attempt = None;
            }
            _ => {}
        }
    }

    fn observe_preview(&mut self, attempt_id: Uuid, text: Option<&str>) {
        let Some(text) = text else {
            self.clear_updates += 1;
            if self.active_attempt == Some(attempt_id) {
                self.active_attempt = None;
            }
            return;
        };
        if text.is_empty() {
            return;
        }
        let elapsed_ms = self
            .started_at
            .map(|started_at| started_at.elapsed().as_millis());
        self.nonempty_snapshots += 1;
        if self.active_attempt == Some(attempt_id) {
            self.snapshots_before_result += 1;
        }
        self.attempts_with_content.insert(attempt_id);
        self.max_snapshot_bytes = self.max_snapshot_bytes.max(text.len());
        self.first_nonempty_ms = self.first_nonempty_ms.or(elapsed_ms);
        self.last_nonempty_ms = elapsed_ms;
    }
}

#[test]
fn reasoning_observation_counts_only_matching_preterminal_snapshots_without_retaining_text() {
    let attempt_id = Uuid::new_v4();
    let other_attempt_id = Uuid::new_v4();
    let mut observation = ReasoningObservation {
        started_at: Some(Instant::now()),
        active_attempt: Some(attempt_id),
        ..Default::default()
    };
    let private_text = "SECRET_REASONING_PAYLOAD";

    observation.observe_preview(other_attempt_id, Some(private_text));
    observation.observe_preview(attempt_id, Some(private_text));
    assert_eq!(observation.nonempty_snapshots, 2);
    assert_eq!(observation.snapshots_before_result, 1);

    observation.observe_preview(attempt_id, None);
    observation.observe_preview(attempt_id, Some(private_text));
    observation.active_attempt = Some(attempt_id);
    observation.observe_event(&AgentEventKindV4::RunCompleted);
    observation.observe_preview(attempt_id, Some(private_text));
    assert_eq!(observation.nonempty_snapshots, 4);
    assert_eq!(observation.snapshots_before_result, 1);
    assert_eq!(observation.clear_updates, 1);
    assert!(!format!("{observation:?}").contains(private_text));
}

#[async_trait]
impl EventStoreV4 for TemporaryEventStore {
    fn preview_model_reasoning(&self, run_id: Uuid, attempt_id: Uuid, text: Option<&str>) {
        if run_id == self.run_id {
            self.reasoning_observation
                .lock()
                .unwrap()
                .observe_preview(attempt_id, text);
        }
    }

    async fn append(&self, event: &AgentEventV4) -> Result<(), String> {
        // This fixture keeps the full production tool directory and RiskBased
        // routing. Low-risk runtime code can otherwise be auto-approved, so the
        // inspection case must stop before any side-effect executor is called.
        if self.inspection_guard {
            if matches!(&event.event, AgentEventKindV4::ToolDispatchStarted { effect, .. }
                if *effect != ToolEffectV4::ReadOnly)
            {
                self.rejected_dispatches.fetch_add(1, Ordering::SeqCst);
                return Err("live inspection rejected side-effect dispatch".into());
            }
            if matches!(event.event, AgentEventKindV4::ModelRequestStarted { .. })
                && self.model_request_count.fetch_add(1, Ordering::SeqCst) >= 4
            {
                return Err("live inspection reached its four-request limit".into());
            }
        }
        self.repository
            .append_agent_event_v4_with_conversation(event)
            .await
            .map_err(|error| error.to_string())?;
        self.reasoning_observation
            .lock()
            .unwrap()
            .observe_event(&event.event);
        Ok(())
    }

    async fn load(&self, run_id: Uuid) -> Result<Vec<AgentEventV4>, String> {
        self.repository
            .agent_events_v4(run_id)
            .await
            .map_err(|error| error.to_string())
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

#[tokio::test]
async fn inspection_guard_blocks_side_effects_and_fifth_model_request_before_persistence() {
    let repository = Store::open_in_memory().await.unwrap();
    let now = chrono::Utc::now();
    let project = Project::new(
        Uuid::new_v4(),
        "synthetic guard fixture",
        "synthetic-workspace",
        ProjectTemplate::Blank,
        now,
    );
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "guard fixture", now);
    repository.save_project(&project).await.unwrap();
    repository.save_conversation(&conversation).await.unwrap();

    for inspection_guard in [true, false] {
        let run_id = Uuid::new_v4();
        let events = TemporaryEventStore {
            repository: repository.clone(),
            run_id,
            reasoning_observation: Mutex::new(ReasoningObservation::default()),
            inspection_guard,
            rejected_dispatches: AtomicUsize::new(0),
            model_request_count: AtomicUsize::new(0),
        };
        let mut previous = AgentEventV4::first(
            run_id,
            project.id,
            conversation.id,
            now,
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        events.append(&previous).await.unwrap();
        for effect in [
            ToolEffectV4::ReadOnly,
            ToolEffectV4::Mutating,
            ToolEffectV4::Runtime,
            ToolEffectV4::Network,
            ToolEffectV4::Delegation,
        ] {
            let next = AgentEventV4::next(
                &previous,
                now,
                AgentEventKindV4::ToolDispatchStarted {
                    call_id: "synthetic-call".into(),
                    tool_id: "synthetic-tool".into(),
                    effect,
                    idempotency_key: "synthetic-key".into(),
                },
            );
            let result = events.append(&next).await;
            if inspection_guard && effect != ToolEffectV4::ReadOnly {
                assert_eq!(
                    result.unwrap_err(),
                    "live inspection rejected side-effect dispatch"
                );
            } else {
                result.unwrap();
                previous = next;
            }
        }
        for attempt in 1..=5 {
            let next = AgentEventV4::next(
                &previous,
                now,
                AgentEventKindV4::ModelRequestStarted {
                    request: omicsops_protocol::ModelRequestStartedV4 {
                        logical_request_id: Uuid::new_v4(),
                        attempt_id: Uuid::new_v4(),
                        model_profile_id: Uuid::new_v4(),
                        model_configuration_hash: None,
                        context_limit_tokens: None,
                        context_limit_source: Default::default(),
                        serialized_request_bytes: None,
                        image_count: None,
                        image_bound_tokens: None,
                        breakdown: None,
                    },
                },
            );
            let result = events.append(&next).await;
            if inspection_guard && attempt == 5 {
                assert_eq!(
                    result.unwrap_err(),
                    "live inspection reached its four-request limit"
                );
            } else {
                result.unwrap();
                previous = next;
            }
        }
        let recorded = events.load(run_id).await.unwrap();
        assert_eq!(recorded.last(), Some(&previous));
        assert_eq!(recorded.len(), if inspection_guard { 6 } else { 11 });
        assert_eq!(
            events.rejected_dispatches.load(Ordering::SeqCst),
            if inspection_guard { 4 } else { 0 }
        );
    }
}

async fn selected_saved_profile() -> Result<ModelProfile, String> {
    let profile_id = std::env::var("OMICSOPS_LIVE_MODEL_PROFILE_ID")
        .map_err(|_| {
            "set OMICSOPS_LIVE_MODEL_PROFILE_ID to an existing saved OpenCode Go profile UUID"
                .to_owned()
        })?
        .parse::<Uuid>()
        .map_err(|_| "OMICSOPS_LIVE_MODEL_PROFILE_ID must be a UUID".to_owned())?;
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from).ok_or(
        "APPDATA is unavailable; this acceptance test must run in the Windows desktop user session",
    )?;
    let database = app_data.join("io.omicsops.desktop").join("omicsops.db");
    if !database.is_file() {
        return Err(format!(
            "the OmicsOps desktop database does not exist at {}",
            database.display()
        ));
    }

    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(false)
        .read_only(true);
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| {
            format!("could not open the OmicsOps desktop database read-only: {error}")
        })?;
    let value_json =
        sqlx::query_scalar::<_, String>("SELECT value_json FROM model_profiles WHERE id = ?1")
            .bind(profile_id.to_string())
            .fetch_optional(&mut connection)
            .await
            .map_err(|error| format!("could not read the selected model profile: {error}"))?;
    connection
        .close()
        .await
        .map_err(|error| format!("could not close the read-only profile connection: {error}"))?;
    let value_json = value_json.ok_or_else(|| {
        "OMICSOPS_LIVE_MODEL_PROFILE_ID does not identify a saved model profile".to_owned()
    })?;
    serde_json::from_str(&value_json)
        .map_err(|error| format!("the selected saved model profile is invalid: {error}"))
}

fn refreshed_go_profile(source: &ModelProfile) -> Result<ModelProfile, String> {
    if source.provider != ModelProviderKind::OpenAiCompatible
        || source.base_url.trim_end_matches('/') != GO_BASE_URL
        || !go_acceptance_model_allowed(&source.model)
    {
        return Err(format!(
            "the selected profile must use open_ai_compatible, {GO_BASE_URL}, and one of {} exactly",
            GO_MODELS.join(", ")
        ));
    }
    let mut refreshed =
        crate::model_commands::model_profile_from_request(omicsops_dto::SaveModelProfileRequest {
            id: Some(source.id),
            label: source.label.clone(),
            provider: "open_ai_compatible".into(),
            base_url: source.base_url.clone(),
            model: source.model.clone(),
            credential: None,
            context_window_tokens: source.context_window_tokens,
            refresh_catalog: true,
            reasoning_effort: Some(source.reasoning_effort.clone()),
            fast_mode: Some(source.fast_mode),
            delegated_model_profile_id: Some(None),
            cli_executable: None,
        })?;
    refreshed.credential_reference = source.credential_reference.clone();
    let capabilities = refreshed
        .catalog_capabilities
        .as_ref()
        .ok_or("the selected endpoint/model has no exact trusted entry in the compiled catalog")?;
    let expected_output_limit = if source.model == "deepseek-v4.1-flash" {
        384_000
    } else {
        131_072
    };
    if capabilities.source_provider != "opencode-go"
        || capabilities.context_limit != 1_000_000
        || capabilities.output_limit != expected_output_limit
        || !refreshed.supports_tools
    {
        return Err(format!(
            "the compiled OpenCode Go model catalog contract is not context=1000000, output={expected_output_limit}, tools=true"
        ));
    }
    if refreshed.effective_context_window_tokens() != 1_000_000 {
        return Err(
            "the selected profile has a lower explicit context bound; clear it before running the full Go Agent acceptance"
                .into(),
        );
    }
    let reference = refreshed
        .credential_reference
        .as_deref()
        .ok_or("the selected OpenCode Go profile has no credential reference")?;
    if SystemCredentialVault
        .get(reference)
        .map_err(|error| format!("could not read the selected profile credential from Windows Credential Manager: {error}"))?
        .is_none()
    {
        return Err("the selected OpenCode Go profile credential is missing from Windows Credential Manager".into());
    }
    Ok(refreshed)
}

#[derive(Clone, Copy)]
enum LiveGoCase {
    Nonce { auto_review: bool },
    EnvironmentInspection,
}

async fn run_live_go_agent_acceptance(case: LiveGoCase) -> Result<(), String> {
    let inspection = matches!(case, LiveGoCase::EnvironmentInspection);
    let auto_review = matches!(case, LiveGoCase::Nonce { auto_review: true });
    let source = selected_saved_profile().await?;
    let profile = refreshed_go_profile(&source)?;
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let nonce = Uuid::new_v4().simple().to_string();
    let filename = format!("opencode-go-live-{}.txt", Uuid::new_v4().simple());
    if !inspection {
        std::fs::write(
            directory.path().join(&filename),
            format!("OpenCode Go Agent acceptance nonce: {nonce}\n"),
        )
        .map_err(|error| error.to_string())?;
    }

    let repository = Store::open_in_memory()
        .await
        .map_err(|error| error.to_string())?;
    let now = chrono::Utc::now();
    let project = Project::new(
        Uuid::new_v4(),
        "OpenCode Go live acceptance",
        directory.path().to_string_lossy(),
        ProjectTemplate::Blank,
        now,
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "OpenCode Go live acceptance",
        now,
    );
    repository
        .save_project(&project)
        .await
        .map_err(|error| error.to_string())?;
    repository
        .save_conversation(&conversation)
        .await
        .map_err(|error| error.to_string())?;
    repository
        .save_model_profile(&profile)
        .await
        .map_err(|error| error.to_string())?;

    let state = AppState {
        repository: repository.clone(),
        credentials: SystemCredentialVault,
        subscription_models: std::sync::Arc::new(
            crate::subscription_models::SubscriptionLoginManager::system(Default::default())?,
        ),
        mcp_sessions: omicsops_mcp::McpSessionManager::new(),
        active_runs: Default::default(),
        live_runtime_registry_v4: Default::default(),
        skills_root: directory.path().join("skills"),
        file_baseline_storage: crate::workspace_file_baselines::FileBaselineStorage::new(
            directory.path().join("baseline-app"),
        ),
        skills_gate: Default::default(),
        research_last_request: Default::default(),
        active_kernels: Default::default(),
        project_kernel_queues: Default::default(),
        sync_controls: Default::default(),
        browser: omicsops_browser::BrowserRuntime::new(
            directory.path().join("browser"),
            directory.path().join("extension"),
        ),
    };
    let selection = ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "local".into(),
        backend_kind: ComputeBackendKindV4::Local,
        autonomy_mode: AutonomyModeV4::Supervised,
        approval_policy: ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: NetworkPolicyV4::HostInherited,
        container_image: None,
    };
    let preferences = ConversationAgentPreferencesV4 {
        delegation_enabled: false,
        auto_review,
        memory_enabled: false,
        fast_mode: None,
    };
    let service_tier = RunServiceTierV4 { fast_mode: None };
    let reviewer_model =
        super::freeze_reviewer_model(&repository, &profile, &preferences, service_tier).await?;
    let run_id = Uuid::new_v4();
    let objective = if inspection {
        "帮我在当前执行环境下，检查有没有在此文件夹下准备好 Python 和 R 环境。只需检查并给出结果。"
            .into()
    } else {
        format!(
            "This is a bounded live acceptance task. Use project.read exactly once to read `{filename}`. The file contains a random validation nonce that is not present in this request. Do not guess it and do not call any other task tool. After reading the file, call agent.complete with the exact nonce in answer_markdown and cite the successful project.read event as evidence for the frozen completion criterion."
        )
    };
    let capabilities = if inspection {
        super::queue_execute_capabilities(&preferences)
    } else {
        BTreeSet::from(["project.read".into()])
    };
    let request = StartDirectV4Request {
        project_id: project.id,
        conversation_id: conversation.id,
        model_profile_id: profile.id,
        objective,
        compute_selection: selection.clone(),
        references: vec![],
        attachments: vec![],
    };
    let (record, spec) = prepare_direct_run_v4(
        &request,
        run_id,
        "[]",
        capabilities,
        DirectRunSnapshotV4 {
            model_configuration_hash: profile.execution_configuration_hash(),
            conversation_preferences: preferences,
            service_tier,
            reviewer_model,
            delegated_model: None,
            reference_context: String::new(),
            input_images: vec![],
        },
        now,
    )?;
    repository
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            &record.status,
            &serde_json::to_value(&record).map_err(|error| error.to_string())?,
        )
        .await
        .map_err(|error| error.to_string())?;
    let events = TemporaryEventStore {
        repository: repository.clone(),
        run_id,
        reasoning_observation: Mutex::new(ReasoningObservation::default()),
        inspection_guard: inspection,
        rejected_dispatches: AtomicUsize::new(0),
        model_request_count: AtomicUsize::new(0),
    };
    events
        .append(&AgentEventV4::first(
            run_id,
            project.id,
            conversation.id,
            now,
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        ))
        .await?;
    let first = events
        .load(run_id)
        .await?
        .pop()
        .ok_or("missing run-created event")?;
    events
        .append(&AgentEventV4::next(
            &first,
            now,
            AgentEventKindV4::RunSpecFrozen {
                approval_hash: spec.approval_hash.clone().ok_or("missing approval hash")?,
                spec_hash: spec.spec_hash.clone().ok_or("missing spec hash")?,
            },
        ))
        .await?;

    let (model, tools) = compose(
        &state,
        &project,
        &selection,
        profile.id,
        run_id,
        conversation.id,
        Some(&spec.plan.requested_capabilities),
        None,
        None,
        spec.model_configuration_hash.as_deref(),
        true,
        &[],
        spec.conversation_preferences.as_ref(),
        spec.service_tier.as_ref(),
        spec.reviewer_model.as_ref(),
    )
    .await?;
    let core = AgentCoreV4 {
        model: model.as_ref(),
        tools: tools.registry.as_ref(),
        events: &events,
        science: None,
    };
    let mut limits = AgentLimitsV4::ordinary(4);
    limits.max_tool_calls = if inspection { 4 } else { 3 };
    limits.max_model_retries = 0;
    let started_at = Instant::now();
    let execution = core
        .execute_with_limits(&spec, limits, &AtomicBool::new(false))
        .await;

    if inspection {
        let recorded = events.load(run_id).await?;
        let model_requests = recorded
            .iter()
            .filter(|event| matches!(event.event, AgentEventKindV4::ModelRequestStarted { .. }))
            .count();
        let tool_requests = recorded
            .iter()
            .filter(|event| matches!(event.event, AgentEventKindV4::ToolRequested { .. }))
            .count();
        let inspection_requests = recorded.iter().filter(|event| matches!(&event.event, AgentEventKindV4::ToolRequested { call } if call.tool_id == "runtime.environment.inspect")).count();
        let successful_inspections = recorded.iter().filter(|event| matches!(&event.event, AgentEventKindV4::ToolFinished { outcome } if outcome.tool_id == "runtime.environment.inspect" && outcome.succeeded)).count();
        let runtime_dispatches = recorded.iter().filter(|event| matches!(&event.event, AgentEventKindV4::ToolDispatchStarted { tool_id, .. } if matches!(tool_id.as_str(), "runtime.execute" | "runtime.environment.ensure"))).count();
        let completed = recorded
            .iter()
            .any(|event| matches!(event.event, AgentEventKindV4::RunCompleted));
        let rejected_dispatches = events.rejected_dispatches.load(Ordering::SeqCst);
        println!(
            "OpenCode Go environment inspection: status={}, elapsed_ms={}, model_requests={}, tool_requests={}, inspection_requests={}, successful_inspections={}, runtime_dispatches={}, rejected_dispatches={}, completed={}",
            if execution.is_ok() {
                "finished"
            } else {
                "failed"
            },
            started_at.elapsed().as_millis(),
            model_requests,
            tool_requests,
            inspection_requests,
            successful_inspections,
            runtime_dispatches,
            rejected_dispatches,
            completed,
        );
        if execution.is_err()
            || !completed
            || !(2..=4).contains(&model_requests)
            || tool_requests > 4
            || inspection_requests != 1
            || successful_inspections != 1
            || runtime_dispatches != 0
            || rejected_dispatches != 0
            || recorded
                .iter()
                .any(|event| matches!(event.event, AgentEventKindV4::ReviewerFinished { .. }))
        {
            return Err(
                "live environment inspection did not satisfy the bounded routing contract".into(),
            );
        }
        return Ok(());
    }

    let reasoning = events.reasoning_observation.lock().unwrap();
    println!(
        "OpenCode Go reasoning preview: model={}, nonempty_snapshots={}, snapshots_before_result={}, attempts_with_content={}, max_snapshot_bytes={}, clear_updates={}, first_nonempty_ms={:?}, last_nonempty_ms={:?}",
        profile.model,
        reasoning.nonempty_snapshots,
        reasoning.snapshots_before_result,
        reasoning.attempts_with_content.len(),
        reasoning.max_snapshot_bytes,
        reasoning.clear_updates,
        reasoning.first_nonempty_ms,
        reasoning.last_nonempty_ms,
    );
    let snapshots_before_result = reasoning.snapshots_before_result;
    drop(reasoning);
    execution.map_err(|error| error.to_string())?;
    if profile.model == "glm-5.3" && snapshots_before_result == 0 {
        return Err(
            "glm-5.3 produced no nonempty reasoning preview before a model result event".into(),
        );
    }

    let recorded = events.load(run_id).await?;
    let model_requests = recorded
        .iter()
        .filter(|event| matches!(event.event, AgentEventKindV4::ModelRequestStarted { .. }))
        .count();
    if model_requests < 2 {
        return Err(format!(
            "expected a model-tool-model exchange, but recorded only {model_requests} model request(s)"
        ));
    }
    let reads = recorded
        .iter()
        .filter_map(|event| match &event.event {
            AgentEventKindV4::ToolFinished { outcome }
                if outcome.tool_id == "project.read" && outcome.succeeded =>
            {
                Some(outcome)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if reads.len() != 1 || !reads[0].model_content.contains(&nonce) {
        return Err("the Agent did not successfully read the nonce file exactly once".into());
    }
    let final_answer = recorded.iter().rev().find_map(|event| match &event.event {
        AgentEventKindV4::CompletionProposalSubmitted { proposal } => {
            Some(proposal.answer_markdown.as_str())
        }
        _ => None,
    });
    if !final_answer.is_some_and(|answer| answer.contains(&nonce)) {
        return Err(
            "the final Agent answer did not contain the nonce returned by project.read".into(),
        );
    }
    let completed_index = recorded
        .iter()
        .position(|event| matches!(event.event, AgentEventKindV4::RunCompleted))
        .ok_or("the production Agent loop did not record RunCompleted")?;
    let reviewer_index = recorded
        .iter()
        .position(|event| matches!(event.event, AgentEventKindV4::ReviewerFinished { .. }));
    if auto_review && !reviewer_index.is_some_and(|index| index < completed_index) {
        return Err("automatic review did not record ReviewerFinished before RunCompleted".into());
    }
    if !auto_review && reviewer_index.is_some() {
        return Err("the review-disabled acceptance unexpectedly recorded ReviewerFinished".into());
    }
    if recorded.iter().any(|event| {
        matches!(
            &event.event,
            AgentEventKindV4::ToolFinished { outcome }
                if outcome.tool_id != "project.read" && !outcome.tool_id.starts_with("agent.")
        )
    }) {
        return Err("the bounded acceptance dispatched an unexpected task tool".into());
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires an existing saved OpenCode Go profile and its Windows keyring credential"]
async fn live_opencode_go_agent_reads_file_and_returns_nonce() {
    if let Err(error) = run_live_go_agent_acceptance(LiveGoCase::Nonce { auto_review: false }).await
    {
        panic!("OpenCode Go Agent live acceptance failed: {error}");
    }
}

#[tokio::test]
#[ignore = "requires an existing saved OpenCode Go profile and its Windows keyring credential"]
async fn live_opencode_go_agent_reviews_file_and_returns_nonce() {
    if let Err(error) = run_live_go_agent_acceptance(LiveGoCase::Nonce { auto_review: true }).await
    {
        panic!("OpenCode Go Agent reviewer live acceptance failed: {error}");
    }
}

#[tokio::test]
#[ignore = "requires one explicitly selected saved OpenCode Go profile; temporary local environment routing only, no SSH"]
async fn live_opencode_go_agent_inspects_selected_environment() {
    let started_at = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(180),
        run_live_go_agent_acceptance(LiveGoCase::EnvironmentInspection),
    )
    .await;
    let status = match result {
        Ok(Ok(())) => "passed",
        Ok(Err(_)) => "failed",
        Err(_) => "timed_out",
    };
    println!(
        "OpenCode Go environment acceptance: status={status}, elapsed_ms={}",
        started_at.elapsed().as_millis()
    );
    assert_eq!(
        status, "passed",
        "live environment routing acceptance failed; raw errors are intentionally not printed"
    );
}

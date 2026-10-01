//! Opt-in production subscription checks. Never mutates an existing project or database.
use super::*;
use omicsops_core::workspace::ModelProviderKind;
use omicsops_core::workspace::{Conversation, ProjectTemplate};
use omicsops_protocol::{ConversationAgentPreferencesV4, RunModeV4, RunServiceTierV4};
use omicsops_store::Store;
use sqlx::{Connection, sqlite::SqliteConnectOptions};
use std::time::Duration;

fn acceptance_profile_id(value: Option<&str>, disposable: Option<&str>) -> Result<Uuid, String> {
    if disposable != Some("1") {
        return Err("set OMICSOPS_LIVE_SUBSCRIPTION_DISPOSABLE=1 explicitly".into());
    }
    value
        .ok_or("set OMICSOPS_LIVE_SUBSCRIPTION_PROFILE_ID explicitly")?
        .parse()
        .map_err(|_| "subscription profile ID must be a UUID".into())
}

async fn read_selected_profile(database: &Path, id: Uuid) -> Result<ModelProfile, String> {
    let options = SqliteConnectOptions::new()
        .filename(database)
        .create_if_missing(false)
        .read_only(true);
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|_| "could not open selected database read-only")?;
    let value =
        sqlx::query_scalar::<_, String>("SELECT value_json FROM model_profiles WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&mut connection)
            .await
            .map_err(|_| "could not read selected profile")?;
    connection
        .close()
        .await
        .map_err(|_| "could not close read-only database")?;
    serde_json::from_str(&value.ok_or("selected profile does not exist")?)
        .map_err(|_| "invalid selected profile".into())
}

fn validate_acceptance_profile(
    profile: &ModelProfile,
    expected: ModelProviderKind,
    owned: &tempfile::TempDir,
    root: &Path,
    has_credential: bool,
    claude_ready: bool,
) -> Result<(), String> {
    if root.canonicalize().ok() != owned.path().canonicalize().ok()
        || std::fs::read_dir(root)
            .map_err(|_| "invalid disposable directory")?
            .next()
            .is_some()
    {
        return Err("acceptance must use its own empty temporary project".into());
    }
    if profile.provider != expected
        || !matches!(
            expected,
            ModelProviderKind::OpenAiCodex
                | ModelProviderKind::OpenAiResponses
                | ModelProviderKind::ClaudeCode
        )
    {
        return Err("subscription provider mismatch".into());
    }
    omicsops_core::workspace::validate_subscription_profile_fields(profile)?;
    if profile.model.trim().is_empty() || !profile.supports_tools || profile.fast_mode == Some(true)
    {
        return Err("invalid subscription acceptance model configuration".into());
    }
    if expected == ModelProviderKind::OpenAiResponses
        && !omicsops_core::workspace::OPENCODE_GO_RESPONSES_MODELS.contains(&profile.model.as_str())
    {
        return Err("Go acceptance requires an exact reviewed Responses model".into());
    }
    if expected == ModelProviderKind::OpenAiCodex && profile.subscription_account_ref.is_none() {
        return Err("Codex account binding missing".into());
    }
    if expected == ModelProviderKind::ClaudeCode {
        if !claude_ready {
            return Err("Claude subscription login or managed policy is unverified; generation remains disabled".into());
        }
    } else if !has_credential {
        return Err("selected subscription keyring credential missing".into());
    }
    Ok(())
}

async fn selected_profile(
    expected: ModelProviderKind,
    directory: &tempfile::TempDir,
) -> Result<ModelProfile, String> {
    if !cfg!(windows) {
        return Err("live subscription acceptance requires Windows".into());
    }
    let id = acceptance_profile_id(
        std::env::var("OMICSOPS_LIVE_SUBSCRIPTION_PROFILE_ID")
            .ok()
            .as_deref(),
        std::env::var("OMICSOPS_LIVE_SUBSCRIPTION_DISPOSABLE")
            .ok()
            .as_deref(),
    )?;
    let database = PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA unavailable")?)
        .join("io.omicsops.desktop")
        .join("omicsops.db");
    let mut profile = read_selected_profile(&database, id).await?;
    // Validate scope before reading any keyring entry or inspecting the CLI.
    validate_acceptance_profile(&profile, expected, directory, directory.path(), true, true)?;
    let has_credential = profile
        .credential_reference
        .as_deref()
        .map(|r| SystemCredentialVault.get(r))
        .transpose()
        .map_err(|_| "could not read selected credential")?
        .flatten()
        .is_some();
    let manager = crate::subscription_models::SubscriptionLoginManager::system(Default::default())?;
    let status = manager.status(&profile).await;
    validate_acceptance_profile(
        &profile,
        expected,
        directory,
        directory.path(),
        has_credential,
        status.authenticated && status.error_code.is_none(),
    )?;
    if !status.authenticated {
        return Err("selected subscription is not authenticated".into());
    }
    profile.delegated_model_profile_id = None;
    // Subscription transports have no reviewed image estimator. No images in this fixture.
    profile.supports_vision = false;
    Ok(profile)
}

struct TemporaryEventStore {
    repository: Store,
}
#[async_trait]
impl EventStoreV4 for TemporaryEventStore {
    async fn append(&self, event: &AgentEventV4) -> Result<(), String> {
        self.repository
            .append_agent_event_v4_with_conversation(event)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
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
            .map_err(|e| e.to_string())
    }
}

#[tokio::test]
#[ignore = "explicit disposable Codex profile and Windows keyring subscription required"]
async fn live_codex_subscription_reads_nonce() {
    run_live_subscription_acceptance(ModelProviderKind::OpenAiCodex)
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "requires verified official Claude policy; production generation is currently gated"]
async fn live_claude_subscription_reads_nonce() {
    run_live_subscription_acceptance(ModelProviderKind::ClaudeCode)
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "explicit disposable Go Responses profile and Windows keyring key required"]
async fn live_go_responses_reads_nonce() {
    run_live_subscription_acceptance(ModelProviderKind::OpenAiResponses)
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "requires verified official Windows Claude policy; production generation is currently gated"]
async fn live_claude_subscription_windows_stop() {
    let directory = tempfile::tempdir().unwrap();
    let profile = selected_profile(ModelProviderKind::ClaudeCode, &directory)
        .await
        .unwrap();
    let manager =
        crate::subscription_models::SubscriptionLoginManager::system(Default::default()).unwrap();
    let client =
        crate::commands::model_client_for_profile_with_services(&profile, manager.model_services())
            .unwrap();
    let request=ProviderRequest { system:"Return a final envelope only; do not execute tools.".into(),messages:vec![omicsops_agent::ModelMessage { role:"user".into(),content:"Write a very long explanation of read-only scientific evidence, at least 10000 words. Do not use tools.".into() }],tools:vec![],require_strict_json_fallback:false,replay:vec![] };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut signal = Some(tx);
    let task = tokio::spawn(async move {
        client
            .stream_with_provider_v4(request, move |event| {
                if matches!(&event,ProviderStreamEvent::ContentProgress{bytes} if *bytes>0)
                    || matches!(&event,ProviderStreamEvent::TextDelta{text} if !text.is_empty())
                {
                    if let Some(tx) = signal.take() {
                        let _ = tx.send(());
                    }
                }
            })
            .await
    });
    let started = tokio::time::timeout(Duration::from_secs(30), rx).await;
    if !matches!(started, Ok(Ok(()))) || task.is_finished() {
        task.abort();
        let _ = task.await;
        panic!(
            "Claude did not reach an active verified generation; cancellation was not demonstrated"
        );
    }
    task.abort();
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("model cancellation did not complete");
    assert!(result.is_err_and(|error| error.is_cancelled()));
    // Job Object descendant termination is separately asserted by real Windows
    // managed_child fixtures; this live check asserts cancellation of the active request.
}

#[tokio::test]
async fn subscription_acceptance_preflight_rejects_unsafe_or_missing_configuration() {
    use omicsops_adapters::credentials::MemoryCredentialVault;
    let id = Uuid::new_v4();
    assert!(acceptance_profile_id(None, Some("1")).is_err());
    assert!(acceptance_profile_id(Some(&id.to_string()), None).is_err());
    assert_eq!(
        acceptance_profile_id(Some(&id.to_string()), Some("1")).unwrap(),
        id
    );
    let owned = tempfile::tempdir().unwrap();
    let foreign = tempfile::tempdir().unwrap();
    let mut profile = ModelProfile {
        id,
        label: "Disposable acceptance".into(),
        provider: ModelProviderKind::OpenAiResponses,
        base_url: "https://opencode.ai/zen/go/v1".into(),
        model: "grok-4.7".into(),
        credential_reference: Some("model/test-only".into()),
        supports_tools: true,
        supports_vision: false,
        context_window_tokens: None,
        catalog_capabilities: None,
        reasoning_effort: None,
        fast_mode: None,
        delegated_model_profile_id: None,
        cli_executable: None,
        subscription_account_ref: None,
    };
    let vault = MemoryCredentialVault::default();
    assert!(
        validate_acceptance_profile(
            &profile,
            ModelProviderKind::OpenAiResponses,
            &owned,
            foreign.path(),
            true,
            true
        )
        .is_err()
    );
    assert!(
        validate_acceptance_profile(
            &profile,
            ModelProviderKind::OpenAiCodex,
            &owned,
            owned.path(),
            true,
            true
        )
        .is_err()
    );
    assert!(
        validate_acceptance_profile(
            &profile,
            ModelProviderKind::OpenAiResponses,
            &owned,
            owned.path(),
            false,
            true
        )
        .is_err()
    );
    std::fs::write(owned.path().join("existing.txt"), "pre-existing project").unwrap();
    assert!(
        validate_acceptance_profile(
            &profile,
            ModelProviderKind::OpenAiResponses,
            &owned,
            owned.path(),
            true,
            true
        )
        .is_err()
    );
    profile.provider = ModelProviderKind::ClaudeCode;
    profile.base_url = "claude-code://local".into();
    profile.credential_reference = None;
    let fresh = tempfile::tempdir().unwrap();
    assert!(
        validate_acceptance_profile(
            &profile,
            ModelProviderKind::ClaudeCode,
            &fresh,
            fresh.path(),
            false,
            false
        )
        .is_err()
    );
    // Only a disposable source fixture is opened; no SystemCredentialVault or generation.
    let original = tempfile::tempdir().unwrap();
    let database = original.path().join("source.db");
    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true);
    let mut source = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE model_profiles (id TEXT PRIMARY KEY, value_json TEXT NOT NULL)")
        .execute(&mut source)
        .await
        .unwrap();
    sqlx::query("INSERT INTO model_profiles VALUES (?1,?2)")
        .bind(id.to_string())
        .bind(serde_json::to_string(&profile).unwrap())
        .execute(&mut source)
        .await
        .unwrap();
    source.close().await.unwrap();
    let before = std::fs::read(&database).unwrap();
    let loaded = read_selected_profile(&database, id).await.unwrap();
    assert_eq!(loaded, profile);
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert!(vault.get("model/test-only").unwrap().is_none());
}

async fn run_live_subscription_acceptance(expected: ModelProviderKind) -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|_| "could not create disposable project")?;
    let profile = selected_profile(expected, &directory).await?;
    let nonce = Uuid::new_v4().simple().to_string();
    let filename = format!("subscription-live-{}.txt", Uuid::new_v4().simple());
    std::fs::write(
        directory.path().join(&filename),
        format!("Subscription Agent acceptance nonce: {nonce}\n"),
    )
    .map_err(|error| error.to_string())?;

    let repository = Store::open_in_memory()
        .await
        .map_err(|error| error.to_string())?;
    let now = chrono::Utc::now();
    let project = Project::new(
        Uuid::new_v4(),
        "Subscription live acceptance",
        directory.path().to_string_lossy(),
        ProjectTemplate::Blank,
        now,
    );
    let conversation = Conversation::new(
        Uuid::new_v4(),
        project.id,
        "Subscription live acceptance",
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
        skills_root: directory.path().join("skills"),
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
        auto_review: false,
        memory_enabled: false,
        fast_mode: None,
    };
    let service_tier = RunServiceTierV4 { fast_mode: None };
    let run_id = Uuid::new_v4();
    let objective = format!(
        "This is a bounded live acceptance task. Use project.read exactly once to read `{filename}`. The file contains a random validation nonce that is not present in this request. Do not guess it and do not call any other task tool. After reading the file, call agent.complete with the exact nonce in answer_markdown and cite the successful project.read event as evidence for the frozen completion criterion."
    );
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
        BTreeSet::from(["project.read".into()]),
        DirectRunSnapshotV4 {
            model_configuration_hash: profile.execution_configuration_hash(),
            conversation_preferences: preferences,
            service_tier,
            reviewer_model: None,
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
        None,
    )
    .await?;
    let core = AgentCoreV4 {
        model: model.as_ref(),
        tools: tools.registry.as_ref(),
        events: &events,
        science: None,
    };
    let mut limits = AgentLimitsV4::ordinary(4);
    limits.max_tool_calls = 3;
    limits.max_model_retries = 0;
    let execution = core
        .execute_with_limits(&spec, limits, &AtomicBool::new(false))
        .await;

    execution.map_err(|error| error.to_string())?;

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
    if !recorded
        .iter()
        .any(|event| matches!(event.event, AgentEventKindV4::RunCompleted))
    {
        return Err("the production Agent loop did not record RunCompleted".into());
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

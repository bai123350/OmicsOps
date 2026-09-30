use super::*;
use futures_util::stream;
use omicsops_adapters::{
    AdapterResult,
    claude_code::{ClaudeAuthKind, ClaudePreflight, ClaudeProcessRunner},
    codex_auth::{
        CodexCredentialBundle, CodexCredentialCoordinator, CodexDeviceAuth, SystemCodexAuthClock,
    },
    credentials::MemoryCredentialVault,
    model_client::ModelClientServices,
    responses::{ResponsesHttpStream, ResponsesTransport},
};
use omicsops_core::workspace::ModelProviderKind;
use omicsops_process::managed_child::{BackgroundLaunchSpec, ManagedBackgroundChild};
use std::{
    sync::{Mutex as StdMutex, atomic::AtomicUsize},
    time::Duration,
};

#[derive(Default)]
struct Events(StdMutex<Vec<AgentEventV4>>);
#[async_trait]
impl EventStoreV4 for Events {
    async fn append(&self, event: &AgentEventV4) -> Result<(), String> {
        self.0.lock().unwrap().push(event.clone());
        Ok(())
    }
    async fn load(&self, run_id: Uuid) -> Result<Vec<AgentEventV4>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.run_id == run_id)
            .cloned()
            .collect())
    }
    async fn archive_context(
        &self,
        _: Uuid,
        transcript: &str,
        checkpoint: &ContextCheckpointV4,
    ) -> Result<ContextArchiveV4, String> {
        Ok(ContextArchiveV4 {
            archive_id: Uuid::new_v4(),
            through_sequence: checkpoint.through_sequence,
            size_bytes: transcript.len() as u64,
            sha256: hex::encode(sha2::Sha256::digest(transcript)),
        })
    }
}
fn proposal(index: usize) -> (&'static str, Value) {
    if index == 0 {
        ("project.read", json!({"path":"nonce.txt"}))
    } else {
        (
            "runtime.execute",
            json!({"language":"python","environment":"system","code":"import os\nos.remove('nonce.txt')"}),
        )
    }
}
#[derive(Default)]
struct Transport {
    requests: StdMutex<Vec<Value>>,
}
#[async_trait]
impl ResponsesTransport for Transport {
    async fn post(
        &self,
        _: url::Url,
        _: reqwest::header::HeaderMap,
        body: Value,
        _: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        let output = if index < 2 {
            let (id, args) = proposal(index);
            let wire = omicsops_adapters::responses::build_responses_request(
                omicsops_adapters::responses::ResponsesEndpointKind::OpenCodeGo,
                &url::Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
                "fixture",
                &ProviderRequest {
                    system: String::new(),
                    messages: vec![],
                    tools: vec![ProviderToolSpec {
                        id: id.into(),
                        description: String::new(),
                        input_schema: json!({}),
                    }],
                    require_strict_json_fallback: false,
                    replay: vec![],
                },
                None,
                None,
            )
            .unwrap();
            let name = wire.body["tools"][0]["name"].clone();
            assert!(
                body["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["name"] == name)
            );
            json!([{"id":format!("item_{index}"),"type":"function_call","call_id":format!("call_{index}"),"name":name,"arguments":args.to_string()}])
        } else {
            json!([{"id":format!("message_{index}"),"type":"message","role":"assistant","content":[{"type":"output_text","text":"Host denied deletion; file retained."}]}])
        };
        requests.push(body);
        Ok(ResponsesHttpStream {status:200,body:Box::pin(stream::iter([Ok(format!("data: {}\n\n",json!({"type":"response.completed","response":{"id":format!("resp_{index}"),"status":"completed","output":output}})).into_bytes())]))})
    }
}
#[derive(Default)]
struct ClaudeRunner(AtomicUsize);
#[async_trait]
impl ClaudeProcessRunner for ClaudeRunner {
    async fn inspect(&self, _: &Path) -> AdapterResult<ClaudePreflight> {
        Ok(ClaudePreflight {
            executable_fingerprint: "isolated-helper".into(),
            version: "2.1.281".into(),
            auth_kind: ClaudeAuthKind::Subscription,
            account_fingerprint: Some("fixture-account".into()),
            restrictions_verified: true,
        })
    }
    async fn spawn(
        &self,
        mut launch: BackgroundLaunchSpec,
    ) -> AdapterResult<ManagedBackgroundChild> {
        let index = self.0.fetch_add(1, Ordering::SeqCst);
        let envelope = if index < 2 {
            let (id, args) = proposal(index);
            json!({"kind":"tool_calls","calls":[{"call_id":format!("call_{index}"),"tool_id":id,"arguments":args}]})
        } else {
            json!({"kind":"final","text":"Host denied deletion; file retained."})
        };
        let assistant = json!({"type":"assistant","message":{"model":"claude-sonnet-5","content":[{"type":"text","text":"envelope"}]}});
        let result = json!({"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":envelope.to_string()});
        let script = format!(
            "$null=[Console]::In.ReadToEnd(); [Console]::Out.WriteLine('{}'); [Console]::Out.WriteLine('{}');",
            assistant.to_string().replace('\'', "''"),
            result.to_string().replace('\'', "''")
        );
        launch.program = PathBuf::from(std::env::var_os("SYSTEMROOT").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        launch.args = [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]
        .map(std::ffi::OsString::from)
        .to_vec();
        ManagedBackgroundChild::spawn(launch).map_err(omicsops_adapters::AdapterError::Io)
    }
}
fn profile(kind: ModelProviderKind) -> ModelProfile {
    let id = Uuid::new_v4();
    serde_json::from_value(json!({"id":id,"label":"fixture","provider":kind,"base_url":match kind {ModelProviderKind::OpenAiCodex=>"https://chatgpt.com/backend-api",ModelProviderKind::OpenAiResponses=>"https://opencode.ai/zen/go/v1",_=>"claude-code://local"},"model":if kind==ModelProviderKind::ClaudeCode {"sonnet"} else {"grok-4.7"},"credential_reference":if kind==ModelProviderKind::ClaudeCode {None} else {Some(format!("model/{id}"))},"supports_tools":true,"supports_vision":false,"context_window_tokens":1_000_000,"cli_executable":if kind==ModelProviderKind::ClaudeCode {Some("C:\\fixture\\claude.exe")} else {None},"subscription_account_ref":(kind==ModelProviderKind::OpenAiCodex).then(Uuid::new_v4)})).unwrap()
}
fn services(profile: &ModelProfile) -> Arc<ModelClientServices> {
    let vault = Arc::new(MemoryCredentialVault::default());
    if let Some(reference) = &profile.credential_reference {
        let credential = if profile.provider == ModelProviderKind::OpenAiCodex {
            CodexCredentialBundle::from_vault_json(&json!({"access_token":"fixture","refresh_token":"fixture","account_id":"fixture","account_ref":profile.subscription_account_ref,"expires_at_ms":i64::MAX}).to_string()).unwrap().to_vault_json().unwrap()
        } else {
            "fixture".into()
        };
        vault.set(reference, &credential).unwrap();
    }
    let clock = Arc::new(SystemCodexAuthClock);
    Arc::new(ModelClientServices {
        vault: vault.clone(),
        codex: Arc::new(CodexCredentialCoordinator::new(
            vault,
            Arc::new(CodexDeviceAuth::new(clock.clone()).unwrap()),
            clock,
        )),
        claude: Arc::new(ClaudeRunner::default()),
    })
}
fn model(profile: &ModelProfile, root: &Path, transport: Arc<Transport>) -> DesktopModelPortV4 {
    DesktopModelPortV4 {
        client: crate::commands::model_client_for_profile_with_services(profile, services(profile))
            .unwrap()
            .with_responses_transport(transport),
        prompt: PromptLayersV4::default(),
        usage_metadata: usage_metadata_for_profile(profile),
        resources: None,
        project_root: root.to_owned(),
        supports_vision: false,
        input_images: vec![],
        reviewer: None,
        delegated: None,
    }
}
#[tokio::test]
async fn subscription_v4_model_roles_preserve_selected_profile_and_frozen_identity() {
    let repository = Store::open_in_memory().await.unwrap();
    for kind in [
        ModelProviderKind::OpenAiCodex,
        ModelProviderKind::OpenAiResponses,
        ModelProviderKind::ClaudeCode,
    ] {
        let mut profile = profile(kind);
        let hash = profile.execution_configuration_hash();
        repository.save_model_profile(&profile).await.unwrap();
        let binding = omicsops_protocol::DelegatedModelBindingV4 {
            profile_id: profile.id,
            configuration_hash: hash.clone(),
        };
        let loaded = load_frozen_main_profile(&repository, profile.id, Some(&hash))
            .await
            .unwrap();
        validate_delegated_profile(&loaded, &binding).unwrap();
        for backend in [
            omicsops_protocol::ReviewerBackendChoiceV4::FollowSession,
            omicsops_protocol::ReviewerBackendChoiceV4::HttpProfile {
                profile_id: profile.id,
            },
        ] {
            let settings = omicsops_protocol::ReviewerSettingsV4 {
                backend,
                ..Default::default()
            };
            let reviewer = crate::session_reviews::resolve_reviewer_profile(
                &repository,
                &settings,
                profile.id,
            )
            .await
            .unwrap();
            assert_eq!(reviewer.id, profile.id);
        }
        profile.label = "edited label".into();
        repository.save_model_profile(&profile).await.unwrap();
        assert!(
            load_frozen_main_profile(&repository, profile.id, Some(&hash))
                .await
                .is_ok()
        );
        match kind {
            ModelProviderKind::OpenAiCodex => {
                profile.subscription_account_ref = Some(Uuid::new_v4())
            }
            ModelProviderKind::ClaudeCode => {
                profile.cli_executable = Some("C:\\different\\claude.exe".into())
            }
            _ => profile.model = "grok-4.7-sibling".into(),
        }
        repository.save_model_profile(&profile).await.unwrap();
        assert!(
            load_frozen_main_profile(&repository, profile.id, Some(&hash))
                .await
                .is_err()
        );
        assert!(validate_delegated_profile(&profile, &binding).is_err());
    }
}
struct Host {
    root: PathBuf,
    reads: AtomicUsize,
    deletes: AtomicUsize,
}

struct WaitingBody(Arc<AtomicBool>);
impl futures_util::Stream for WaitingBody {
    type Item = AdapterResult<Vec<u8>>;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Pending
    }
}
impl Drop for WaitingBody {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
struct WaitingTransport {
    started: tokio::sync::Notify,
    closed: Arc<AtomicBool>,
}
#[async_trait]
impl ResponsesTransport for WaitingTransport {
    async fn post(
        &self,
        _: url::Url,
        _: reqwest::header::HeaderMap,
        _: Value,
        _: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        self.started.notify_one();
        Ok(ResponsesHttpStream {
            status: 200,
            body: Box::pin(WaitingBody(self.closed.clone())),
        })
    }
}
#[tokio::test]
async fn subscription_v4_stop_closes_http_without_claiming_remote_job_cancelled() {
    use omicsops_core::workspace::{Conversation, ProjectTemplate};
    use omicsops_protocol::{ExecutionPlanV4, RunModeV4, RuntimeJobStateV4};
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "stop fixture",
        "synthetic",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "stop fixture", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "fixture".into(),
        steps: vec!["compute".into()],
        completion_criteria: vec!["evidence".into()],
        requested_capabilities: Default::default(),
    };
    let spec = RunSpecV4::freeze(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        Uuid::new_v4(),
        plan.clone(),
        &plan.canonical_hash().unwrap(),
        Utc::now(),
    )
    .unwrap();
    store
        .save_agent_run_v4(
            spec.run_id,
            project.id,
            conversation.id,
            "running",
            &json!({"spec":spec}),
        )
        .await
        .unwrap();
    let first = AgentEventV4::first(
        spec.run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    let call = ToolCallV4 {
        call_id: "remote-compute".into(),
        tool_id: "runtime.execute".into(),
        arguments: json!({"language":"python","environment":"system","code":"print('fixture')","background":true}),
    };
    let requested = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::ToolRequested { call: call.clone() },
    );
    store.append_agent_event_v4(&requested).await.unwrap();
    store
        .append_agent_event_v4(&AgentEventV4::next(
            &requested,
            Utc::now(),
            AgentEventKindV4::ToolDispatchStarted {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                effect: ToolEffectV4::Runtime,
                idempotency_key: call.call_id.clone(),
            },
        ))
        .await
        .unwrap();
    let key = ExecutionContextKeyV4 {
        project_id: project.id,
        run_id: spec.run_id,
        backend_id: "ssh:fixture".into(),
        language: KernelLanguageV4::Python,
        environment: "system".into(),
    };
    let (job, _) = store
        .reserve_runtime_job_with_remote_root_v4(
            &key,
            "remote-compute",
            &call.canonical_hash().unwrap(),
            Some("/fixture/jobs"),
            Some("fixture-host-key"),
        )
        .await
        .unwrap();
    let job = store
        .advance_runtime_job_v4(&job, RuntimeJobStateV4::Running, Some(Uuid::new_v4()), None)
        .await
        .unwrap();
    for kind in [
        ModelProviderKind::OpenAiCodex,
        ModelProviderKind::OpenAiResponses,
    ] {
        let profile = profile(kind);
        let transport = Arc::new(WaitingTransport {
            started: tokio::sync::Notify::new(),
            closed: Arc::new(AtomicBool::new(false)),
        });
        let client =
            crate::commands::model_client_for_profile_with_services(&profile, services(&profile))
                .unwrap()
                .with_responses_transport(transport.clone());
        let task = tokio::spawn(async move {
            client
                .stream_with_provider_once(
                    ProviderRequest {
                        system: "host".into(),
                        messages: vec![],
                        tools: vec![],
                        require_strict_json_fallback: false,
                        replay: vec![],
                    },
                    |_| {},
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(3), transport.started.notified())
            .await
            .unwrap();
        store
            .request_run_stop_v4(&omicsops_dto::StopRunRequestV4 {
                request_id: Uuid::new_v4(),
                project_id: project.id,
                conversation_id: conversation.id,
                run_id: spec.run_id,
            })
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(transport.closed.load(Ordering::SeqCst));
        assert_eq!(
            store
                .runtime_job_v4(&key, &call.call_id)
                .await
                .unwrap()
                .unwrap(),
            job
        );
    }
}
#[async_trait]
impl ToolExecutorV4 for Host {
    fn local_deletion_approval_reason(&self, call: &ToolCallV4) -> Option<String> {
        crate::local_deletion_approval::local_deletion_approval_reason(
            call,
            ComputeBackendKindV4::Local,
        )
    }
    async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
        if call.tool_id == "runtime.execute" {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            panic!("denied deletion reached host executor");
        }
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutcomeV4 {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            succeeded: true,
            model_content: std::fs::read_to_string(self.root.join("nonce.txt")).unwrap(),
            data: json!({}),
            provenance: vec!["host-fixture-read".into()],
        })
    }
}
#[tokio::test]
async fn subscription_v4_local_delete_requires_host_approval_and_resumes_without_reexecuting_read()
{
    for kind in [
        ModelProviderKind::OpenAiCodex,
        ModelProviderKind::OpenAiResponses,
        ModelProviderKind::ClaudeCode,
    ] {
        if kind == ModelProviderKind::ClaudeCode && !cfg!(windows) {
            continue;
        }
        let directory = tempfile::tempdir().unwrap();
        let nonce = Uuid::new_v4().to_string();
        std::fs::write(directory.path().join("nonce.txt"), &nonce).unwrap();
        let profile = profile(kind);
        let model = model(&profile, directory.path(), Arc::new(Transport::default()));
        let host = Arc::new(Host {
            root: directory.path().to_owned(),
            reads: AtomicUsize::new(0),
            deletes: AtomicUsize::new(0),
        });
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), host.clone()).unwrap();
        let request = StartDirectV4Request {
            project_id: Uuid::new_v4(),
            conversation_id: Uuid::new_v4(),
            model_profile_id: profile.id,
            objective: "Read nonce.txt then request its deletion.".into(),
            compute_selection: ComputeSelectionV4 {
                schema_version: 4,
                backend_id: "local".into(),
                backend_kind: ComputeBackendKindV4::Local,
                autonomy_mode: AutonomyModeV4::Supervised,
                approval_policy: ApprovalPolicyV4::RiskBased,
                environment: "system".into(),
                network_policy: NetworkPolicyV4::HostInherited,
                container_image: None,
            },
            references: vec![],
            attachments: vec![],
        };
        let (_, spec) = prepare_direct_run_v4(
            &request,
            Uuid::new_v4(),
            "[]",
            BTreeSet::from(["project.read".into(), "runtime.execute".into()]),
            DirectRunSnapshotV4 {
                model_configuration_hash: profile.execution_configuration_hash(),
                conversation_preferences: Default::default(),
                service_tier: omicsops_protocol::RunServiceTierV4 { fast_mode: None },
                reviewer_model: None,
                delegated_model: None,
                reference_context: String::new(),
                input_images: vec![],
            },
            Utc::now(),
        )
        .unwrap();
        let events = Events::default();
        events
            .append(&AgentEventV4::first(
                spec.run_id,
                spec.project_id,
                spec.conversation_id,
                Utc::now(),
                AgentEventKindV4::RunCreated {
                    mode: omicsops_protocol::RunModeV4::Execute,
                },
            ))
            .await
            .unwrap();
        let core = AgentCoreV4 {
            model: &model,
            tools: &registry,
            events: &events,
            science: None,
        };
        assert!(
            matches!(
                core.execute(&spec, 3).await,
                Err(AgentCoreErrorV4::WaitingForApproval)
            ),
            "{kind:?}"
        );
        assert_eq!(host.reads.load(Ordering::SeqCst), 1);
        assert_eq!(host.deletes.load(Ordering::SeqCst), 0);
        let recorded = events.load(spec.run_id).await.unwrap();
        let approval = recorded
            .iter()
            .find_map(|event| match &event.event {
                AgentEventKindV4::ToolApprovalRequested { request } => Some(request),
                _ => None,
            })
            .unwrap();
        events
            .append(&AgentEventV4::next(
                recorded.last().unwrap(),
                Utc::now(),
                AgentEventKindV4::ToolApprovalDecided {
                    approval_id: approval.approval_id.clone(),
                    call_hash: approval.call_hash.clone(),
                    decision: ToolApprovalDecisionV4::Denied,
                },
            ))
            .await
            .unwrap();
        let _ = core.execute(&spec, 1).await;
        assert_eq!(host.reads.load(Ordering::SeqCst), 1);
        assert_eq!(host.deletes.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read_to_string(directory.path().join("nonce.txt")).unwrap(),
            nonce
        );
        let recorded = events.load(spec.run_id).await.unwrap();
        assert_eq!(recorded.iter().filter(|event|matches!(&event.event,AgentEventKindV4::ToolFinished {outcome} if outcome.tool_id=="project.read")).count(),1);
        assert!(recorded.iter().any(|event|matches!(&event.event,AgentEventKindV4::ToolFinished {outcome} if outcome.data["error_kind"]=="approval_denied")));
        assert!(
            registry
                .validate(
                    omicsops_protocol::RunModeV4::Plan,
                    &ToolCallV4 {
                        call_id: "plan-delete".into(),
                        tool_id: "runtime.execute".into(),
                        arguments: proposal(1).1
                    }
                )
                .is_err()
        );
    }
}

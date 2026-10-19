//! Deterministic integration coverage for scientific runtime failure receipts.
use super::*;
use omicsops_agent_core::verify_completion_v4;
use omicsops_core::workspace::{Conversation, ProjectTemplate};
use omicsops_protocol::{
    CompletionCriterionEvidenceV4, CompletionEvidenceRefV4, CompletionProposalV4, RunModeV4,
    RuntimeJobStateV4,
};
use std::sync::Mutex as StdMutex;

struct TestEvents(Store);

#[async_trait]
impl EventStoreV4 for TestEvents {
    async fn append(&self, event: &AgentEventV4) -> Result<(), String> {
        self.0
            .append_agent_event_v4(event)
            .await
            .map_err(|error| error.to_string())
    }

    async fn load(&self, run_id: Uuid) -> Result<Vec<AgentEventV4>, String> {
        self.0
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
        self.0
            .archive_agent_context_v4(run_id, transcript, checkpoint)
            .await
            .map_err(|error| error.to_string())
    }
}

struct ScriptedCalls(StdMutex<Vec<ToolCallV4>>);

#[async_trait]
impl ModelPortV4 for ScriptedCalls {
    async fn stream(
        &self,
        _: ModelRequestV4,
        _: &mut (dyn FnMut(ModelStreamEventV4) + Send),
    ) -> Result<ModelTurnV4, ModelFailureV4> {
        let mut calls = self.0.lock().unwrap();
        Ok(ModelTurnV4 {
            public_text: String::new(),
            tool_calls: if calls.is_empty() {
                vec![]
            } else {
                vec![calls.remove(0)]
            },
            provider_continuation: None,
        })
    }
}

struct RuntimeReceipts {
    outcomes: BTreeMap<String, ToolOutcomeV4>,
    recovery: Option<(Store, ExecutionContextKeyV4)>,
    undispatched: bool,
    uncertain: bool,
}

#[async_trait]
impl ToolPortV4 for RuntimeReceipts {
    fn descriptors(&self, _: RunModeV4) -> Vec<ToolDescriptorV4> {
        vec![ToolDescriptorV4 {
            id: "runtime.execute".into(),
            description: "deterministic runtime receipt".into(),
            input_schema: json!({}),
            effect: ToolEffectV4::Runtime,
        }]
    }

    fn effect(&self, tool_id: &str) -> Option<ToolEffectV4> {
        (tool_id == "runtime.execute").then_some(ToolEffectV4::Runtime)
    }

    async fn prepare_call(&self, call: &ToolCallV4) -> Result<Option<ToolOutcomeV4>, String> {
        Ok(self.undispatched.then(|| ToolOutcomeV4 {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            succeeded: false,
            model_content: "interpreter unavailable; no computation dispatched".into(),
            data: json!({"operation_dispatched":false,"recoverable":true,"error_kind":"interpreter_unavailable"}),
            provenance: vec![],
        }))
    }

    async fn execute(&self, _: RunModeV4, call: ToolCallV4) -> Result<ToolOutcomeV4, String> {
        if self.uncertain {
            return Err("synthetic worker disconnected after dispatch".into());
        }
        Ok(self
            .outcomes
            .get(&call.call_id)
            .expect("preflight and durable recovery must never execute the original computation")
            .clone())
    }

    async fn recover_result(&self, call: &ToolCallV4) -> Result<Option<ToolOutcomeV4>, String> {
        let Some((repository, key)) = &self.recovery else {
            return Ok(None);
        };
        repository
            .recover_runtime_result_v4(key, call)
            .await
            .map_err(|error| error.to_string())?
            .map(|(job, result)| crate::runtime_jobs_v4::outcome(call, &job, &result))
            .transpose()
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    repository: Store,
    events: TestEvents,
    science: RepositoryScientificStateStoreV4,
    spec: RunSpecV4,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = Store::open(directory.path().join("science-contract.sqlite"))
            .await
            .unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "scientific failure contract",
            directory.path().display().to_string(),
            ProjectTemplate::Blank,
            Utc::now(),
        );
        repository.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "fixture", Utc::now());
        repository.save_conversation(&conversation).await.unwrap();
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "produce verified output after a repairable code error".into(),
            steps: vec!["execute and verify".into()],
            completion_criteria: vec!["verified output".into()],
            requested_capabilities: BTreeSet::from(["runtime.execute".into()]),
        };
        let run_id = Uuid::new_v4();
        let model_profile_id = Uuid::new_v4();
        let selection = ComputeSelectionV4 {
            schema_version: 4,
            backend_id: "local".into(),
            backend_kind: ComputeBackendKindV4::Local,
            autonomy_mode: AutonomyModeV4::Supervised,
            approval_policy: ApprovalPolicyV4::AutoApproveExceptLocalDeletion,
            environment: "system".into(),
            network_policy: NetworkPolicyV4::HostInherited,
            container_image: None,
        };
        let approval_hash = RunSpecV4::approval_hash_for(
            run_id,
            project.id,
            conversation.id,
            model_profile_id,
            &plan,
            &selection,
        )
        .unwrap();
        let spec = RunSpecV4::freeze_with_compute(
            run_id,
            project.id,
            conversation.id,
            model_profile_id,
            plan,
            selection,
            &approval_hash,
            Utc::now(),
        )
        .unwrap();
        repository
            .save_agent_run_v4_if_unlocked(
                spec.run_id,
                spec.project_id,
                spec.conversation_id,
                "running",
                &json!({"spec":spec}),
            )
            .await
            .unwrap();
        let events = TestEvents(repository.clone());
        events
            .append(&AgentEventV4::first(
                spec.run_id,
                spec.project_id,
                spec.conversation_id,
                Utc::now(),
                AgentEventKindV4::RunCreated {
                    mode: RunModeV4::Execute,
                },
            ))
            .await
            .unwrap();
        let science = RepositoryScientificStateStoreV4 {
            repository: repository.clone(),
            backend_id: "local".into(),
            mutation_lock: Arc::new(Mutex::new(())),
        };
        Self {
            _directory: directory,
            repository,
            events,
            science,
            spec,
        }
    }

    async fn append(&self, kind: AgentEventKindV4) {
        let events = self.events.load(self.spec.run_id).await.unwrap();
        self.events
            .append(&AgentEventV4::next(
                events.last().unwrap(),
                Utc::now(),
                kind,
            ))
            .await
            .unwrap();
    }

    async fn start(&self, call: &ToolCallV4) {
        self.append(AgentEventKindV4::ToolRequested { call: call.clone() })
            .await;
        self.science
            .before_tool(self.spec.project_id, self.spec.run_id, call)
            .await
            .unwrap();
        self.append(AgentEventKindV4::ToolDispatchStarted {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            effect: ToolEffectV4::Runtime,
            idempotency_key: call.call_id.clone(),
        })
        .await;
    }
}

fn analysis_call(call_id: &str) -> ToolCallV4 {
    ToolCallV4 {
        call_id: call_id.into(),
        tool_id: "runtime.execute".into(),
        arguments: json!({
            "language":"python","environment":"system","code":"print('fixture')",
            "analysis":{
                "analysis_type":"qc","input_dataset_ids":[],"sample_ids":[],
                "method":"dynamic-python","parameters":{},"software_requirements":[],
                "database_versions":{},"random_seed":7
            }
        }),
    }
}

fn runtime_result(succeeded: bool) -> RuntimeResultV4 {
    RuntimeResultV4 {
        request_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        process_identity: "deterministic-python:42".into(),
        stdout: if succeeded { "repaired" } else { "partial" }.into(),
        stderr: if succeeded {
            ""
        } else {
            "Traceback: invalid generated code"
        }
        .into(),
        stdout_capture: None,
        stderr_capture: None,
        succeeded,
        artifacts: vec![RuntimeArtifactV4 {
            relative_path: if succeeded {
                "results/repaired.tsv"
            } else {
                "results/partial.tsv"
            }
            .into(),
            size_bytes: 10,
            sha256: if succeeded {
                "repaired-host-hash"
            } else {
                "partial-host-hash"
            }
            .into(),
        }],
        software_versions: BTreeMap::new(),
    }
}

fn outcome(call: &ToolCallV4, result: &RuntimeResultV4) -> ToolOutcomeV4 {
    ToolOutcomeV4 {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        succeeded: result.succeeded,
        model_content: format!("{}{}", result.stdout, result.stderr),
        data: serde_json::to_value(result).unwrap(),
        provenance: vec![format!("kernel-session:{}", result.session_id)],
    }
}

fn proposal(sequence: u64) -> CompletionProposalV4 {
    CompletionProposalV4 {
        schema_version: 4,
        summary: "verified repaired output".into(),
        answer_markdown: "The repaired execution produced verified output.".into(),
        criteria: vec![CompletionCriterionEvidenceV4 {
            criterion: "verified output".into(),
            evidence: vec![CompletionEvidenceRefV4::Event { sequence }],
        }],
    }
}

#[tokio::test]
async fn completed_runtime_failure_is_failed_and_repair_can_pass_completion() {
    let fixture = Fixture::new().await;
    let failed_call = analysis_call("failed-cell");
    let repaired_call = analysis_call("repaired-cell");
    let failed_result = runtime_result(false);
    let repaired_result = runtime_result(true);
    let model = ScriptedCalls(StdMutex::new(vec![
        failed_call.clone(),
        repaired_call.clone(),
    ]));
    let tools = RuntimeReceipts {
        outcomes: BTreeMap::from([
            (
                failed_call.call_id.clone(),
                outcome(&failed_call, &failed_result),
            ),
            (
                repaired_call.call_id.clone(),
                outcome(&repaired_call, &repaired_result),
            ),
        ]),
        recovery: None,
        undispatched: false,
        uncertain: false,
    };
    let result = AgentCoreV4 {
        model: &model,
        tools: &tools,
        events: &fixture.events,
        science: Some(&fixture.science),
    }
    .execute(&fixture.spec, 2)
    .await;
    assert!(
        matches!(result, Err(AgentCoreErrorV4::MissingCompletion)),
        "{result:?}"
    );
    let state = fixture
        .science
        .snapshot(fixture.spec.project_id)
        .await
        .unwrap();
    let failed = state
        .analyses
        .values()
        .find(|a| a.source_call_id == "failed-cell")
        .unwrap();
    assert_eq!(failed.status, AnalysisStatusV4::Failed);
    assert!(failed.ended_at.is_some());
    assert_eq!(failed.runtime.session_id, Some(failed_result.session_id));
    assert_eq!(
        failed.runtime.process_identity.as_deref(),
        Some("deterministic-python:42")
    );
    let repaired = state
        .analyses
        .values()
        .find(|a| a.source_call_id == "repaired-cell")
        .unwrap();
    assert_eq!(repaired.status, AnalysisStatusV4::Succeeded);
    assert!(
        state
            .artifacts
            .values()
            .any(|a| a.producer_analysis_id == failed.id && !a.valid)
    );
    assert!(
        state
            .artifacts
            .values()
            .any(|a| a.producer_analysis_id == repaired.id && a.valid)
    );
    let events = fixture.events.load(fixture.spec.run_id).await.unwrap();
    let failed_event = events.iter().find(|event| matches!(&event.event,
        AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == "failed-cell" && !outcome.succeeded)).unwrap();
    assert!(
        events.iter().any(|event| matches!(&event.event,
        AgentEventKindV4::ScientificStateChanged { changes, .. }
        if event.sequence > failed_event.sequence
            && changes.contains(&format!("analysis_finished:{}:Failed", failed.id)))),
        "confirmed failure must update the replayable scientific event chain"
    );
    let repaired_event = events.iter().find(|event| matches!(&event.event,
        AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == "repaired-cell" && outcome.succeeded)).unwrap();
    assert!(
        verify_completion_v4(
            &fixture.spec,
            &state,
            &events,
            &proposal(repaired_event.sequence)
        )
        .passed
    );
    assert!(
        !verify_completion_v4(
            &fixture.spec,
            &state,
            &events,
            &proposal(failed_event.sequence)
        )
        .passed,
        "failed execution must never become successful completion evidence"
    );
}

#[tokio::test]
async fn recovered_durable_failed_result_finishes_analysis_without_reexecution() {
    let fixture = Fixture::new().await;
    let call = analysis_call("durable-failure");
    fixture.start(&call).await;
    let key = ExecutionContextKeyV4 {
        project_id: fixture.spec.project_id,
        run_id: fixture.spec.run_id,
        backend_id: "local".into(),
        language: KernelLanguageV4::Python,
        environment: "system".into(),
    };
    let result = runtime_result(false);
    let (reserved, acquired) = fixture
        .repository
        .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
        .await
        .unwrap();
    assert!(acquired);
    let running = fixture
        .repository
        .advance_runtime_job_v4(
            &reserved,
            RuntimeJobStateV4::Running,
            Some(result.session_id),
            None,
        )
        .await
        .unwrap();
    fixture
        .repository
        .advance_runtime_job_v4(&running, RuntimeJobStateV4::Failed, None, Some(&result))
        .await
        .unwrap();
    let model = ScriptedCalls(StdMutex::new(vec![]));
    let tools = RuntimeReceipts {
        outcomes: BTreeMap::new(),
        recovery: Some((fixture.repository.clone(), key)),
        undispatched: false,
        uncertain: false,
    };
    let execution = AgentCoreV4 {
        model: &model,
        tools: &tools,
        events: &fixture.events,
        science: Some(&fixture.science),
    }
    .execute(&fixture.spec, 1)
    .await;
    assert!(
        matches!(execution, Err(AgentCoreErrorV4::MissingCompletion)),
        "{execution:?}"
    );
    let state = fixture
        .science
        .snapshot(fixture.spec.project_id)
        .await
        .unwrap();
    let analysis = state.analyses.values().next().unwrap();
    assert_eq!(analysis.status, AnalysisStatusV4::Failed);
    assert_eq!(analysis.runtime.session_id, Some(result.session_id));
    let events = fixture.events.load(fixture.spec.run_id).await.unwrap();
    assert!(events.iter().any(|event| matches!(&event.event,
        AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == call.call_id && !outcome.succeeded)));
    assert!(
        events.iter().any(|event| matches!(&event.event,
        AgentEventKindV4::ScientificStateChanged { changes, .. }
        if changes.contains(&format!("analysis_finished:{}:Failed", analysis.id)))),
        "recovered failure must update the replayable scientific event chain"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, AgentEventKindV4::ToolDispatchUncertain { .. }))
    );
}

#[tokio::test]
async fn interpreter_preflight_does_not_create_or_finish_an_analysis() {
    let fixture = Fixture::new().await;
    let model = ScriptedCalls(StdMutex::new(vec![analysis_call(
        "unavailable-interpreter",
    )]));
    let tools = RuntimeReceipts {
        outcomes: BTreeMap::new(),
        recovery: None,
        undispatched: true,
        uncertain: false,
    };
    let result = AgentCoreV4 {
        model: &model,
        tools: &tools,
        events: &fixture.events,
        science: Some(&fixture.science),
    }
    .execute(&fixture.spec, 1)
    .await;
    assert!(
        matches!(result, Err(AgentCoreErrorV4::MissingCompletion)),
        "{result:?}"
    );
    let state = fixture
        .science
        .snapshot(fixture.spec.project_id)
        .await
        .unwrap();
    assert!(state.analyses.is_empty());
    assert!(state.provenance.is_empty());
    let events = fixture.events.load(fixture.spec.run_id).await.unwrap();
    assert!(events.iter().any(|event| matches!(&event.event,
        AgentEventKindV4::ToolFinished { outcome } if outcome.data["operation_dispatched"] == false)));
    assert!(!events.iter().any(|event| matches!(
        event.event,
        AgentEventKindV4::ToolDispatchStarted { .. }
            | AgentEventKindV4::ToolDispatchUncertain { .. }
    )));
}

#[tokio::test]
async fn unconfirmed_dispatch_keeps_running_analysis_and_uncertain_event() {
    let fixture = Fixture::new().await;
    let model = ScriptedCalls(StdMutex::new(vec![analysis_call("disconnected-worker")]));
    let tools = RuntimeReceipts {
        outcomes: BTreeMap::new(),
        recovery: None,
        undispatched: false,
        uncertain: true,
    };
    let result = AgentCoreV4 {
        model: &model,
        tools: &tools,
        events: &fixture.events,
        science: Some(&fixture.science),
    }
    .execute(&fixture.spec, 1)
    .await;
    assert!(
        matches!(result, Err(AgentCoreErrorV4::UncertainSideEffect(_))),
        "{result:?}"
    );
    let state = fixture
        .science
        .snapshot(fixture.spec.project_id)
        .await
        .unwrap();
    let analysis = state.analyses.values().next().unwrap();
    assert_eq!(analysis.status, AnalysisStatusV4::Running);
    assert!(analysis.ended_at.is_none());
    assert!(state.provenance.is_empty());
    let events = fixture.events.load(fixture.spec.run_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, AgentEventKindV4::ToolDispatchUncertain { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, AgentEventKindV4::ToolFinished { .. }))
    );
}

#[tokio::test]
async fn nonterminal_background_and_undispatched_outcomes_do_not_finish_analysis() {
    for data in [
        json!({"job":{"job_id":Uuid::new_v4(),"state":"running"},"status":"submitted","submission_observed":true,"resubmitted":false}),
        json!({"operation_dispatched":false,"error_kind":"interpreter_unavailable","recoverable":true}),
    ] {
        let fixture = Fixture::new().await;
        let call = analysis_call("nonterminal-call");
        fixture
            .science
            .before_tool(fixture.spec.project_id, fixture.spec.run_id, &call)
            .await
            .unwrap();
        let receipt = ToolOutcomeV4 {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            succeeded: data["status"] == "submitted",
            model_content: "nonterminal receipt".into(),
            data,
            provenance: vec![],
        };
        // Rejection is acceptable; neither payload confirms runtime completion.
        let _ = fixture
            .science
            .after_tool(
                fixture.spec.project_id,
                fixture.spec.run_id,
                &call,
                &receipt,
            )
            .await;
        let state = fixture
            .science
            .snapshot(fixture.spec.project_id)
            .await
            .unwrap();
        let analysis = state.analyses.values().next().unwrap();
        assert_eq!(analysis.status, AnalysisStatusV4::Running);
        assert!(analysis.ended_at.is_none());
        assert!(state.artifacts.is_empty());
        assert!(state.provenance.is_empty());
    }
}

#[tokio::test]
async fn snapshot_reconciles_historical_confirmed_failed_receipt_once() {
    let fixture = Fixture::new().await;
    let call = analysis_call("historical-failure");
    fixture.start(&call).await;
    let result = runtime_result(false);
    fixture
        .append(AgentEventKindV4::ToolFinished {
            outcome: outcome(&call, &result),
        })
        .await;
    let repaired = fixture
        .science
        .snapshot(fixture.spec.project_id)
        .await
        .unwrap();
    let analysis = repaired.analyses.values().next().unwrap();
    assert_eq!(analysis.status, AnalysisStatusV4::Failed);
    assert_eq!(analysis.runtime.session_id, Some(result.session_id));
    assert!(analysis.ended_at.is_some());
    assert_eq!(repaired.artifacts.len(), 1);
    assert!(repaired.artifacts.values().all(|artifact| !artifact.valid));
    assert_eq!(repaired.provenance.len(), 1);
    let again = fixture
        .science
        .snapshot(fixture.spec.project_id)
        .await
        .unwrap();
    assert_eq!(
        again, repaired,
        "reconciliation must be idempotent and must not create duplicate provenance"
    );
}

#[tokio::test]
async fn snapshot_does_not_reconcile_unconfirmed_or_wrong_scope_failures() {
    for case in [
        "preflight",
        "partial",
        "uncertain",
        "wrong_call",
        "wrong_run",
        "wrong_project",
        "wrong_backend",
        "successful_result",
        "wrong_tool",
    ] {
        let fixture = Fixture::new().await;
        let call = analysis_call("historical-unconfirmed");
        fixture.start(&call).await;
        let mut receipt = outcome(&call, &runtime_result(false));
        match case {
            "preflight" => {
                receipt.data["operation_dispatched"] = json!(false);
            }
            "partial" => {
                receipt.data = json!({"succeeded":false});
            }
            "successful_result" => {
                receipt.data = serde_json::to_value(runtime_result(true)).unwrap();
            }
            "wrong_tool" => {
                receipt.tool_id = "project.read".into();
            }
            "uncertain" => {
                fixture
                    .append(AgentEventKindV4::ToolDispatchUncertain {
                        call_id: call.call_id.clone(),
                        tool_id: call.tool_id.clone(),
                    })
                    .await;
            }
            "wrong_call" => {
                receipt.call_id = "another-call".into();
            }
            "wrong_run" | "wrong_project" | "wrong_backend" => {
                let mut state = fixture
                    .repository
                    .scientific_state_v4(fixture.spec.project_id)
                    .await
                    .unwrap()
                    .unwrap();
                let analysis = state.analyses.values_mut().next().unwrap();
                if case == "wrong_run" {
                    analysis.run_id = Uuid::new_v4();
                } else if case == "wrong_project" {
                    analysis.project_id = Uuid::new_v4();
                } else {
                    analysis.runtime.backend_id = "ssh:other".into();
                }
                fixture
                    .repository
                    .save_scientific_state_v4(&state)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        if case != "uncertain" {
            fixture
                .append(AgentEventKindV4::ToolFinished { outcome: receipt })
                .await;
        }
        let state = fixture
            .science
            .snapshot(fixture.spec.project_id)
            .await
            .unwrap();
        let analysis = state.analyses.values().next().unwrap();
        assert_eq!(analysis.status, AnalysisStatusV4::Running, "{case}");
        assert!(analysis.ended_at.is_none(), "{case}");
        assert!(state.provenance.is_empty(), "{case}");
        assert!(state.artifacts.is_empty(), "{case}");
    }
}

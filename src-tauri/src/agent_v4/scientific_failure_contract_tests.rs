//! Deterministic integration coverage for scientific runtime failure receipts.
use super::*;
use omicsops_agent_core::verify_completion_v4;
use omicsops_core::workspace::{Conversation, ProjectTemplate};
use omicsops_protocol::{
    CompletionCriterionEvidenceV4, CompletionEvidenceRefV4, CompletionProposalV4, ContextArchiveV4,
    ContextCheckpointV4, RunModeV4, RuntimeJobStateV4,
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

    async fn commit_context_compaction(
        &self,
        spec: &RunSpecV4,
        transcript: &str,
        events: &[AgentEventV4; 2],
    ) -> Result<(), ContextCompactionCommitErrorV4> {
        self.0
            .commit_automatic_context_compaction_v4(spec, transcript, events)
            .await
            .map_err(context_compaction_commit_error_v4)
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
        Self::with_criterion("verified output").await
    }

    async fn with_criterion(criterion: &str) -> Self {
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
            objective: criterion.into(),
            steps: vec!["execute and verify".into()],
            completion_criteria: vec![criterion.into()],
            requested_capabilities: BTreeSet::from([
                "runtime.execute".into(),
                "runtime.environment.inspect".into(),
            ]),
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
                &json!({"status":"running", "spec":spec}),
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
        let first = events.load(spec.run_id).await.unwrap().pop().unwrap();
        events
            .append(&AgentEventV4::next(
                &first,
                Utc::now(),
                AgentEventKindV4::RunSpecFrozen {
                    approval_hash: spec.approval_hash.clone().unwrap(),
                    spec_hash: spec.spec_hash.clone().unwrap(),
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

    async fn compaction_candidate(&self) -> (String, [AgentEventV4; 2]) {
        let source = self.events.load(self.spec.run_id).await.unwrap();
        let transcript = serde_json::to_string(&source).unwrap();
        let head = source.last().unwrap();
        let archived = AgentEventV4::next(
            head,
            Utc::now(),
            AgentEventKindV4::ContextArchived {
                archive: ContextArchiveV4 {
                    archive_id: Uuid::new_v4(),
                    through_sequence: head.sequence,
                    size_bytes: transcript.len() as u64,
                    sha256: hex::encode(sha2::Sha256::digest(transcript.as_bytes())),
                },
            },
        );
        let checkpointed = AgentEventV4::next(
            &archived,
            Utc::now(),
            AgentEventKindV4::ContextCheckpointed {
                checkpoint: ContextCheckpointV4 {
                    schema_version: 4,
                    through_sequence: head.sequence,
                    completion_criteria: self.spec.plan.completion_criteria.clone(),
                    unresolved_errors: vec![],
                    recent_steps: vec![],
                    scientific_state: serde_json::to_value(
                        self.science.load(self.spec.project_id).await.unwrap(),
                    )
                    .unwrap(),
                    task_shape: None,
                    phase: None,
                    task_revision: None,
                    tasks: vec![],
                    cycle_id: None,
                },
            },
        );
        (transcript, [archived, checkpointed])
    }
}

#[tokio::test]
async fn automatic_compaction_host_publishes_exact_events_only_after_commit() {
    let fixture = Fixture::new().await;
    let before = fixture.events.load(fixture.spec.run_id).await.unwrap();
    let (transcript, events) = fixture.compaction_candidate().await;
    let mut published = vec![];
    commit_context_compaction_and_publish_v4(
        &fixture.repository,
        &fixture.spec,
        &transcript,
        &events,
        |event| {
            // Read through an independent connection at publication time, so a
            // publish moved before commit cannot pass on uncommitted rows.
            let path = fixture._directory.path().join("science-contract.sqlite");
            let durable_count = std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move {
                        use sqlx::Connection;
                        let mut connection = sqlx::SqliteConnection::connect_with(
                            &sqlx::sqlite::SqliteConnectOptions::new()
                                .filename(path)
                                .read_only(true),
                        )
                        .await
                        .unwrap();
                        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM agent_events_v4")
                            .fetch_one(&mut connection)
                            .await
                            .unwrap()
                    })
            })
            .join()
            .unwrap();
            assert_eq!(durable_count as usize, before.len() + 2);
            published.push(event.clone());
        },
    )
    .await
    .unwrap();
    assert_eq!(published, events);
    let durable = fixture.events.load(fixture.spec.run_id).await.unwrap();
    assert_eq!(&durable[..before.len()], &before);
    assert_eq!(&durable[before.len()..], &events);
}

#[tokio::test]
async fn automatic_compaction_host_rejection_never_publishes_or_archives() {
    let fixture = Fixture::new().await;
    let (transcript, events) = fixture.compaction_candidate().await;
    fixture
        .append(AgentEventKindV4::ModelText {
            text: "source advanced before commit".into(),
        })
        .await;
    let before = fixture.events.load(fixture.spec.run_id).await.unwrap();
    let mut published = vec![];
    let error = commit_context_compaction_and_publish_v4(
        &fixture.repository,
        &fixture.spec,
        &transcript,
        &events,
        |event| published.push(event.clone()),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ContextCompactionCommitErrorV4::Rejected(_)));
    assert!(published.is_empty());
    assert_eq!(
        fixture.events.load(fixture.spec.run_id).await.unwrap(),
        before
    );
    let AgentEventKindV4::ContextArchived { archive } = &events[0].event else {
        panic!("missing archive event")
    };
    assert!(
        fixture
            .repository
            .agent_context_archive_v4(archive.archive_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn automatic_compaction_store_errors_distinguish_rejection_and_unavailability() {
    assert!(matches!(
        context_compaction_commit_error_v4(omicsops_store::StoreError::InvalidInput(
            "source changed".into()
        )),
        ContextCompactionCommitErrorV4::Rejected(_)
    ));
    assert!(matches!(
        context_compaction_commit_error_v4(omicsops_store::StoreError::Database(
            sqlx::Error::PoolClosed
        )),
        ContextCompactionCommitErrorV4::Unavailable(_)
    ));
    assert!(matches!(
        context_compaction_commit_error_v4(omicsops_store::StoreError::Io(std::io::Error::other(
            "storage unavailable"
        ))),
        ContextCompactionCommitErrorV4::Unavailable(_)
    ));
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

async fn initialization_receipt(fixture: &Fixture, call: &ToolCallV4) -> ToolOutcomeV4 {
    let key = ExecutionContextKeyV4 {
        project_id: fixture.spec.project_id,
        run_id: fixture.spec.run_id,
        backend_id: "local".into(),
        language: KernelLanguageV4::Python,
        environment: "system".into(),
    };
    let (reserved, acquired) = fixture
        .repository
        .reserve_runtime_job_v4(&key, &call.call_id, &call.canonical_hash().unwrap())
        .await
        .unwrap();
    assert!(acquired);
    fixture
        .repository
        .fail_runtime_initialization_v4(
            &reserved,
            omicsops_protocol::RuntimeInitializationFailureV4 {
                code: omicsops_protocol::RuntimeInitializationFailureCodeV4::SshDriverUploadFailed,
            },
        )
        .await
        .unwrap();
    crate::runtime_jobs_v4::recover_initialization_failure(&fixture.repository, &key, call)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn scientific_calls_in_different_runs_create_distinct_analyses() {
    let fixture = Fixture::new().await;
    let project_id = fixture.spec.project_id;
    let first_run = fixture.spec.run_id;
    let second_run = Uuid::new_v4();
    let call = analysis_call("ollama-0");
    fixture
        .science
        .before_tool(project_id, first_run, &call)
        .await
        .unwrap();
    fixture
        .science
        .after_tool(
            project_id,
            first_run,
            &call,
            &outcome(&call, &runtime_result(true)),
        )
        .await
        .unwrap();
    let original = fixture
        .repository
        .scientific_state_v4(project_id)
        .await
        .unwrap()
        .unwrap();

    assert!(
        fixture
            .science
            .before_tool(project_id, second_run, &call)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        fixture
            .science
            .before_tool(project_id, second_run, &call)
            .await
            .unwrap()
            .is_none()
    );
    fixture
        .science
        .after_tool(
            project_id,
            second_run,
            &call,
            &outcome(&call, &runtime_result(false)),
        )
        .await
        .unwrap();
    let state = fixture
        .repository
        .scientific_state_v4(project_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.analyses.len(), 2);
    for (id, analysis) in &original.analyses {
        assert_eq!(state.analyses[id], *analysis);
    }
    let second = state
        .analyses
        .values()
        .find(|analysis| analysis.run_id == second_run)
        .unwrap();
    assert_eq!(second.status, AnalysisStatusV4::Failed);
    let manifest = state
        .provenance
        .values()
        .find(|manifest| manifest.run_id == second_run)
        .unwrap();
    assert_eq!(manifest.analysis_id, second.id);
    assert!(
        manifest
            .output_artifact_ids
            .iter()
            .all(|id| state.artifacts[id].producer_analysis_id == second.id
                && !state.artifacts[id].valid)
    );
    assert!(
        fixture
            .science
            .after_tool(
                project_id,
                second_run,
                &call,
                &outcome(&call, &runtime_result(false))
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .repository
            .scientific_state_v4(project_id)
            .await
            .unwrap()
            .unwrap(),
        state
    );
}

#[tokio::test]
async fn scientific_completion_cannot_finish_another_runs_pending_analysis() {
    let fixture = Fixture::new().await;
    let project_id = fixture.spec.project_id;
    let call = analysis_call("shared-call");
    fixture
        .science
        .before_tool(project_id, fixture.spec.run_id, &call)
        .await
        .unwrap();
    let original = fixture
        .repository
        .scientific_state_v4(project_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        fixture
            .science
            .after_tool(
                project_id,
                Uuid::new_v4(),
                &call,
                &outcome(&call, &runtime_result(true))
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .repository
            .scientific_state_v4(project_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
}

#[tokio::test]
async fn scientific_evidence_deduplication_is_scoped_to_the_run() {
    let fixture = Fixture::new().await;
    let project_id = fixture.spec.project_id;
    let call = ToolCallV4 {
        call_id: "ollama-0".into(),
        tool_id: "science.record_evidence".into(),
        arguments: json!({"claim":"synthetic source claim","sources":[{"kind":"literature","source_id":"fixture","citation":"synthetic fixture"}],"strength":"supporting","conflicts_with":[]}),
    };
    let result = ToolOutcomeV4 {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        succeeded: true,
        model_content: "fixture".into(),
        data: json!({}),
        provenance: vec![],
    };
    let first_run = fixture.spec.run_id;
    let second_run = Uuid::new_v4();
    assert!(
        fixture
            .science
            .after_tool(project_id, first_run, &call, &result)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        fixture
            .science
            .after_tool(project_id, first_run, &call, &result)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .science
            .after_tool(project_id, second_run, &call, &result)
            .await
            .unwrap()
            .is_some()
    );
    let state = fixture
        .repository
        .scientific_state_v4(project_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.evidence.len(), 2);
    let runs = state
        .evidence
        .values()
        .map(|record| {
            serde_json::to_value(record).unwrap()["run_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        runs,
        BTreeSet::from([first_run.to_string(), second_run.to_string()])
    );

    // Legacy evidence lacks a run identity and cannot suppress a new run's record.
    let mut legacy_json = serde_json::to_value(&state).unwrap();
    for record in legacy_json["evidence"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        record.as_object_mut().unwrap().remove("run_id");
    }
    let legacy: ScientificStateV4 = serde_json::from_value(legacy_json).unwrap();
    fixture
        .repository
        .save_scientific_state_v4(&legacy)
        .await
        .unwrap();
    assert!(
        fixture
            .science
            .after_tool(project_id, first_run, &call, &result)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        fixture
            .repository
            .scientific_state_v4(project_id)
            .await
            .unwrap()
            .unwrap()
            .evidence
            .len(),
        3
    );
}

#[tokio::test]
async fn initialization_failure_finishes_analysis_without_fabricated_execution() {
    let fixture = Fixture::new().await;
    let call = analysis_call("failed-initialization");
    fixture.start(&call).await;
    let receipt = initialization_receipt(&fixture, &call).await;
    let update = fixture
        .science
        .after_tool(
            fixture.spec.project_id,
            fixture.spec.run_id,
            &call,
            &receipt,
        )
        .await
        .expect("a durable startup failure must not be parsed as an executed RuntimeResult")
        .unwrap();
    let state = fixture.science.load(fixture.spec.project_id).await.unwrap();
    let analysis = state.analyses.values().next().unwrap();
    assert_eq!(analysis.status, AnalysisStatusV4::Failed);
    assert!(analysis.ended_at.is_some());
    assert!(analysis.runtime.session_id.is_none());
    assert!(analysis.runtime.process_identity.is_none());
    assert!(state.artifacts.is_empty());
    assert!(state.provenance.is_empty());
    assert!(
        update
            .changes
            .iter()
            .all(|change| !change.starts_with("provenance_recorded:"))
    );
    assert!(
        fixture
            .science
            .after_tool(
                fixture.spec.project_id,
                fixture.spec.run_id,
                &call,
                &receipt
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture.science.load(fixture.spec.project_id).await.unwrap(),
        state
    );
}

#[tokio::test]
async fn scientific_snapshot_recovery_preserves_other_runs_with_the_same_call_id() {
    for initialization_failed in [false, true] {
        let fixture = Fixture::new().await;
        let project_id = fixture.spec.project_id;
        let call = analysis_call("shared-recovery-call");
        fixture.start(&call).await;
        let other_run = Uuid::new_v4();
        fixture
            .science
            .before_tool(project_id, other_run, &call)
            .await
            .unwrap();
        let original = fixture.science.load(project_id).await.unwrap();
        assert_eq!(original.analyses.len(), 2);
        let other = original
            .analyses
            .values()
            .find(|analysis| analysis.run_id == other_run)
            .unwrap()
            .clone();
        let receipt = if initialization_failed {
            initialization_receipt(&fixture, &call).await
        } else {
            outcome(&call, &runtime_result(false))
        };
        fixture
            .append(AgentEventKindV4::ToolFinished { outcome: receipt })
            .await;
        let recovered = fixture.science.snapshot(project_id).await.unwrap();
        assert_eq!(recovered.analyses[&other.id], other);
        let finished = recovered
            .analyses
            .values()
            .find(|analysis| analysis.run_id == fixture.spec.run_id)
            .unwrap();
        assert_eq!(finished.status, AnalysisStatusV4::Failed);
        assert!(finished.ended_at.is_some());
        assert!(
            recovered
                .provenance
                .values()
                .all(|manifest| manifest.run_id == fixture.spec.run_id
                    && manifest.analysis_id == finished.id)
        );
        if initialization_failed {
            assert!(recovered.provenance.is_empty());
            assert!(recovered.artifacts.is_empty());
        } else {
            assert_eq!(recovered.provenance.len(), 1);
            assert!(recovered.artifacts.values().all(|artifact| artifact.producer_analysis_id == finished.id && !artifact.valid));
        }
        assert_eq!(
            fixture.science.snapshot(project_id).await.unwrap(),
            recovered
        );
        let reopened = Store::open(fixture._directory.path().join("science-contract.sqlite"))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .scientific_state_v4(project_id)
                .await
                .unwrap()
                .unwrap(),
            recovered
        );
    }
}

#[tokio::test]
async fn snapshot_reconciles_initialization_failure_without_execution_provenance() {
    let fixture = Fixture::new().await;
    let call = analysis_call("initialization-crash-window");
    fixture.start(&call).await;
    let receipt = initialization_receipt(&fixture, &call).await;
    fixture
        .append(AgentEventKindV4::ToolFinished { outcome: receipt })
        .await;
    let state = fixture
        .science
        .snapshot(fixture.spec.project_id)
        .await
        .unwrap();
    let analysis = state.analyses.values().next().unwrap();
    assert_eq!(analysis.status, AnalysisStatusV4::Failed);
    assert!(analysis.ended_at.is_some());
    assert!(analysis.runtime.session_id.is_none());
    assert!(analysis.runtime.process_identity.is_none());
    assert!(state.artifacts.is_empty());
    assert!(state.provenance.is_empty());
    assert_eq!(
        fixture
            .science
            .snapshot(fixture.spec.project_id)
            .await
            .unwrap(),
        state
    );
}

#[tokio::test]
async fn untrusted_initialization_metadata_cannot_finish_an_analysis() {
    for case in [
        "missing_receipt",
        "changed_receipt",
        "wrong_call_hash",
        "wrong_run",
    ] {
        let fixture = Fixture::new().await;
        let mut call = analysis_call("untrusted-initialization");
        fixture.start(&call).await;
        let mut receipt = if case == "missing_receipt" {
            ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: false,
                model_content: "untrusted receipt".into(),
                data: json!({"error_kind":"runtime_initialization_failed", "code_dispatched":false}),
                provenance: vec![],
            }
        } else {
            initialization_receipt(&fixture, &call).await
        };
        if case == "changed_receipt" {
            receipt.data["setup_effects_possible"] = json!(false);
        }
        if case == "wrong_call_hash" {
            call.arguments["code"] = json!("different code");
        }
        let run_id = if case == "wrong_run" {
            Uuid::new_v4()
        } else {
            fixture.spec.run_id
        };
        let result = fixture
            .science
            .after_tool(fixture.spec.project_id, run_id, &call, &receipt)
            .await;
        if case == "wrong_run" {
            // No analysis belongs to this run, so the hook has nothing to update.
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err(), "{case}");
        }
        let state = fixture.science.load(fixture.spec.project_id).await.unwrap();
        assert_eq!(
            state.analyses.values().next().unwrap().status,
            AnalysisStatusV4::Running,
            "{case}"
        );
        assert!(state.provenance.is_empty(), "{case}");
    }
}

struct InspectionFailureModel {
    repository: Store,
    run_id: Uuid,
    runtime_call: ToolCallV4,
}

#[async_trait]
impl ModelPortV4 for InspectionFailureModel {
    async fn review(&self, request: ReviewerRequestV4) -> Result<ReviewerReportV4, ModelFailureV4> {
        assert!(request.deterministic_report.passed);
        assert!(
            request
                .proposal
                .answer_markdown
                .contains("code was not submitted")
        );
        Ok(ReviewerReportV4 {
            schema_version: 4,
            summary: "The answer distinguishes observed interpreters from unexecuted checks."
                .into(),
            findings: vec![],
        })
    }

    async fn stream(
        &self,
        request: ModelRequestV4,
        _: &mut (dyn FnMut(ModelStreamEventV4) + Send),
    ) -> Result<ModelTurnV4, ModelFailureV4> {
        let events = self.repository.agent_events_v4(self.run_id).await.unwrap();
        let inspected = events.iter().find_map(|event| match &event.event {
            AgentEventKindV4::ToolFinished { outcome }
                if outcome.call_id == "inspect-existing" && outcome.succeeded =>
            {
                Some(event.sequence)
            }
            _ => None,
        });
        let failed = events.iter().any(|event| {
            matches!(&event.event,
            AgentEventKindV4::ToolFinished { outcome }
            if outcome.call_id == self.runtime_call.call_id && !outcome.succeeded)
        });
        let call = match (inspected, failed) {
            (None, _) => ToolCallV4 {
                call_id: "inspect-existing".into(),
                tool_id: "runtime.environment.inspect".into(),
                arguments: json!({"language":"python", "environment":"system"}),
            },
            (Some(_), false) => self.runtime_call.clone(),
            (Some(sequence), true) => {
                assert!(request.context.contains("runtime_initialization_failed"));
                assert!(request.context.contains("code_dispatched"));
                ToolCallV4 {
                    call_id: "report-inspection".into(),
                    tool_id: "agent.complete".into(),
                    arguments: json!({
                        "schema_version":4,
                        "summary":"Interpreter inspection completed with an execution limitation.",
                        "answer_markdown":"Python was found by the read-only interpreter probe. This does not verify project dependencies. The optional execution check could not initialize its kernel; its code was not submitted. Setup files or idle processes may exist. No execution retry was attempted.",
                        "criteria":[{"criterion":"report interpreter discovery and execution limitations", "evidence":[{"kind":"event", "sequence":sequence}]}]
                    }),
                }
            }
        };
        Ok(ModelTurnV4 {
            public_text: String::new(),
            tool_calls: vec![call],
            provider_continuation: None,
        })
    }
}

struct InspectionFailureTools {
    repository: Store,
    key: ExecutionContextKeyV4,
    runtime: RuntimeManagerV4,
    executions: std::sync::atomic::AtomicUsize,
}

struct UnavailableKernel;

#[async_trait]
impl omicsops_runtime::KernelBackendV4 for UnavailableKernel {
    fn descriptor(&self) -> ComputeBackendDescriptorV4 {
        ComputeBackendDescriptorV4 {
            schema_version: 4,
            backend_id: "local".into(),
            kind: ComputeBackendKindV4::Local,
            isolation: IsolationStrengthV4::Process,
            available: true,
            supports_python: true,
            supports_r: false,
            supports_network_policy: false,
        }
    }

    async fn launch(
        &self,
        _: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn omicsops_runtime::KernelProcessV4>, String> {
        Err("ssh_kernel_driver_upload_failed: synthetic write failure".into())
    }
}

#[async_trait]
impl ToolPortV4 for InspectionFailureTools {
    fn descriptors(&self, _: RunModeV4) -> Vec<ToolDescriptorV4> {
        ["runtime.environment.inspect", "runtime.execute"]
            .into_iter()
            .map(|id| ToolDescriptorV4 {
                id: id.into(),
                description: "deterministic inspection contract".into(),
                input_schema: json!({}),
                effect: self.effect(id).unwrap(),
            })
            .collect()
    }

    fn effect(&self, tool_id: &str) -> Option<ToolEffectV4> {
        match tool_id {
            "runtime.environment.inspect" => Some(ToolEffectV4::ReadOnly),
            "runtime.execute" => Some(ToolEffectV4::Runtime),
            _ => None,
        }
    }

    async fn execute(&self, _: RunModeV4, call: ToolCallV4) -> Result<ToolOutcomeV4, String> {
        if call.tool_id == "runtime.environment.inspect" {
            return Ok(ToolOutcomeV4 {
                call_id: call.call_id,
                tool_id: call.tool_id,
                succeeded: true,
                model_content: "Python executable found; project packages are not verified.".into(),
                data: json!({"status":"available","language":"python","dependencies_verified":false}),
                provenance: vec!["host-interpreter-probe".into()],
            });
        }
        assert_eq!(call.call_id, "optional-execution-check");
        assert_eq!(
            self.executions
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            0,
            "the failed runtime call must not be retried"
        );
        match crate::runtime_jobs_v4::execute_reserved(
            &self.repository,
            &self.runtime,
            &self.key,
            &call,
            required(&call.arguments, "code")?.into(),
            vec![],
        )
        .await?
        {
            crate::runtime_jobs_v4::RuntimeExecutionV4::InitializationFailed(outcome) => {
                Ok(outcome)
            }
            _ => panic!("an unavailable kernel cannot execute a cell"),
        }
    }
}

#[tokio::test]
async fn core_reports_existing_inspection_after_known_initialization_failure_without_retry() {
    for has_analysis in [false, true] {
        let fixture =
            Fixture::with_criterion("report interpreter discovery and execution limitations").await;
        let mut runtime_call = analysis_call("optional-execution-check");
        if !has_analysis {
            runtime_call
                .arguments
                .as_object_mut()
                .unwrap()
                .remove("analysis");
        }
        let model = InspectionFailureModel {
            repository: fixture.repository.clone(),
            run_id: fixture.spec.run_id,
            runtime_call,
        };
        let tools = InspectionFailureTools {
            repository: fixture.repository.clone(),
            key: ExecutionContextKeyV4 {
                project_id: fixture.spec.project_id,
                run_id: fixture.spec.run_id,
                backend_id: "local".into(),
                language: KernelLanguageV4::Python,
                environment: "system".into(),
            },
            runtime: RuntimeManagerV4::new(Arc::new(UnavailableKernel)),
            executions: std::sync::atomic::AtomicUsize::new(0),
        };
        let result = AgentCoreV4 {
            model: &model,
            tools: &tools,
            events: &fixture.events,
            science: Some(&fixture.science),
        }
        .execute(&fixture.spec, 3)
        .await;
        assert!(result.is_ok(), "has_analysis={has_analysis}: {result:?}");
        assert_eq!(
            tools.executions.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let events = fixture.events.load(fixture.spec.run_id).await.unwrap();
        assert!(
            events
                .iter()
                .any(|event| matches!(event.event, AgentEventKindV4::RunCompleted))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.event, AgentEventKindV4::ToolDispatchUncertain { .. }))
        );
        let failed = events.iter().find(|event| matches!(&event.event, AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == "optional-execution-check")).unwrap();
        let state = fixture
            .science
            .snapshot(fixture.spec.project_id)
            .await
            .unwrap();
        assert!(state.provenance.is_empty());
        assert!(state.artifacts.is_empty());
        assert!(
            state
                .analyses
                .values()
                .all(|analysis| analysis.status == AnalysisStatusV4::Failed)
        );
        let invalid_claim = CompletionProposalV4 {
            schema_version: 4,
            summary: "invalid proof".into(),
            answer_markdown: "invalid proof".into(),
            criteria: vec![CompletionCriterionEvidenceV4 {
                criterion: fixture.spec.plan.completion_criteria[0].clone(),
                evidence: vec![CompletionEvidenceRefV4::Event {
                    sequence: failed.sequence,
                }],
            }],
        };
        assert!(
            !verify_completion_v4(&fixture.spec, &state, &events, &invalid_claim).passed,
            "a startup failure must not become successful completion evidence"
        );
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

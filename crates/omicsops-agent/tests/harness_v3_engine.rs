use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use omicsops_agent::{
    AgentError, AgentResult,
    harness_v3::{
        AgentRunSpecV3, AuthorityDecisionV3, CancellationTokenV3, CompletionCriterionV3,
        EngineResultV3, EventSinkV3, FindingSeverityV3, HarnessV3Engine, ModelProviderV2,
        ModelRequestV2, ModelStreamEventV2, NoopToolAuditSinkV3, RunStatusV3,
        StaticToolAuthorityV3, ToolCallRequestV3, ToolDefinitionV3, ToolOutcomeStatusV3,
        ToolOutcomeV3, ToolRouterV3, ToolRuntimeV3, builtin_tool_definitions_v3,
    },
};
use serde_json::json;
use uuid::Uuid;

#[derive(Default)]
struct MemorySink(Mutex<Vec<omicsops_agent::harness_v3::AgentRunEventV3>>);

impl EventSinkV3 for MemorySink {
    fn append(&self, event: &omicsops_agent::harness_v3::AgentRunEventV3) -> Result<(), String> {
        self.0.lock().unwrap().push(event.clone());
        Ok(())
    }
}

struct FailOnceRuntime(AtomicUsize);

#[async_trait]
impl ToolRuntimeV3 for FailOnceRuntime {
    async fn execute(
        &self,
        _definition: &ToolDefinitionV3,
        request: &ToolCallRequestV3,
        _cancellation: &CancellationTokenV3,
    ) -> ToolOutcomeV3 {
        let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
        ToolOutcomeV3 {
            call_id: request.call_id.clone(),
            status: if first {
                ToolOutcomeStatusV3::Failed
            } else {
                ToolOutcomeStatusV3::Succeeded
            },
            model_content: if first {
                "temporary read failure".into()
            } else {
                "matrix inspected".into()
            },
            structured_result: Some(json!({"ok":!first})),
            error: first.then(|| "temporary read failure".into()),
            truncated: false,
            provenance: vec!["fixture:repair".into()],
        }
    }
}

struct ScriptedProvider {
    id: Uuid,
    responses: Mutex<VecDeque<Vec<ModelStreamEventV2>>>,
    requests: Mutex<Vec<ModelRequestV2>>,
}

impl ScriptedProvider {
    fn new(responses: Vec<Vec<ModelStreamEventV2>>) -> Self {
        Self {
            id: Uuid::new_v4(),
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ModelProviderV2 for ScriptedProvider {
    fn profile_id(&self) -> Uuid {
        self.id
    }

    async fn stream_v2(&self, request: ModelRequestV2) -> AgentResult<Vec<ModelStreamEventV2>> {
        self.requests.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AgentError::Model("script exhausted".into()))
    }
}

#[derive(Default)]
struct FixtureRuntime(Mutex<Vec<String>>);

#[async_trait]
impl ToolRuntimeV3 for FixtureRuntime {
    async fn execute(
        &self,
        _definition: &ToolDefinitionV3,
        request: &ToolCallRequestV3,
        _cancellation: &CancellationTokenV3,
    ) -> ToolOutcomeV3 {
        self.0.lock().unwrap().push(request.tool_id.clone());
        let structured_result = if request.tool_id == "artifact.verify" {
            Some(json!({
                "path":"results/pbmc.h5ad",
                "size_bytes":42,
                "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }))
        } else {
            Some(json!({"ok":true}))
        };
        ToolOutcomeV3 {
            call_id: request.call_id.clone(),
            status: ToolOutcomeStatusV3::Succeeded,
            model_content: format!("{} succeeded", request.tool_id),
            structured_result,
            error: None,
            truncated: false,
            provenance: vec![format!("fixture:{}", request.tool_id)],
        }
    }
}

fn tool_call(call_id: &str, index: u32, tool_id: &str, arguments: &str) -> Vec<ModelStreamEventV2> {
    vec![
        ModelStreamEventV2::ToolCallStarted {
            call_id: call_id.into(),
            index,
            tool_id: tool_id.into(),
        },
        ModelStreamEventV2::ToolArgumentsDelta {
            call_id: call_id.into(),
            index,
            arguments: arguments.into(),
        },
        ModelStreamEventV2::ToolCallCompleted {
            call_id: call_id.into(),
            index,
        },
    ]
}

#[tokio::test]
async fn scripted_executor_runs_multiple_tools_completes_and_invokes_isolated_reviewer() {
    let mut tools = builtin_tool_definitions_v3();
    let spec = AgentRunSpecV3::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Analyze PBMC3k dynamically",
        vec![CompletionCriterionV3::new(
            "deliver",
            "deliver verified h5ad",
        )],
        tools.clone(),
        vec![],
    );
    let mut first = tool_call("list", 0, "remote.list", "{}");
    first.extend(tool_call(
        "read",
        1,
        "remote.read",
        "{\"path\":\"input/matrix.mtx\"}",
    ));
    first.extend(tool_call(
        "verify",
        2,
        "artifact.verify",
        "{\"path\":\"results/pbmc.h5ad\"}",
    ));
    first.push(ModelStreamEventV2::Completed);
    let mut complete = tool_call(
        "complete",
        0,
        "agent.complete",
        &json!({
            "criteria":[{"id":"deliver","evidence_sequences":[1]}],
            "artifacts":[{
                "path":"results/pbmc.h5ad",
                "size_bytes":42,
                "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "evidence_sequence":1
            }],
            "summary":"done"
        })
        .to_string(),
    );
    complete.push(ModelStreamEventV2::Completed);
    let executor = Arc::new(ScriptedProvider::new(vec![first, complete]));
    let reviewer = Arc::new(ScriptedProvider::new(vec![vec![
        ModelStreamEventV2::TextDelta {
            text: json!({
                "cycle":0,
                "findings":[{
                    "severity":"ok",
                    "summary":"artifact is verified",
                    "evidence":["event:1","results/pbmc.h5ad"]
                }]
            })
            .to_string(),
        },
        ModelStreamEventV2::Completed,
    ]]));
    let runtime = Arc::new(FixtureRuntime::default());
    let router = Arc::new(
        ToolRouterV3::new(
            std::mem::take(&mut tools),
            Arc::new(StaticToolAuthorityV3(AuthorityDecisionV3::Allowed)),
            runtime.clone(),
            Arc::new(NoopToolAuditSinkV3),
            4,
        )
        .unwrap(),
    );
    let sink = Arc::new(MemorySink::default());
    let result: EngineResultV3 = HarnessV3Engine::new(
        spec,
        executor.clone(),
        reviewer.clone(),
        router,
        sink.clone(),
    )
    .run(CancellationTokenV3::new())
    .await
    .unwrap();

    assert_eq!(result.status, RunStatusV3::Completed);
    assert_eq!(
        runtime.0.lock().unwrap().as_slice(),
        ["remote.list", "remote.read", "artifact.verify"]
    );
    assert_eq!(
        result.review_report.unwrap().findings[0].severity,
        FindingSeverityV3::Ok
    );
    assert_eq!(reviewer.requests.lock().unwrap().len(), 1);
    let reviewer_tools = &reviewer.requests.lock().unwrap()[0].tools;
    assert!(reviewer_tools.iter().all(|tool| matches!(
        tool.id.as_str(),
        "remote.list" | "remote.read" | "artifact.verify"
    )));
    assert!(sink.0.lock().unwrap().len() > 8);
}

#[tokio::test]
async fn reviewer_errors_reopen_executor_twice_then_need_attention() {
    let tools = builtin_tool_definitions_v3();
    let spec = AgentRunSpecV3::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Analyze",
        vec![CompletionCriterionV3::new("done", "done")],
        tools.clone(),
        vec![],
    );
    let complete_response = || {
        let mut response = tool_call(
            "complete",
            0,
            "agent.complete",
            &json!({
                "criteria":[{"id":"done","evidence_sequences":[1]}],
                "artifacts":[]
            })
            .to_string(),
        );
        response.push(ModelStreamEventV2::Completed);
        response
    };
    let executor = Arc::new(ScriptedProvider::new(vec![
        complete_response(),
        complete_response(),
        complete_response(),
    ]));
    let error_review = || {
        vec![
            ModelStreamEventV2::TextDelta {
                text: json!({
                    "cycle":0,
                    "findings":[{
                        "severity":"error",
                        "summary":"seed missing",
                        "evidence":["results/qc.json"]
                    }]
                })
                .to_string(),
            },
            ModelStreamEventV2::Completed,
        ]
    };
    let reviewer = Arc::new(ScriptedProvider::new(vec![
        error_review(),
        error_review(),
        error_review(),
    ]));
    let router = Arc::new(
        ToolRouterV3::new(
            tools,
            Arc::new(StaticToolAuthorityV3(AuthorityDecisionV3::Allowed)),
            Arc::new(FixtureRuntime::default()),
            Arc::new(NoopToolAuditSinkV3),
            4,
        )
        .unwrap(),
    );

    let result = HarnessV3Engine::new(
        spec,
        executor.clone(),
        reviewer,
        router,
        Arc::new(MemorySink::default()),
    )
    .run(CancellationTokenV3::new())
    .await
    .unwrap();

    assert_eq!(result.status, RunStatusV3::NeedsAttention);
    assert_eq!(executor.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn black_box_executor_repairs_a_failed_tool_before_completion() {
    let tools = builtin_tool_definitions_v3();
    let spec = AgentRunSpecV3::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Inspect and repair",
        vec![CompletionCriterionV3::new("inspected", "matrix inspected")],
        tools.clone(),
        vec![],
    );
    let executor = Arc::new(ScriptedProvider::new(vec![
        tool_call(
            "read-failed",
            0,
            "remote.read",
            "{\"path\":\"input/matrix.mtx\"}",
        ),
        tool_call(
            "read-repaired",
            0,
            "remote.read",
            "{\"path\":\"input/matrix.mtx\",\"max_bytes\":1024}",
        ),
        tool_call(
            "complete",
            0,
            "agent.complete",
            "{\"criteria\":[{\"id\":\"inspected\",\"evidence_sequences\":[8]}],\"artifacts\":[]}",
        ),
    ]));
    let reviewer = Arc::new(ScriptedProvider::new(vec![vec![ModelStreamEventV2::TextDelta {
        text: json!({"cycle":0,"findings":[{"severity":"ok","summary":"repair verified","evidence":["event:8"]}]}).to_string(),
    }]]));
    let runtime = Arc::new(FailOnceRuntime(AtomicUsize::new(0)));
    let router = Arc::new(
        ToolRouterV3::new(
            tools,
            Arc::new(StaticToolAuthorityV3(AuthorityDecisionV3::Allowed)),
            runtime.clone(),
            Arc::new(NoopToolAuditSinkV3),
            4,
        )
        .unwrap(),
    );

    let result = HarnessV3Engine::new(
        spec,
        executor,
        reviewer,
        router,
        Arc::new(MemorySink::default()),
    )
    .run(CancellationTokenV3::new())
    .await
    .unwrap();

    assert_eq!(result.status, RunStatusV3::Completed);
    assert!(result.ledger.unresolved_errors.is_empty());
    assert_eq!(runtime.0.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn malformed_tool_arguments_receive_exactly_one_strict_json_repair() {
    let tools = builtin_tool_definitions_v3();
    let spec = AgentRunSpecV3::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Repair JSON",
        vec![CompletionCriterionV3::new("done", "read completed")],
        tools.clone(),
        vec![],
    );
    let malformed = tool_call("broken", 0, "remote.read", "{\"path\":");
    let repaired = tool_call(
        "repaired",
        0,
        "remote.read",
        "{\"path\":\"input/matrix.mtx\"}",
    );
    let complete = tool_call(
        "complete",
        0,
        "agent.complete",
        "{\"criteria\":[{\"id\":\"done\",\"evidence_sequences\":[5]}],\"artifacts\":[]}",
    );
    let executor = Arc::new(ScriptedProvider::new(vec![malformed, repaired, complete]));
    let reviewer = Arc::new(ScriptedProvider::new(vec![vec![ModelStreamEventV2::TextDelta {
        text: json!({"cycle":0,"findings":[{"severity":"ok","summary":"JSON repair worked","evidence":["event:5"]}]}).to_string(),
    }]]));
    let router = Arc::new(
        ToolRouterV3::new(
            tools,
            Arc::new(StaticToolAuthorityV3(AuthorityDecisionV3::Allowed)),
            Arc::new(FixtureRuntime::default()),
            Arc::new(NoopToolAuditSinkV3),
            4,
        )
        .unwrap(),
    );

    let result = HarnessV3Engine::new(
        spec,
        executor.clone(),
        reviewer,
        router,
        Arc::new(MemorySink::default()),
    )
    .run(CancellationTokenV3::new())
    .await
    .unwrap();

    assert_eq!(result.status, RunStatusV3::Completed);
    assert_eq!(executor.requests.lock().unwrap().len(), 3);
    assert!(
        executor.requests.lock().unwrap()[1]
            .system
            .contains("STRICT_JSON_REPAIR")
    );
}

#[tokio::test]
async fn recovery_never_reexecutes_a_dispatched_side_effect_with_unknown_result() {
    let tools = builtin_tool_definitions_v3();
    let spec = AgentRunSpecV3::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Recover",
        vec![],
        tools.clone(),
        vec![],
    );
    let first = omicsops_agent::harness_v3::AgentRunEventV3::first(
        &spec,
        chrono::Utc::now(),
        omicsops_agent::harness_v3::AgentRunEventKindV3::RunStarted,
    )
    .unwrap();
    let dispatched = omicsops_agent::harness_v3::AgentRunEventV3::next(
        &first,
        chrono::Utc::now(),
        omicsops_agent::harness_v3::AgentRunEventKindV3::ToolCallDispatched {
            request: ToolCallRequestV3 {
                call_id: "write-unknown".into(),
                tool_id: "remote.write".into(),
                arguments: json!({"path":"results/a","content":"x"}),
                idempotency_key: "write-key".into(),
            },
            read_only: false,
        },
    )
    .unwrap();
    let runtime = Arc::new(FixtureRuntime::default());
    let router = Arc::new(
        ToolRouterV3::new(
            tools,
            Arc::new(StaticToolAuthorityV3(AuthorityDecisionV3::Allowed)),
            runtime.clone(),
            Arc::new(NoopToolAuditSinkV3),
            4,
        )
        .unwrap(),
    );
    let sink = Arc::new(MemorySink(Mutex::new(vec![
        first.clone(),
        dispatched.clone(),
    ])));
    let provider = Arc::new(ScriptedProvider::new(vec![]));
    let result = HarnessV3Engine::resume(
        spec,
        provider.clone(),
        provider,
        router,
        sink,
        vec![first, dispatched],
    )
    .unwrap()
    .run(CancellationTokenV3::new())
    .await
    .unwrap();

    assert_eq!(result.status, RunStatusV3::Recovering);
    assert_eq!(runtime.0.lock().unwrap().len(), 0);
}

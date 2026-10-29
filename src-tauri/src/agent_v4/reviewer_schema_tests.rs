//! Reviewer schema errors are recoverable reports, never executor tool retries.
use super::*;
use futures_util::{StreamExt, stream};
use omicsops_adapters::{
    AdapterResult,
    claude_code::{ClaudePreflight, ClaudeProcessRunner},
    codex_auth::{CodexCredentialCoordinator, CodexDeviceAuth, SystemCodexAuthClock},
    credentials::MemoryCredentialVault,
    model_client::ModelClientServices,
    responses::{ResponsesHttpStream, ResponsesTransport},
};
use omicsops_process::managed_child::{BackgroundLaunchSpec, ManagedBackgroundChild};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct NoProcess;
#[async_trait]
impl ClaudeProcessRunner for NoProcess {
    async fn inspect(&self, _: &Path) -> AdapterResult<ClaudePreflight> {
        panic!("review fixture must not inspect any executable");
    }
    async fn spawn(&self, _: BackgroundLaunchSpec) -> AdapterResult<ManagedBackgroundChild> {
        panic!("review fixture must not start any process");
    }
}

struct ReviewTransport {
    report: Value,
    requests: Mutex<Vec<Value>>,
}
#[async_trait]
impl ResponsesTransport for ReviewTransport {
    async fn post(
        &self,
        _: url::Url,
        _: reqwest::header::HeaderMap,
        body: Value,
        _: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        let name = body["tools"][0]["name"].clone();
        self.requests.lock().unwrap().push(body);
        let event = json!({"type":"response.completed","response":{
            "id":"review-response", "status":"completed",
            "output":[{"id":"review-item","type":"function_call","call_id":"review-call","name":name,"arguments":self.report.to_string()}]
        }});
        Ok(ResponsesHttpStream {
            status: 200,
            body: Box::pin(stream::iter([
                Ok(format!("data: {event}\n\n").into_bytes()),
            ])),
        })
    }
}

fn reviewer(transport: Arc<dyn ResponsesTransport>) -> DesktopModelPortV4 {
    let profile: ModelProfile = serde_json::from_value(json!({
        "id":Uuid::new_v4(), "label":"offline Go review", "provider":"open_ai_responses",
        "base_url":"https://opencode.ai/zen/go/v1", "model":"deepseek-v4.1-flash",
        "credential_reference":"fixture-review", "supports_tools":true,"supports_vision":false,
        "context_window_tokens":100000
    }))
    .unwrap();
    let vault = Arc::new(MemoryCredentialVault::default());
    vault.set("fixture-review", "not-a-real-key").unwrap();
    let clock = Arc::new(SystemCodexAuthClock);
    let services = Arc::new(ModelClientServices {
        vault: vault.clone(),
        codex: Arc::new(CodexCredentialCoordinator::new(
            vault,
            Arc::new(CodexDeviceAuth::new(clock.clone()).unwrap()),
            clock,
        )),
        claude: Arc::new(NoProcess),
    });
    let client = crate::commands::model_client_for_profile_with_services(&profile, services)
        .unwrap()
        .with_request_budget(RequestBudget {
            context_window_tokens: 100_000,
            reserved_output_tokens: 16_384,
            safety_margin_tokens: 1_024,
        });
    assert!(
        matches!(&client, ModelClient::Responses(_)),
        "fixture requires the injectable Responses transport"
    );
    DesktopModelPortV4 {
        client: client.with_responses_transport(transport),
        prompt: PromptLayersV4::default(),
        usage_metadata: usage_metadata_for_profile(&profile),
        resources: None,
        project_root: PathBuf::new(),
        supports_vision: false,
        input_images: vec![],
        reviewer: None,
        delegated: None,
    }
}

fn request() -> ReviewerRequestV4 {
    ReviewerRequestV4 {
        frozen_objective: "Report the verified interpreter availability".into(),
        completion_criteria: vec!["Use host observations".into()],
        proposal: omicsops_protocol::CompletionProposalV4 {
            schema_version: 4,
            summary: "The host checked interpreter availability".into(),
            answer_markdown: "Python was observed in the selected environment.".into(),
            criteria: vec![],
        },
        deterministic_report: omicsops_protocol::DeterministicVerificationV4 {
            schema_version: 4,
            passed: true,
            findings: vec![],
        },
        scientific_state: ScientificStateV4::new(Uuid::new_v4()),
        verified_evidence: vec![],
    }
}

#[tokio::test]
async fn desktop_reviewer_report_schema_failures_are_classified_without_defaulting_or_nested_retries()
 {
    for report in [
        json!({"summary":"Checked", "findings":[]}),
        json!({"schema_version":null, "summary":"Checked", "findings":[]}),
        json!({"schema_version":3, "summary":"Checked", "findings":[]}),
        json!({"schema_version":4, "summary":"", "findings":[]}),
        json!({"schema_version":4, "summary":"Checked", "findings":[{"severity":"PRIVATE_MODEL_SENTINEL","code":"x","message":"x","evidence":["event:7"]}]}),
        json!({"schema_version":4, "summary":"Checked", "findings":[{"severity":"warn","code":"x","message":"x","evidence":[]}]}),
    ] {
        let transport = Arc::new(ReviewTransport {
            report,
            requests: Mutex::new(vec![]),
        });
        let failure = reviewer(transport.clone())
            .review(request())
            .await
            .unwrap_err();
        assert_eq!(failure.class, ModelErrorClassV4::InvalidResponse);
        assert!(
            !failure.retryable,
            "the core owns the single shared regeneration budget"
        );
        assert!(
            failure.message.starts_with("reviewer_report_schema:"),
            "{}",
            failure.message
        );
        assert!(!failure.message.contains("PRIVATE_MODEL_SENTINEL"));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn desktop_reviewer_report_schema_and_prompt_reach_mock_transport_with_frozen_evidence() {
    let report = json!({"schema_version":4,"summary":"Checked the supplied host observations","findings":[]});
    let transport = Arc::new(ReviewTransport {
        report: report.clone(),
        requests: Mutex::new(vec![]),
    });
    let input = request();
    let result = reviewer(transport.clone())
        .review(input.clone())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), report);
    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let wire = &requests[0];
    assert_eq!(wire["tools"].as_array().unwrap().len(), 1);
    assert_eq!(
        wire["tools"][0]["parameters"]["required"],
        json!(["schema_version", "summary", "findings"])
    );
    assert_eq!(
        wire["tools"][0]["parameters"]["properties"]["schema_version"]["const"],
        4
    );
    assert!(
        wire["instructions"]
            .as_str()
            .unwrap()
            .contains("\"schema_version\":4")
    );
    assert_eq!(
        wire["input"][0]["content"],
        serde_json::to_string(&input).unwrap()
    );
}

#[tokio::test]
async fn desktop_reviewer_large_raw_evidence_uses_a_bounded_first_request() {
    let transport = Arc::new(ReviewTransport {
        report: json!({"schema_version":4,"summary":"Read-only review","findings":[]}),
        requests: Mutex::new(vec![]),
    });
    let mut input = request();
    for (sequence, bytes) in [(7, 600_000), (8, 530_000)] {
        let data = json!({"content":[{"type":"text","text":"x".repeat(bytes)}]});
        input
            .verified_evidence
            .push(omicsops_agent_core::ReviewerEvidenceV4 {
                reference: omicsops_protocol::CompletionEvidenceRefV4::Event { sequence },
                payload: json!({"kind":"tool_finished","outcome":{
                    "call_id":format!("source-{sequence}"),"tool_id":"use_mcp_tool",
                    "succeeded":true,"model_content":data.to_string(),"data":data,
                    "provenance":["synthetic-frozen-evidence"]
                }}),
            });
    }
    let id = Uuid::new_v4();
    input.scientific_state.evidence.insert(
        id,
        omicsops_science::EvidenceRecordV4 {
            schema_version: 4,
            id,
            project_id: input.scientific_state.project_id,
            run_id: None,
            source_call_id: "synthetic-science".into(),
            claim: "y".repeat(500_000),
            sources: vec![],
            strength: omicsops_science::EvidenceStrengthV4::Exploratory,
            conflicts_with: Default::default(),
            valid: true,
            invalid_reason: None,
            recorded_at: Utc::now(),
        },
    );
    let original = input.clone();
    let result = reviewer(transport.clone())
        .review(input.clone())
        .await
        .unwrap();
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.code == "review_evidence_partial")
    );
    let requests = transport.requests.lock().unwrap();
    assert!(
        !requests.is_empty(),
        "large frozen evidence must reach a bounded review request rather than fail provider admission"
    );
    assert!(serde_json::to_vec(&requests[0]).unwrap().len() < 65_536);
    assert_eq!(
        input, original,
        "projection must preserve the frozen raw evidence"
    );
}

#[derive(Clone, Copy)]
enum ReadScenario {
    ReadThenSubmit,
    Mixed,
    MultipleSubmit,
    Unknown,
    ForgedFinding,
    RepeatedRead,
    InvalidThenCorrect,
    LegacyHashThenCorrect,
    InlinePages,
    RepeatedInvalid,
    WideRepeatedBatch,
    WideDistinctBatch,
    ReadAfterDisabled,
}

struct ReadReviewTransport {
    scenario: ReadScenario,
    requests: Mutex<Vec<Value>>,
}

#[async_trait]
impl ResponsesTransport for ReadReviewTransport {
    async fn post(
        &self,
        _: url::Url,
        _: reqwest::header::HeaderMap,
        body: Value,
        _: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(body.clone());
        if body["tools"].as_array().unwrap().len() == 1 {
            if matches!(self.scenario, ReadScenario::ReadAfterDisabled) {
                let first: Value =
                    serde_json::from_str(requests[0]["input"][0]["content"].as_str().unwrap())
                        .unwrap();
                let source = first["_review_projection"]["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|source| source["citation"] == "event:7")
                    .unwrap();
                let event = json!({"type":"response.completed","response":{"id":"forbidden-last-read","status":"completed","output":[{
                    "id":"forbidden-read","type":"function_call","call_id":"forbidden-read","name":requests[0]["tools"][1]["name"],
                    "arguments":json!({"source_id":source["source_id"],"offset":0,"limit":1}).to_string()
                }]}});
                return Ok(ResponsesHttpStream {
                    status: 200,
                    body: Box::pin(stream::iter([
                        Ok(format!("data: {event}\n\n").into_bytes()),
                    ])),
                });
            }
            let event = json!({"type":"response.completed","response":{"id":format!("review-final-{index}"),"status":"completed","output":[{
                "id":"final-submit","type":"function_call","call_id":"submit-final","name":body["tools"][0]["name"],
                "arguments":json!({"schema_version":4,"summary":"Partial review from the supplied evidence","findings":[]}).to_string()
            }]}});
            return Ok(ResponsesHttpStream {
                status: 200,
                body: Box::pin(stream::iter([
                    Ok(format!("data: {event}\n\n").into_bytes()),
                ])),
            });
        }
        let first: Value =
            serde_json::from_str(requests[0]["input"][0]["content"].as_str().unwrap()).unwrap();
        let catalog = first["_review_projection"]["sources"]
            .as_array()
            .expect("model-visible source directory");
        let item = catalog
            .iter()
            .find(|item| item["citation"] == "event:7")
            .unwrap();
        let mut arguments = json!({
            "source_id":item["source_id"],
            "pointer":"/outcome/data/answer", "offset":0,"limit":8192
        });
        let advertised = body["tools"][1]["parameters"]["properties"]["source_id"]["enum"]
            .as_array()
            .unwrap();
        assert!(
            catalog
                .iter()
                .all(|item| advertised.contains(&item["source_id"]))
        );
        let report = json!({"schema_version":4,"summary":"Checked the supplied observation","findings":[{
            "severity":"ok","code":"observed_fact","message":"The supplied fact is present","evidence":["event:7"]
        }]});
        let call = |name: &Value, args: Value, id: &str| json!({"id":format!("item-{id}"),"type":"function_call","call_id":id,"name":name,"arguments":args.to_string()});
        let submit = call(&body["tools"][0]["name"], report.clone(), "submit");
        let read_name = &body["tools"][1]["name"];
        let output = match self.scenario {
            ReadScenario::ReadThenSubmit if index % 2 == 1 => vec![submit],
            ReadScenario::ReadThenSubmit
            | ReadScenario::RepeatedRead
            | ReadScenario::ReadAfterDisabled => {
                vec![call(read_name, arguments, "read")]
            }
            ReadScenario::Mixed => vec![call(read_name, arguments, "read"), submit],
            ReadScenario::MultipleSubmit => vec![
                submit,
                call(&body["tools"][0]["name"], report, "submit-again"),
            ],
            ReadScenario::Unknown => vec![call(&json!("ungranted_tool"), json!({}), "unknown")],
            ReadScenario::ForgedFinding => {
                let mut report = report;
                report["findings"][0]["evidence"] = json!(["event:99999"]);
                vec![call(&body["tools"][0]["name"], report, "submit")]
            }
            ReadScenario::InvalidThenCorrect if index == 0 => {
                arguments["source_id"] = json!("file:C:/x-private-outside");
                vec![call(read_name, arguments, "invalid-read")]
            }
            ReadScenario::LegacyHashThenCorrect if index == 0 => {
                arguments["source_sha256"] = json!("0".repeat(64));
                vec![call(read_name, arguments, "old-contract-read")]
            }
            ReadScenario::InvalidThenCorrect | ReadScenario::LegacyHashThenCorrect
                if index == 1 =>
            {
                vec![call(read_name, arguments, "corrected-read")]
            }
            ReadScenario::InvalidThenCorrect | ReadScenario::LegacyHashThenCorrect => vec![submit],
            ReadScenario::InlinePages if index < 2 => {
                let item = catalog
                    .iter()
                    .find(|item| item["citation"] == "event:8")
                    .unwrap();
                arguments["source_id"] = item["source_id"].clone();
                arguments["pointer"] = json!("/inline_fact");
                arguments["limit"] = json!(if index == 0 { 5 } else { 8192 });
                if index == 1 {
                    let text = body["input"][0]["content"].as_str().unwrap();
                    let frame: Value = serde_json::from_str(text).unwrap();
                    arguments["offset"] =
                        frame["_review_reads"].as_array().unwrap().last().unwrap()["next_offset"]
                            .clone();
                }
                assert!(arguments.get("source_sha256").is_none());
                assert!(arguments.get("snapshot_sha256").is_none());
                vec![call(read_name, arguments, "inline-page")]
            }
            ReadScenario::InlinePages => vec![submit],
            ReadScenario::RepeatedInvalid => {
                arguments["source_id"] = json!("event:unprovided");
                (0..4)
                    .map(|n| {
                        call(
                            read_name,
                            arguments.clone(),
                            &format!("invalid-{index}-{n}"),
                        )
                    })
                    .collect()
            }
            ReadScenario::WideRepeatedBatch if index < 3 => {
                arguments["pointer"] = json!("/outcome/data/bulk");
                (0..4)
                    .map(|n| call(read_name, arguments.clone(), &format!("wide-{index}-{n}")))
                    .collect()
            }
            ReadScenario::WideDistinctBatch if index == 0 => {
                arguments["pointer"] = json!("/outcome/data/bulk");
                (0..4)
                    .map(|n| {
                        let mut arguments = arguments.clone();
                        arguments["offset"] = json!(n);
                        call(read_name, arguments, &format!("distinct-{n}"))
                    })
                    .collect()
            }
            ReadScenario::WideRepeatedBatch | ReadScenario::WideDistinctBatch => vec![submit],
        };
        let event = json!({"type":"response.completed","response":{"id":format!("review-{index}"),"status":"completed","output":output}});
        Ok(ResponsesHttpStream {
            status: 200,
            body: Box::pin(stream::iter([
                Ok(format!("data: {event}\n\n").into_bytes()),
            ])),
        })
    }
}

fn read_review_request() -> ReviewerRequestV4 {
    let mut input = request();
    input.verified_evidence.push(omicsops_agent_core::ReviewerEvidenceV4 {
        reference:omicsops_protocol::CompletionEvidenceRefV4::Event { sequence:7 },
        payload:json!({"outcome":{"data":{"answer":"verified-fact-731","bulk":"x".repeat(600_000)}}}),
    });
    input
}

#[tokio::test]
async fn desktop_reviewer_reads_frozen_selected_evidence_and_retries_the_same_snapshot() {
    let transport = Arc::new(ReadReviewTransport {
        scenario: ReadScenario::ReadThenSubmit,
        requests: Mutex::new(vec![]),
    });
    let input = read_review_request();
    let original = input.clone();
    let model = reviewer(transport.clone());
    for _ in 0..2 {
        let result = model.review(input.clone()).await.unwrap();
        assert_eq!(result.findings[0].code, "observed_fact");
        assert!(
            result
                .findings
                .iter()
                .any(|f| f.code == "review_evidence_partial")
        );
    }
    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[0]["input"], requests[2]["input"],
        "regeneration must reuse unchanged frozen evidence"
    );
    assert!(
        requests[1]["input"][0]["content"]
            .as_str()
            .unwrap()
            .contains("verified-fact-731")
    );
    assert!(
        requests
            .iter()
            .all(|body| serde_json::to_vec(body).unwrap().len() < 65_536)
    );
    assert_eq!(input, original);
}

#[tokio::test]
async fn desktop_reviewer_rejects_mixed_multiple_unknown_or_forged_tools_and_findings() {
    for scenario in [
        ReadScenario::Mixed,
        ReadScenario::MultipleSubmit,
        ReadScenario::Unknown,
        ReadScenario::ForgedFinding,
    ] {
        let transport = Arc::new(ReadReviewTransport {
            scenario,
            requests: Mutex::new(vec![]),
        });
        let failure = reviewer(transport.clone())
            .review(read_review_request())
            .await
            .unwrap_err();
        assert!(!failure.retryable);
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn desktop_reviewer_read_loop_stops_at_its_fixed_turn_budget() {
    let transport = Arc::new(ReadReviewTransport {
        scenario: ReadScenario::RepeatedRead,
        requests: Mutex::new(vec![]),
    });
    let report = reviewer(transport.clone())
        .review(read_review_request())
        .await
        .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "review_evidence_partial")
    );
    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests.last().unwrap()["tools"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        requests.last().unwrap()["tools"][0]["parameters"]["properties"]["findings"]["maxItems"],
        7
    );
}

#[tokio::test]
async fn desktop_reviewer_invalid_read_is_corrected_in_the_same_frozen_snapshot() {
    for scenario in [
        ReadScenario::InvalidThenCorrect,
        ReadScenario::LegacyHashThenCorrect,
    ] {
        let transport = Arc::new(ReadReviewTransport {
            scenario,
            requests: Mutex::new(vec![]),
        });
        let result = reviewer(transport.clone())
            .review(read_review_request())
            .await
            .unwrap();
        assert!(result.findings.iter().any(|f| f.code == "observed_fact"));
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        let correction = requests[1]["input"][0]["content"].as_str().unwrap();
        assert!(correction.contains("reviewer_read_"));
        assert!(!correction.contains("x-private-outside"));
        assert!(
            requests[2]["input"][0]["content"]
                .as_str()
                .unwrap()
                .contains("verified-fact-731")
        );
    }
}

#[tokio::test]
async fn desktop_reviewer_reads_inline_source_pages_using_only_visible_source_id() {
    let transport = Arc::new(ReadReviewTransport {
        scenario: ReadScenario::InlinePages,
        requests: Mutex::new(vec![]),
    });
    let mut input = read_review_request();
    input.verified_evidence.push(omicsops_agent_core::ReviewerEvidenceV4 {
        reference:omicsops_protocol::CompletionEvidenceRefV4::Event { sequence:8 },
        payload:json!({"inline_fact":"inline 数据🧬 fact", "source_id":"untrusted-alias", "source_sha256":"untrusted-hash"}),
    });
    let result = reviewer(transport.clone()).review(input).await.unwrap();
    assert!(result.findings.iter().any(|f| f.code == "observed_fact"));
    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[2]["input"][0]["content"]
            .as_str()
            .unwrap()
            .contains("数据🧬")
    );
}

#[tokio::test]
async fn desktop_reviewer_invalid_reads_exhaust_the_attempt_budget() {
    let transport = Arc::new(ReadReviewTransport {
        scenario: ReadScenario::RepeatedInvalid,
        requests: Mutex::new(vec![]),
    });
    let report = reviewer(transport.clone())
        .review(read_review_request())
        .await
        .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "review_evidence_partial")
    );
    assert_eq!(transport.requests.lock().unwrap().len(), 5);
}

#[tokio::test]
async fn desktop_reviewer_long_contract_and_escaped_read_batches_stay_admissible() {
    for scenario in [
        ReadScenario::WideRepeatedBatch,
        ReadScenario::WideDistinctBatch,
    ] {
        let transport = Arc::new(ReadReviewTransport {
            scenario,
            requests: Mutex::new(vec![]),
        });
        let mut model = reviewer(transport.clone());
        model.client = model.client.with_request_budget(RequestBudget {
            context_window_tokens: 1_000_000,
            reserved_output_tokens: 16_384,
            safety_margin_tokens: 1_024,
        });
        let mut input = read_review_request();
        input.proposal.answer_markdown = "a".repeat(105_000);
        input.verified_evidence[0].payload["outcome"]["data"]["bulk"] =
            json!("\"\\数据🧬".repeat(50_000));
        let result = model.review(input).await.expect("bounded read batches must permit an honest partial review rather than overflow the next context");
        assert!(
            result
                .findings
                .iter()
                .any(|finding| finding.code == "review_evidence_partial")
        );
        let requests = transport.requests.lock().unwrap();
        assert!(requests.len() >= 2);
        for request in requests.iter() {
            let context = request["input"][0]["content"].as_str().unwrap();
            assert!(context.len() <= 128 * 1024);
            let context: Value = serde_json::from_str(context).unwrap();
            assert_eq!(
                context["proposal"]["answer_markdown"]
                    .as_str()
                    .unwrap()
                    .len(),
                105_000
            );
        }
    }
}

#[tokio::test]
async fn desktop_reviewer_control_reservation_rejects_unfit_fixed_contract_before_model_dispatch() {
    let transport = Arc::new(ReadReviewTransport {
        scenario: ReadScenario::RepeatedRead,
        requests: Mutex::new(vec![]),
    });
    let mut model = reviewer(transport.clone());
    model.client = model.client.with_request_budget(RequestBudget {
        context_window_tokens: 1_000_000,
        reserved_output_tokens: 16_384,
        safety_margin_tokens: 1_024,
    });
    let mut input = read_review_request();
    input.proposal.answer_markdown = "a".repeat(129_000);
    let error = model.review(input).await.unwrap_err();
    assert_eq!(error.message, "reviewer_initial_read_budget_exceeded");
    assert!(transport.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn desktop_reviewer_cannot_read_after_the_final_submit_only_boundary() {
    let transport = Arc::new(ReadReviewTransport {
        scenario: ReadScenario::ReadAfterDisabled,
        requests: Mutex::new(vec![]),
    });
    let error = reviewer(transport.clone())
        .review(read_review_request())
        .await
        .unwrap_err();
    assert!(!error.retryable);
    assert_eq!(error.class, ModelErrorClassV4::InvalidResponse);
    assert_eq!(
        error.message,
        "model endpoint failed: responses_tool_not_granted"
    );
    assert_eq!(transport.requests.lock().unwrap().len(), 8);
}

struct ProgressReviewTransport {
    inner: Arc<ReadReviewTransport>,
    observed: Arc<AtomicUsize>,
}

#[async_trait]
impl ResponsesTransport for ProgressReviewTransport {
    async fn post(
        &self,
        endpoint: url::Url,
        headers: reqwest::header::HeaderMap,
        body: Value,
        timeout: Duration,
    ) -> AdapterResult<ResponsesHttpStream> {
        let response = self.inner.post(endpoint, headers, body, timeout).await?;
        let mut body = response.body;
        let mut wire = Vec::new();
        while let Some(chunk) = body.next().await {
            wire.extend(chunk?);
        }
        let text = std::str::from_utf8(&wire).unwrap();
        let terminal: Value =
            serde_json::from_str(text.trim().strip_prefix("data:").unwrap().trim()).unwrap();
        let call = &terminal["response"]["output"][0];
        let prefix=[
            json!({"type":"response.reasoning_text.delta","delta":"private review fixture reasoning"}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"id":call["id"],"type":"function_call","call_id":call["call_id"],"name":call["name"],"arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":call["id"],"delta":call["arguments"]}),
        ].into_iter().map(|event|Ok(format!("data: {event}\n\n").into_bytes()));
        let observed = self.observed.clone();
        let before = self.observed.load(Ordering::SeqCst);
        let terminal = stream::once(async move {
            assert!(
                observed.load(Ordering::SeqCst) > before,
                "review stream progress must reach its caller before the terminal report chunk"
            );
            Ok(wire)
        });
        Ok(ResponsesHttpStream {
            status: response.status,
            body: Box::pin(stream::iter(prefix).chain(terminal)),
        })
    }
}

#[tokio::test]
async fn desktop_reviewer_progress_bridges_live_selected_reviewer_pages_and_final_submission() {
    let observed = Arc::new(AtomicUsize::new(0));
    let selected_requests = Arc::new(ReadReviewTransport {
        scenario: ReadScenario::ReadThenSubmit,
        requests: Mutex::new(vec![]),
    });
    let selected = reviewer(Arc::new(ProgressReviewTransport {
        inner: selected_requests.clone(),
        observed: observed.clone(),
    }));
    let executor_requests = Arc::new(ReviewTransport {
        report: json!({"schema_version":4,"summary":"executor must not review","findings":[]}),
        requests: Mutex::new(vec![]),
    });
    let mut executor = reviewer(executor_requests.clone());
    executor.reviewer = Some(Box::new(selected));
    let mut kinds = Vec::new();
    let mut progress_bytes = Vec::new();
    let report = executor
        .review_with_progress(read_review_request(), &mut |event| match event {
            ModelStreamEventV4::ReasoningDelta(_) => {
                observed.fetch_add(1, Ordering::SeqCst);
                kinds.push("reasoning");
            }
            ModelStreamEventV4::ContentProgress { bytes } if bytes > 0 => {
                progress_bytes.push(bytes);
                observed.fetch_add(1, Ordering::SeqCst);
                kinds.push("arguments");
            }
            ModelStreamEventV4::Activity(ModelActivityPhaseV4::ToolCall) => kinds.push("tool"),
            _ => {}
        })
        .await
        .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "review_evidence_partial")
    );
    assert_eq!(
        selected_requests.requests.lock().unwrap().len(),
        2,
        "one page request and one final submission"
    );
    assert!(
        executor_requests.requests.lock().unwrap().is_empty(),
        "the frozen independent reviewer must receive the request"
    );
    assert!(
        !kinds.contains(&"reasoning"),
        "Responses normalizes private reasoning to byte progress, not reasoning text"
    );
    assert_eq!(
        progress_bytes.len(),
        4,
        "reasoning and argument progress for both review turns"
    );
    for round in progress_bytes.chunks_exact(2) {
        assert_eq!(round[0], "private review fixture reasoning".len() as u32);
        assert!(round[1] > 0);
    }
    assert!(kinds.iter().filter(|kind| **kind == "tool").count() >= 2);
}

#[tokio::test]
async fn desktop_reviewer_http_reasoning_content_reaches_supervisor_before_report_completion() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = url::Url::parse(&format!("http://{}/v1", listener.local_addr().unwrap())).unwrap();
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let count = socket.read(&mut buffer).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() - header_end < length {
            let count = socket.read(&mut buffer).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
        }
        let request: Value =
            serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        let report = json!({"schema_version":4,"summary":"Reviewed the supplied observations","findings":[]});
        let reasoning = format!(
            "data: {}\n\n",
            json!({"choices":[{"delta":{"reasoning_content":"private HTTP reviewer fixture reasoning"}}]})
        );
        let arguments = format!(
            "data: {}\n\n",
            json!({"choices":[{"delta":{"tool_calls":[{
                "index":0,"id":"http-review","type":"function","function":{"name":request["tools"][0]["function"]["name"],"arguments":report.to_string()}
            }]}}]})
        );
        let terminal = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",reasoning.len()+arguments.len()+terminal.len(),reasoning).unwrap();
        socket.flush().unwrap();
        progress_rx.recv_timeout(Duration::from_secs(5)).expect(
            "HTTP reasoning delta must reach review supervisor while the report is still streaming",
        );
        socket.write_all(arguments.as_bytes()).unwrap();
        socket.write_all(terminal.as_bytes()).unwrap();
    });
    let unused = Arc::new(ReviewTransport {
        report: json!({}),
        requests: Mutex::new(vec![]),
    });
    let mut model = reviewer(unused.clone());
    model.client = ModelClient::Http(
        UnifiedModelClient::new(
            model.usage_metadata.model_profile_id,
            ProviderProtocol::OpenAiCompatible,
            base,
            "deepseek-fixture",
            Some("not-a-real-key".into()),
        )
        .unwrap()
        .with_request_budget(RequestBudget {
            context_window_tokens: 100_000,
            reserved_output_tokens: 16_384,
            safety_margin_tokens: 1_024,
        }),
    );
    let mut kinds = Vec::new();
    let report = model
        .review_with_progress(request(), &mut |event| match event {
            ModelStreamEventV4::ReasoningDelta(_) => {
                let _ = progress_tx.send(());
                kinds.push("reasoning");
            }
            ModelStreamEventV4::Activity(ModelActivityPhaseV4::ToolCall) => kinds.push("arguments"),
            _ => {}
        })
        .await
        .unwrap();
    server.join().unwrap();
    assert!(report.findings.is_empty());
    assert!(kinds.contains(&"reasoning"));
    assert!(kinds.contains(&"arguments"));
    assert!(unused.requests.lock().unwrap().is_empty());
}

//! Reviewer schema errors are recoverable reports, never executor tool retries.
use super::*;
use futures_util::stream;
use omicsops_adapters::{
    AdapterResult,
    claude_code::{ClaudePreflight, ClaudeProcessRunner},
    codex_auth::{CodexCredentialCoordinator, CodexDeviceAuth, SystemCodexAuthClock},
    credentials::MemoryCredentialVault,
    model_client::ModelClientServices,
    responses::{ResponsesHttpStream, ResponsesTransport},
};
use omicsops_process::managed_child::{BackgroundLaunchSpec, ManagedBackgroundChild};
use std::{sync::Mutex, time::Duration};

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
    ForgedHash,
    ForgedFinding,
    RepeatedRead,
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
        let first: Value =
            serde_json::from_str(requests[0]["input"][0]["content"].as_str().unwrap()).unwrap();
        let mut arguments = json!({
            "snapshot_sha256":first["_review_projection"]["snapshot_sha256"],
            "source_id":"evidence:0", "source_sha256":first["verified_evidence"][0]["payload"]["source_sha256"],
            "pointer":"/outcome/data/answer", "offset":0,"limit":8192
        });
        let report = json!({"schema_version":4,"summary":"Checked the supplied observation","findings":[{
            "severity":"ok","code":"observed_fact","message":"The supplied fact is present","evidence":["event:7"]
        }]});
        let call = |name: &Value, args: Value, id: &str| json!({"id":format!("item-{id}"),"type":"function_call","call_id":id,"name":name,"arguments":args.to_string()});
        let submit = call(&body["tools"][0]["name"], report.clone(), "submit");
        let read_name = &body["tools"][1]["name"];
        let output = match self.scenario {
            ReadScenario::ReadThenSubmit if index % 2 == 1 => vec![submit],
            ReadScenario::ReadThenSubmit | ReadScenario::RepeatedRead => {
                vec![call(read_name, arguments, "read")]
            }
            ReadScenario::Mixed => vec![call(read_name, arguments, "read"), submit],
            ReadScenario::MultipleSubmit => vec![
                submit,
                call(&body["tools"][0]["name"], report, "submit-again"),
            ],
            ReadScenario::Unknown => vec![call(&json!("ungranted_tool"), json!({}), "unknown")],
            ReadScenario::ForgedHash => {
                arguments["source_sha256"] = json!("0".repeat(64));
                vec![call(read_name, arguments, "read")]
            }
            ReadScenario::ForgedFinding => {
                let mut report = report;
                report["findings"][0]["evidence"] = json!(["event:99999"]);
                vec![call(&body["tools"][0]["name"], report, "submit")]
            }
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
        ReadScenario::ForgedHash,
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
    let failure = reviewer(transport.clone())
        .review(read_review_request())
        .await
        .unwrap_err();
    assert_eq!(failure.message, "reviewer_turn_budget_exceeded");
    assert_eq!(transport.requests.lock().unwrap().len(), 8);
}

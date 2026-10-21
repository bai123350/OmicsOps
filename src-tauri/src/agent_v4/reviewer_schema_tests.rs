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

fn reviewer(transport: Arc<ReviewTransport>) -> DesktopModelPortV4 {
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
    let client =
        crate::commands::model_client_for_profile_with_services(&profile, services).unwrap();
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

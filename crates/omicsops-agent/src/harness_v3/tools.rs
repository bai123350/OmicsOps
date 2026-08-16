use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolContractErrorV3 {
    #[error("tool arguments must be an object")]
    ExpectedObject,
    #[error("missing required tool argument {0}")]
    MissingRequired(String),
    #[error("tool argument {path} must be {expected}")]
    InvalidType { path: String, expected: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevelV3 {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConcurrencyV3 {
    ParallelReadOnly,
    Serial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinitionV3 {
    pub id: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub capabilities: Vec<String>,
    pub risk: RiskLevelV3,
    pub read_only: bool,
    pub concurrency: ToolConcurrencyV3,
    pub timeout_ms: u64,
}

impl ToolDefinitionV3 {
    pub fn mcp(
        server_id: impl AsRef<str>,
        tool: impl AsRef<str>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            id: format!("mcp::{}::{}", server_id.as_ref(), tool.as_ref()),
            description: description.into(),
            input_schema,
            output_schema: Value::Object(Default::default()),
            capabilities: vec!["launch_mcp".into()],
            risk: RiskLevelV3::Medium,
            read_only: false,
            concurrency: ToolConcurrencyV3::Serial,
            timeout_ms: 30_000,
        }
    }

    pub fn validate_arguments(&self, arguments: &Value) -> Result<(), ToolContractErrorV3> {
        let object = arguments
            .as_object()
            .ok_or(ToolContractErrorV3::ExpectedObject)?;
        if let Some(required) = self.input_schema.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(name) {
                    return Err(ToolContractErrorV3::MissingRequired(name.into()));
                }
            }
        }
        let properties = self
            .input_schema
            .get("properties")
            .and_then(Value::as_object);
        if let Some(properties) = properties {
            for (name, value) in object {
                let Some(expected) = properties
                    .get(name)
                    .and_then(|schema| schema.get("type"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let valid = match expected {
                    "string" => value.is_string(),
                    "number" => value.is_number(),
                    "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
                    "boolean" => value.is_boolean(),
                    "object" => value.is_object(),
                    "array" => value.is_array(),
                    "null" => value.is_null(),
                    _ => true,
                };
                if !valid {
                    return Err(ToolContractErrorV3::InvalidType {
                        path: name.clone(),
                        expected: expected.into(),
                    });
                }
            }
        }
        Ok(())
    }
}

pub fn builtin_tool_definitions_v3() -> Vec<ToolDefinitionV3> {
    let definition = |id: &str,
                      description: &str,
                      input_schema: Value,
                      capabilities: Vec<String>,
                      risk: RiskLevelV3,
                      read_only: bool,
                      timeout_ms: u64| ToolDefinitionV3 {
        id: id.into(),
        description: description.into(),
        input_schema,
        output_schema: json!({"type":"object"}),
        capabilities,
        risk,
        read_only,
        concurrency: if read_only {
            ToolConcurrencyV3::ParallelReadOnly
        } else {
            ToolConcurrencyV3::Serial
        },
        timeout_ms,
    };
    vec![
        definition(
            "remote.list",
            "List files below a project-relative remote directory.",
            json!({"type":"object","properties":{"path":{"type":"string"},"max_entries":{"type":"integer"}}}),
            vec!["remote_read".into()],
            RiskLevelV3::Low,
            true,
            30_000,
        ),
        definition(
            "remote.read",
            "Read a bounded UTF-8 preview from a project-relative remote file.",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"},"max_bytes":{"type":"integer"}}}),
            vec!["remote_read".into()],
            RiskLevelV3::Low,
            true,
            30_000,
        ),
        definition(
            "remote.write",
            "Atomically write UTF-8 content to a project-relative remote file.",
            json!({"type":"object","required":["path","content"],"properties":{"path":{"type":"string"},"content":{"type":"string"}}}),
            vec!["remote_write".into()],
            RiskLevelV3::High,
            false,
            30_000,
        ),
        definition(
            "remote.exec",
            "Execute an approved command as the dedicated low-privilege SSH user.",
            json!({"type":"object","required":["command"],"properties":{"command":{"type":"string"},"timeout_ms":{"type":"integer"}}}),
            vec!["execute_remote".into()],
            RiskLevelV3::High,
            false,
            300_000,
        ),
        definition(
            "kernel.execute",
            "Execute exploratory code in the isolated project Kernel.",
            json!({"type":"object","required":["language","code"],"properties":{"language":{"type":"string"},"code":{"type":"string"}}}),
            vec!["kernel_execute".into()],
            RiskLevelV3::Medium,
            false,
            300_000,
        ),
        definition(
            "artifact.verify",
            "Verify an artifact's project-relative path, size, and SHA-256.",
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"},"expected_sha256":{"type":"string"},"minimum_bytes":{"type":"integer"}}}),
            vec!["remote_read".into()],
            RiskLevelV3::Low,
            true,
            30_000,
        ),
        definition(
            "agent.request_input",
            "Pause the run and request a concise answer from the user.",
            json!({"type":"object","required":["question"],"properties":{"question":{"type":"string"},"reason":{"type":"string"}}}),
            vec!["request_input".into()],
            RiskLevelV3::Low,
            false,
            30_000,
        ),
        definition(
            "agent.complete",
            "Propose completion with criterion evidence and verified artifacts.",
            json!({"type":"object","required":["criteria","artifacts"],"properties":{"criteria":{"type":"array"},"artifacts":{"type":"array"},"summary":{"type":"string"}}}),
            vec!["complete_run".into()],
            RiskLevelV3::Low,
            false,
            30_000,
        ),
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRequestV3 {
    pub call_id: String,
    pub tool_id: String,
    pub arguments: Value,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcomeStatusV3 {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Rejected,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutcomeV3 {
    pub call_id: String,
    pub status: ToolOutcomeStatusV3,
    pub model_content: String,
    pub structured_result: Option<Value>,
    pub error: Option<String>,
    pub truncated: bool,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityDecisionV3 {
    Allowed,
    ApprovalRequired { summary: String },
    Denied { reason: String },
}

pub trait ToolAuthorityV3: Send + Sync {
    fn authorize(
        &self,
        definition: &ToolDefinitionV3,
        request: &ToolCallRequestV3,
    ) -> AuthorityDecisionV3;
}

#[derive(Debug, Clone)]
pub struct StaticToolAuthorityV3(pub AuthorityDecisionV3);

impl ToolAuthorityV3 for StaticToolAuthorityV3 {
    fn authorize(
        &self,
        _definition: &ToolDefinitionV3,
        _request: &ToolCallRequestV3,
    ) -> AuthorityDecisionV3 {
        self.0.clone()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CancellationTokenV3 {
    cancelled: Arc<AtomicBool>,
}

impl CancellationTokenV3 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[async_trait]
pub trait ToolRuntimeV3: Send + Sync {
    async fn execute(
        &self,
        definition: &ToolDefinitionV3,
        request: &ToolCallRequestV3,
        cancellation: &CancellationTokenV3,
    ) -> ToolOutcomeV3;
}

pub trait ToolAuditSinkV3: Send + Sync {
    fn dispatched(&self, definition: &ToolDefinitionV3, request: &ToolCallRequestV3);
    fn completed(&self, definition: &ToolDefinitionV3, outcome: &ToolOutcomeV3);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopToolAuditSinkV3;

impl ToolAuditSinkV3 for NoopToolAuditSinkV3 {
    fn dispatched(&self, _definition: &ToolDefinitionV3, _request: &ToolCallRequestV3) {}
    fn completed(&self, _definition: &ToolDefinitionV3, _outcome: &ToolOutcomeV3) {}
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolRouterErrorV3 {
    #[error("tool {0} is not registered in the frozen run specification")]
    UnknownTool(String),
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("tool approval is required: {0}")]
    ApprovalRequired(String),
    #[error("tool authority denied the call: {0}")]
    Denied(String),
    #[error("tool call was cancelled")]
    Cancelled,
    #[error("tool router configuration is invalid: {0}")]
    InvalidConfiguration(String),
    #[error("tool output is invalid: {0}")]
    InvalidOutput(String),
}

pub struct ToolRouterV3 {
    definitions: BTreeMap<String, ToolDefinitionV3>,
    authority: Arc<dyn ToolAuthorityV3>,
    runtime: Arc<dyn ToolRuntimeV3>,
    audit: Arc<dyn ToolAuditSinkV3>,
    read_permits: Arc<Semaphore>,
    side_effect_permit: Mutex<()>,
    successful_outcomes: StdMutex<BTreeMap<String, ToolOutcomeV3>>,
}

impl ToolRouterV3 {
    pub fn new(
        definitions: Vec<ToolDefinitionV3>,
        authority: Arc<dyn ToolAuthorityV3>,
        runtime: Arc<dyn ToolRuntimeV3>,
        audit: Arc<dyn ToolAuditSinkV3>,
        max_parallel_read_only: usize,
    ) -> Result<Self, ToolRouterErrorV3> {
        if max_parallel_read_only == 0 {
            return Err(ToolRouterErrorV3::InvalidConfiguration(
                "read-only concurrency must be positive".into(),
            ));
        }
        let mut by_id = BTreeMap::new();
        for definition in definitions {
            let id = definition.id.clone();
            if id.trim().is_empty() || by_id.insert(id.clone(), definition).is_some() {
                return Err(ToolRouterErrorV3::InvalidConfiguration(format!(
                    "duplicate or empty tool id {id}"
                )));
            }
        }
        Ok(Self {
            definitions: by_id,
            authority,
            runtime,
            audit,
            read_permits: Arc::new(Semaphore::new(max_parallel_read_only)),
            side_effect_permit: Mutex::new(()),
            successful_outcomes: StdMutex::new(BTreeMap::new()),
        })
    }

    pub async fn execute(
        &self,
        request: ToolCallRequestV3,
        cancellation: CancellationTokenV3,
    ) -> Result<ToolOutcomeV3, ToolRouterErrorV3> {
        if cancellation.is_cancelled() {
            return Err(ToolRouterErrorV3::Cancelled);
        }
        let definition = self
            .definitions
            .get(&request.tool_id)
            .cloned()
            .ok_or_else(|| ToolRouterErrorV3::UnknownTool(request.tool_id.clone()))?;
        definition
            .validate_arguments(&request.arguments)
            .map_err(|error| ToolRouterErrorV3::InvalidArguments(error.to_string()))?;
        match self.authority.authorize(&definition, &request) {
            AuthorityDecisionV3::Allowed => {}
            AuthorityDecisionV3::ApprovalRequired { summary } => {
                return Err(ToolRouterErrorV3::ApprovalRequired(summary));
            }
            AuthorityDecisionV3::Denied { reason } => {
                return Err(ToolRouterErrorV3::Denied(reason));
            }
        }

        if let Some(mut cached) = self
            .successful_outcomes
            .lock()
            .expect("tool outcome cache lock")
            .get(&request.idempotency_key)
            .cloned()
        {
            cached.call_id = request.call_id;
            return Ok(cached);
        }

        self.audit.dispatched(&definition, &request);
        let duration = Duration::from_millis(definition.timeout_ms.max(1));
        let execution = async {
            if definition.read_only && definition.concurrency == ToolConcurrencyV3::ParallelReadOnly
            {
                let _permit = self
                    .read_permits
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| ToolRouterErrorV3::Cancelled)?;
                Ok(self
                    .runtime
                    .execute(&definition, &request, &cancellation)
                    .await)
            } else {
                let _permit = self.side_effect_permit.lock().await;
                Ok(self
                    .runtime
                    .execute(&definition, &request, &cancellation)
                    .await)
            }
        };
        let mut outcome = match tokio::time::timeout(duration, execution).await {
            Ok(result) => result?,
            Err(_) => ToolOutcomeV3 {
                call_id: request.call_id.clone(),
                status: ToolOutcomeStatusV3::TimedOut,
                model_content: format!(
                    "{} timed out after {} ms",
                    definition.id, definition.timeout_ms
                ),
                structured_result: None,
                error: Some("tool timeout".into()),
                truncated: false,
                provenance: Vec::new(),
            },
        };
        if let Some(result) = &outcome.structured_result {
            validate_schema_value(&definition.output_schema, result)
                .map_err(ToolRouterErrorV3::InvalidOutput)?;
        }
        let (preview, truncated) = truncate_utf8(&outcome.model_content, 32 * 1024);
        outcome.model_content = preview;
        outcome.truncated |= truncated;
        self.audit.completed(&definition, &outcome);
        if outcome.status == ToolOutcomeStatusV3::Succeeded
            && !request.idempotency_key.trim().is_empty()
        {
            self.successful_outcomes
                .lock()
                .expect("tool outcome cache lock")
                .insert(request.idempotency_key, outcome.clone());
        }
        Ok(outcome)
    }
}

fn validate_schema_value(schema: &Value, value: &Value) -> Result<(), String> {
    let Some(expected) = schema.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    let valid = match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(format!("expected {expected}"))
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.into(), false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), true)
}

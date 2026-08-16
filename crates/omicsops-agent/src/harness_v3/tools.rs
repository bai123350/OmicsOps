use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

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

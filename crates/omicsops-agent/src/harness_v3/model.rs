use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ModelMessage;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelToolSpec {
    pub id: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequestV2 {
    pub system: String,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ModelToolSpec>,
    #[serde(default)]
    pub require_strict_json_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelStreamEventV2 {
    TextDelta {
        text: String,
    },
    ToolCallStarted {
        call_id: String,
        index: u32,
        tool_id: String,
    },
    ToolArgumentsDelta {
        call_id: String,
        index: u32,
        arguments: String,
    },
    ToolCallCompleted {
        call_id: String,
        index: u32,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        #[serde(default)]
        provider_json: Value,
    },
    Retrying {
        attempt: u8,
        delay_ms: u64,
        message: String,
    },
    Error {
        code: String,
        message: String,
        repair_attempted: bool,
    },
    Completed,
}

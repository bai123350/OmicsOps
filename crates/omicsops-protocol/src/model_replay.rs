use crate::ToolCallV4;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelReplayItemV4 {
    AssistantText {
        text: String,
    },
    ToolCall {
        call: ToolCallV4,
    },
    ToolResult {
        call_id: String,
        output: String,
    },
    ResponsesReasoning {
        id: String,
        encrypted_content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelReplayBindingV4 {
    pub model_profile_id: Uuid,
    pub configuration_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderContinuationV4 {
    pub items: Vec<ModelReplayItemV4>,
}

impl ModelProviderContinuationV4 {
    pub fn validate(&self) -> Result<(), String> {
        if self.items.len() > 16
            || serde_json::to_vec(self)
                .map_err(|_| "invalid continuation")?
                .len()
                > 1024 * 1024
        {
            return Err("provider continuation exceeds its limit".into());
        }
        let mut calls = std::collections::BTreeSet::new();
        for item in &self.items {
            match item {
                ModelReplayItemV4::ResponsesReasoning {
                    id,
                    encrypted_content,
                } => {
                    if id.is_empty()
                        || id.len() > 256
                        || encrypted_content.is_empty()
                        || encrypted_content.len() > 256 * 1024
                    {
                        return Err("invalid opaque reasoning continuation".into());
                    }
                }
                ModelReplayItemV4::ToolCall { call } => {
                    if call.call_id.is_empty()
                        || call.call_id.len() > 256
                        || call.tool_id.is_empty()
                        || !call.arguments.is_object()
                        || !calls.insert(&call.call_id)
                    {
                        return Err("invalid continuation tool call".into());
                    }
                }
                ModelReplayItemV4::ToolResult { .. } => {
                    return Err("provider cannot supply host tool results".into());
                }
                ModelReplayItemV4::AssistantText { .. } => {}
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelReplayRecordedV4 {
    pub logical_request_id: Uuid,
    pub attempt_id: Uuid,
    pub binding: ModelReplayBindingV4,
    pub continuation: ModelProviderContinuationV4,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_replay_continuation_bounds_and_rejects_provider_tool_results() {
        let one = ModelReplayItemV4::ResponsesReasoning {
            id: "rs_1".into(),
            encrypted_content: "opaque_fixture".into(),
        };
        assert!(
            ModelProviderContinuationV4 {
                items: vec![one.clone(); 16]
            }
            .validate()
            .is_ok()
        );
        assert!(
            ModelProviderContinuationV4 {
                items: vec![one; 17]
            }
            .validate()
            .is_err()
        );
        assert!(
            ModelProviderContinuationV4 {
                items: vec![ModelReplayItemV4::ToolResult {
                    call_id: "c".into(),
                    output: "provider fabricated".into()
                }]
            }
            .validate()
            .is_err()
        );
        assert!(
            ModelProviderContinuationV4 {
                items: vec![ModelReplayItemV4::ResponsesReasoning {
                    id: "rs".into(),
                    encrypted_content: "x".repeat(256 * 1024 + 1)
                }]
            }
            .validate()
            .is_err()
        );
        assert!(serde_json::from_str::<ModelReplayItemV4>(r#"{"kind":"responses_reasoning","id":"rs","encrypted_content":"opaque","text":"PRIVATE_REASONING_SENTINEL"}"#).is_err());
    }
}

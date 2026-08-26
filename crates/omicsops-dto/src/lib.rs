//! Data-only DTOs shared by the native Tauri boundary and the web client.
//!
//! This crate intentionally has no desktop, UI, database, or async runtime
//! dependencies so it remains usable from native and `wasm32` consumers.

use omicsops_protocol::{ComputeSelectionV4, ExecutionPlanV4};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Conversation-scoped Agent Runtime V4 mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAgentModeV4 {
    /// Direct Agent execution mode (the legacy/default behavior).
    #[default]
    Agent,
    /// Plan-first mode, where execution requires plan approval.
    Plan,
}

/// Request shape shared by mode readers even though the Tauri command keeps
/// its historical two-argument boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetConversationAgentModeRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
}

/// Response returned by `get_conversation_agent_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetConversationAgentModeResponseV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: SessionAgentModeV4,
}

/// Request accepted by `set_conversation_agent_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SetConversationAgentModeRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: SessionAgentModeV4,
}

/// Response returned by `set_conversation_agent_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SetConversationAgentModeResponseV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: SessionAgentModeV4,
}

/// Summary returned by Agent Runtime V4 commands.
///
/// The two mode/revision fields are additive and optional. Keeping them
/// optional lets clients deserialize summaries emitted before conversation
/// modes were persisted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSummaryV4 {
    pub run_id: Uuid,
    pub status: String,
    pub plan: Option<ExecutionPlanV4>,
    pub plan_hash: Option<String>,
    pub compute_selection: Option<ComputeSelectionV4>,
    pub approval_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<SessionAgentModeV4>,
}

// Keep aliases for the two common naming orders used by existing OmicsOps
// request/response types. They are type aliases, so they cannot diverge on
// the wire and remain source-compatible for boundary consumers.
pub type GetConversationAgentModeV4Request = GetConversationAgentModeRequestV4;
pub type GetConversationAgentModeV4Response = GetConversationAgentModeResponseV4;
pub type SetConversationAgentModeV4Request = SetConversationAgentModeRequestV4;
pub type SetConversationAgentModeV4Response = SetConversationAgentModeResponseV4;
pub type GetConversationAgentModeRequest = GetConversationAgentModeRequestV4;
pub type GetConversationAgentModeResponse = GetConversationAgentModeResponseV4;
pub type SetConversationAgentModeRequest = SetConversationAgentModeRequestV4;
pub type SetConversationAgentModeResponse = SetConversationAgentModeResponseV4;
pub type ConversationAgentModeGetRequestV4 = GetConversationAgentModeRequestV4;
pub type ConversationAgentModeGetResponseV4 = GetConversationAgentModeResponseV4;
pub type ConversationAgentModeSetRequestV4 = SetConversationAgentModeRequestV4;
pub type ConversationAgentModeSetResponseV4 = SetConversationAgentModeResponseV4;
pub type ConversationAgentModeRequestV4 = SetConversationAgentModeRequestV4;
pub type ConversationAgentModeResponseV4 = SetConversationAgentModeResponseV4;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mode_wire_values_are_lowercase_and_default_to_agent() {
        assert_eq!(
            serde_json::to_value(SessionAgentModeV4::Agent).unwrap(),
            json!("agent")
        );
        assert_eq!(
            serde_json::to_value(SessionAgentModeV4::Plan).unwrap(),
            json!("plan")
        );
        assert_eq!(SessionAgentModeV4::default(), SessionAgentModeV4::Agent);
    }

    #[test]
    fn old_run_summary_is_backward_compatible() {
        let summary: RunSummaryV4 = serde_json::from_value(json!({
            "run_id": Uuid::nil(),
            "status": "running",
            "plan": null,
            "plan_hash": null,
            "compute_selection": null,
            "approval_hash": null
        }))
        .unwrap();
        assert_eq!(summary.plan_revision, None);
        assert_eq!(summary.session_mode, None);
    }
}

//! Data-only DTOs shared by the native Tauri boundary and the web client.
//!
//! This crate intentionally has no desktop, UI, database, or async runtime
//! dependencies so it remains usable from native and `wasm32` consumers.

use chrono::{DateTime, Utc};
pub use omicsops_protocol::{
    AgentRequestRouteV4, BrowserApprovalBindingV4, BrowserApprovalScopeV4, BrowserAuthorizationV4,
    BrowserSessionKindV4, BrowserTabSummaryV4,
};
use omicsops_protocol::{ComputeSelectionV4, ExecutionPlanV4};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitGuidanceV4Request {
    pub message_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidanceRecordV4 {
    pub message_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub ordinal: u64,
    pub markdown: String,
    pub accepted_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveModelProfileRequest {
    pub id: Option<Uuid>,
    pub label: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub credential: Option<String>,
    pub context_window_tokens: Option<u32>,
    /// Omission preserves the saved request; null restores provider defaults.
    #[serde(
        default,
        deserialize_with = "explicit_nullable_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<Option<String>>,
    /// Omission preserves an existing binding; explicit null restores inheritance.
    #[serde(
        default,
        deserialize_with = "explicit_nullable_uuid",
        skip_serializing_if = "Option::is_none"
    )]
    pub delegated_model_profile_id: Option<Option<Uuid>>,
}

fn explicit_nullable_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

fn explicit_nullable_uuid<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<Uuid>>, D::Error> {
    Option::<Uuid>::deserialize(deserializer).map(Some)
}

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

/// Durable lifecycle of one immutable proposed-plan revision.
///
/// The plan content, hash, and revision number never change after insertion.
/// Status and feedback are lifecycle metadata used to make the latest
/// revision approvable without rewriting an older proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanRevisionStatusV4 {
    Generating,
    Revising,
    Pending,
    Approved,
    Superseded,
    Cancelled,
}

impl PlanRevisionStatusV4 {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Generating | Self::Revising | Self::Pending)
    }
}

/// A persisted, content-addressed plan revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProposedPlanRevisionV4 {
    pub id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub revision: u64,
    pub plan: ExecutionPlanV4,
    pub markdown: String,
    pub plan_hash: String,
    pub status: PlanRevisionStatusV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Backward/forward-compatible aliases used by the native command boundary.
pub type PlanRevisionStatus = PlanRevisionStatusV4;
pub type ProposedPlanRevision = ProposedPlanRevisionV4;

/// Request body for `agent_v4_request_plan_revision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RequestPlanRevisionV4 {
    pub run_id: Uuid,
    pub plan_hash: String,
    pub feedback: String,
}

pub type AgentV4RequestPlanRevisionRequest = RequestPlanRevisionV4;
pub type RequestPlanRevisionRequestV4 = RequestPlanRevisionV4;
pub type PlanRevisionRequestV4 = RequestPlanRevisionV4;

/// Response returned after feedback is durably attached to the current
/// revision. A subsequent planning pass creates the next immutable revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RequestPlanRevisionResponseV4 {
    pub run_id: Uuid,
    pub revision: u64,
    pub plan_hash: String,
    pub status: PlanRevisionStatusV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

pub type AgentV4RequestPlanRevisionResponse = RequestPlanRevisionResponseV4;
pub type AgentV4RequestPlanRevisionResponseV4 = RequestPlanRevisionResponseV4;

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

/// Atomically hydrated conversation-scoped Agent/Plan state.
///
/// The optional fields use defaults so a consumer can safely deserialize an
/// older response that predates durable plan revisions or Agent runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConversationAgentStateV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: SessionAgentModeV4,
    pub locked: bool,
    #[serde(default)]
    pub latest_plan_revision: Option<ProposedPlanRevisionV4>,
    #[serde(default)]
    pub latest_run: Option<RunSummaryV4>,
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

    #[test]
    fn conversation_agent_state_uses_snake_case_and_old_optional_defaults() {
        let project_id = Uuid::from_u128(3);
        let conversation_id = Uuid::from_u128(4);
        let state = ConversationAgentStateV4 {
            project_id,
            conversation_id,
            mode: SessionAgentModeV4::Plan,
            locked: true,
            latest_plan_revision: None,
            latest_run: None,
        };
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            json!({
                "project_id": project_id,
                "conversation_id": conversation_id,
                "mode": "plan",
                "locked": true,
                "latest_plan_revision": null,
                "latest_run": null
            })
        );

        let legacy: ConversationAgentStateV4 = serde_json::from_value(json!({
            "project_id": project_id,
            "conversation_id": conversation_id,
            "mode": "agent",
            "locked": false
        }))
        .unwrap();
        assert_eq!(legacy.latest_plan_revision, None);
        assert_eq!(legacy.latest_run, None);
    }
}

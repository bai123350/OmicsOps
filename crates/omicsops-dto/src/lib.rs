//! Data-only DTOs shared by the native Tauri boundary and the web client.
//!
//! This crate intentionally has no desktop, UI, database, or async runtime
//! dependencies so it remains usable from native and `wasm32` consumers.

use chrono::{DateTime, Utc};
pub use omicsops_protocol::{
    AgentRequestRouteV4, BrowserApprovalBindingV4, BrowserApprovalScopeV4, BrowserAuthorizationV4,
    BrowserSessionKindV4, BrowserTabSummaryV4, CompactContextRequestV4, ContextBudgetV4,
    ContextCompactionReceiptV4, ContextCompactionStatusV4, ContextLimitSourceV4, ContextUsageRowV4,
    ContextUsageSnapshotV4, ContextWindowUsageV4, ConversationAgentPreferencesV4,
    ModelRequestStartedV4, ModelUsageObservationV4, ModelUsageSampleV4, ReviewerBackendChoiceV4,
    ReviewerSettingsV4, SESSION_REVIEW_MAX_ERROR_BYTES, SESSION_REVIEW_MAX_FINDING_CODE_BYTES,
    SESSION_REVIEW_MAX_FINDING_MESSAGE_BYTES, SESSION_REVIEW_MAX_FINDING_SOURCES,
    SESSION_REVIEW_MAX_FINDINGS, SESSION_REVIEW_MAX_PER_PROJECT,
    SESSION_REVIEW_MAX_REPORT_SUMMARY_BYTES, SESSION_REVIEW_MAX_SOURCE_SNAPSHOT_BYTES,
    SESSION_REVIEW_MAX_SOURCE_TEXT_BYTES, SESSION_REVIEW_MAX_SOURCES, SessionReviewBeginResultV4,
    SessionReviewFindingV4, SessionReviewRecordV4, SessionReviewReportV4, SessionReviewRequestV4,
    SessionReviewSeverityV4, SessionReviewSourceV4, SessionReviewStatusV4, UsageAggregationV4,
    UsageObservationStateV4, UsageTotalsV4, session_review_source_snapshot_hash,
    session_review_source_snapshot_value,
};
use omicsops_protocol::{ComputeSelectionV4, ExecutionPlanV4};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationExportFormat {
    Html,
    Png,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationExportRequest {
    pub format: ConversationExportFormat,
    pub content_base64: String,
}

/// Main Agent model iterations. Zero disables the iteration limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentIterationSettingsV4 {
    pub max_iterations: u32,
    #[serde(default)]
    pub auto_continue: bool,
    #[serde(default = "default_auto_continue_limit")]
    pub auto_continue_limit: u32,
    #[serde(default = "default_session_enabled")]
    pub auto_compact: bool,
    #[serde(default = "default_session_enabled")]
    pub follow_up_questions: bool,
}

fn default_auto_continue_limit() -> u32 {
    10
}
fn default_session_enabled() -> bool {
    true
}

impl Default for AgentIterationSettingsV4 {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            auto_continue: false,
            auto_continue_limit: default_auto_continue_limit(),
            auto_compact: true,
            follow_up_questions: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetConversationCapabilitiesV4Request {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
}

/// Read-only availability snapshot, not a grant of execution permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationCapabilitiesV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub skills: Vec<ConversationSkillCapabilityV4>,
    pub mcp_servers: Vec<ConversationMcpCapabilityV4>,
    /// Markdown files directly under the local project's .omicsops/memory directory.
    pub memory_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSkillCapabilityV4 {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMcpCapabilityV4 {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub tool_count: usize,
}

/// A compiled scientific MCP domain; catalog inspection never starts a process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundledMcpPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub description_zh: String,
    pub tool_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddBundledMcpServerRequest {
    pub preset_id: String,
}

/// Transient public output snapshot. None clears an unfinished preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTextPreviewV4 {
    pub run_id: Uuid,
    pub text: Option<String>,
}

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
    /// Explicitly adopt the bundled catalog; ordinary edits preserve saved capabilities.
    #[serde(default)]
    pub refresh_catalog: bool,
    /// Omission preserves the saved request; null restores provider defaults.
    #[serde(
        default,
        deserialize_with = "explicit_nullable_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<Option<String>>,
    /// Omission preserves the stored mode; explicit null clears it; a bool
    /// selects standard (`false`) or Fast (`true`) processing.
    #[serde(
        default,
        deserialize_with = "explicit_nullable_bool",
        skip_serializing_if = "Option::is_none"
    )]
    pub fast_mode: Option<Option<bool>>,
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

    #[test]
    fn fast_mode_request_distinguishes_omission_clear_and_value() {
        let base = serde_json::json!({
            "label": "test",
            "provider": "open_ai_compatible",
            "base_url": "https://api.openai.com/v1",
            "model": "gpt-5.6-luna"
        });
        let missing: SaveModelProfileRequest = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(missing.fast_mode, None);
        assert!(
            serde_json::to_value(missing)
                .unwrap()
                .get("fast_mode")
                .is_none()
        );
        for value in [None, Some(false), Some(true)] {
            let mut payload = base.clone();
            payload["fast_mode"] = serde_json::json!(value);
            let request: SaveModelProfileRequest = serde_json::from_value(payload).unwrap();
            assert_eq!(request.fast_mode, Some(value));
            let encoded = serde_json::to_value(request).unwrap();
            assert_eq!(encoded["fast_mode"], serde_json::json!(value));
        }
    }
}

fn explicit_nullable_bool<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<bool>>, D::Error> {
    Option::<bool>::deserialize(deserializer).map(Some)
}

mod composer_attachments;
mod composer_references;
pub use composer_attachments::{ComposerAttachmentReceipt, StageComposerAttachmentRequest};
pub use composer_references::{
    ComposerCatalogItem, ComposerReference, ComposerTextPreview, CreateComposerQuoteRequest,
};

mod composer_workflows;
pub use composer_workflows::{ComposerWorkflowTemplate, SaveComposerWorkflowRequest};

mod project_templates;
pub use project_templates::{
    QuickAction, SaveQuickActionRequest, SaveSpecialistTemplateRequest, SpecialistTemplate,
};

mod credentials_settings;
pub use credentials_settings::{
    CreateCredentialResult, CreateManagedCredentialRequest, CredentialConsumer, CredentialEntry,
    CredentialPresence, CredentialTarget, CredentialValueKind, DeleteCredentialResult,
    ManagedCredentialMetadata, ReplaceCredentialRequest,
};

mod composer_queue;
pub use composer_queue::{
    ComposerQueueActionRequestV4, ComposerQueueActionV4, ComposerQueueFailureCodeV4,
    ComposerQueueFrozenConfigV4, ComposerQueueItemV4, ComposerQueueMaterialSnapshotV4,
    ComposerQueueModeV4, ComposerQueueStatusV4, ComposerReplacementErrorV4,
    ComposerReplacementFailureKindV4, ComposerReplacementReceiptV4, EnqueueComposerTurnRequestV4,
    ReplaceComposerTurnRequestV4, UpdateComposerQueueRequestV4,
};

mod conversation_branches;
pub use conversation_branches::{
    BranchCreateFailureKindV4, BranchCreateFailureV4, ConversationBranchCheckpointKindV4,
    ConversationBranchCheckpointV4, ConversationBranchSendReceiptV4, ConversationBranchStateV4,
    ConversationBranchV4, CreateConversationBranchAndSendRequestV4,
    CreateConversationBranchRequestV4,
};

mod storage_usage;
pub use storage_usage::{
    StorageScanIssueV4, StorageScanLimitsV4, StorageUsageCategoryV4, StorageUsageEntryV4,
    StorageUsageScopeV4, StorageUsageSnapshotV4, StorageUsageStatusV4,
};

mod usage_settings;
pub use usage_settings::{
    UsageAggregatePage, UsageConversationPage, UsageConversationRow, UsageDay, UsageFilter,
    UsageGroup, UsageTool,
};

mod memory_files;
pub use memory_files::{
    CreateMemoryFileRequestV4, DeleteMemoryFileRequestV4, MemoryFileSummaryV4, MemoryFileV4,
    UpdateMemoryFileRequestV4,
};

mod side_chat;
pub use side_chat::{
    SIDE_CHAT_MAX_ANSWER_BYTES, SIDE_CHAT_MAX_ATTACHMENTS, SIDE_CHAT_MAX_CITATIONS,
    SIDE_CHAT_MAX_EVENT_HEADS, SIDE_CHAT_MAX_LABEL_BYTES, SIDE_CHAT_MAX_MODEL_LABEL_BYTES,
    SIDE_CHAT_MAX_PER_PROJECT, SIDE_CHAT_MAX_QUESTION_BYTES, SIDE_CHAT_MAX_REFERENCES,
    SIDE_CHAT_MAX_ROLE_BYTES, SIDE_CHAT_MAX_SOURCE_ID_BYTES, SIDE_CHAT_MAX_SOURCE_SNAPSHOT_BYTES,
    SIDE_CHAT_MAX_SOURCE_TEXT_BYTES, SIDE_CHAT_MAX_SOURCES, SideChatBeginResultV4,
    SideChatEventHeadV4, SideChatFailureCodeV4, SideChatGetRequestV4, SideChatListRequestV4,
    SideChatSendErrorKindV4, SideChatSendErrorV4, SideChatSendRequestV4, SideChatSourceV4,
    SideChatSourceWatermarkV4, SideChatTurnStatusV4, SideChatTurnV4, side_chat_request_hash,
    side_chat_source_snapshot_hash, side_chat_source_snapshot_value,
};

mod run_stops;
pub use run_stops::{StopRunReceiptV4, StopRunRequestV4, StopRunStatusV4};

/// Distinguish a host preflight rejection from an unconfirmed dispatch result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionReviewStartFailureKindV4 {
    Rejected,
    Uncertain,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReviewStartErrorV4 {
    pub kind: SessionReviewStartFailureKindV4,
    pub message: String,
}

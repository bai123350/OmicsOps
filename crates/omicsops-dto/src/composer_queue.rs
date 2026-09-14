use omicsops_protocol::{
    ComputeSelectionV4, ConversationAgentPreferencesV4, DelegatedModelBindingV4,
    ReviewerModelBindingV4, RunServiceTierV4,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Queue mode captured when the user accepts a composer turn. The host later
/// freezes the model configuration before dispatching the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerQueueModeV4 {
    Agent,
    Plan,
}

/// Durable lifecycle values returned by the queue read API. Pending CRUD in
/// the Store currently creates and mutates only `Pending` rows; the remaining
/// values are reserved for the dispatch/reconciliation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerQueueStatusV4 {
    Pending,
    Dispatching,
    Running,
    Completed,
    Failed,
    Cancelled,
    Uncertain,
}

/// Actions that are safe to apply to a pending row. Dispatch and
/// reconciliation actions stay host-internal and are deliberately absent
/// from this UI-facing mutation contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerQueueActionV4 {
    Cancel,
    MoveUp,
    MoveDown,
    /// Fold a text-only pending item into the currently running ordinary run
    /// as durable guidance. The host derives the guidance message id from the
    /// queue request id, so retries cannot enqueue a second FIFO turn.
    CutIn,
}

/// The request payload accepted by the durable pending queue. Model settings
/// are input hints only at this layer; the host freezes the authoritative
/// configuration before dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnqueueComposerTurnRequestV4 {
    pub request_id: Uuid,
    pub message_id: Uuid,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: ComposerQueueModeV4,
    pub message_markdown: String,
    pub model_profile_id: Uuid,
    pub compute_selection: ComputeSelectionV4,
    pub references: Vec<crate::ComposerReference>,
    pub attachments: Vec<Uuid>,
}

/// Atomically stop this exact observed run and accept a fresh queued turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceComposerTurnRequestV4 {
    pub turn: EnqueueComposerTurnRequestV4,
    pub target_run_id: Uuid,
    pub expected_event_sequence: u64,
    pub expected_event_hash: String,
    pub stop_request_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerReplacementFailureKindV4 {
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerReplacementErrorV4 {
    pub kind: ComposerReplacementFailureKindV4,
    pub message: String,
}

/// Immutable source boundary. Old transcript/evidence remains inspectable;
/// the replacement uses a new run's context and cannot inherit old authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerReplacementReceiptV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub target_run_id: Uuid,
    pub source_message_id: Option<Uuid>,
    pub source_event_sequence: u64,
    pub source_event_hash: String,
    pub stop: crate::StopRunReceiptV4,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
}

/// Full replacement of a pending composer payload. The original enqueue
/// idempotency hash is intentionally not part of this request and is retained
/// by the Store when this update succeeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateComposerQueueRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: u64,
    pub message_markdown: String,
    pub references: Vec<crate::ComposerReference>,
    pub attachments: Vec<Uuid>,
}

/// Scoped CAS action for cancellation, cut-in guidance, or adjacent FIFO
/// movement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueActionRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: u64,
    pub action: ComposerQueueActionV4,
}

/// Host-resolved settings captured before a pending row is inserted. This is
/// an output/storage type; callers must not be allowed to manufacture it as
/// authority for a dispatch. The native host validates this snapshot again
/// before starting the reserved run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueFrozenConfigV4 {
    pub model_profile_id: Uuid,
    pub model_configuration_hash: String,
    pub conversation_preferences: ConversationAgentPreferencesV4,
    pub service_tier: RunServiceTierV4,
    pub delegated_model: Option<DelegatedModelBindingV4>,
    pub reviewer_model: Option<ReviewerModelBindingV4>,
    pub compute_selection: ComputeSelectionV4,
}

/// Host-owned material captured before queue insertion. Reference context is
/// bounded and sanitized for the future dispatcher, but is deliberately not
/// returned by the public queue item to the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueMaterialSnapshotV4 {
    pub reference_context: String,
    pub reference_context_sha256: String,
    pub attachment_receipts: Vec<crate::ComposerAttachmentReceipt>,
    pub attachment_snapshot_sha256: String,
}

/// Bounded host failure classification. Raw provider or filesystem errors are
/// never returned as queue status text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerQueueFailureCodeV4 {
    ConfigurationChanged,
    MaterialChanged,
    DispatchFailed,
    RunFailed,
    CancelledByUser,
    LeaseUncertain,
}

/// Public queue row. Lease owner/token and the future frozen host snapshot
/// stay out of this UI contract; the Store keeps the original request hash
/// privately for idempotency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueItemV4 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_target_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_receipt: Option<ComposerReplacementReceiptV4>,
    pub request_id: Uuid,
    pub message_id: Uuid,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub position: u64,
    pub revision: u64,
    pub mode: ComposerQueueModeV4,
    pub message_markdown: String,
    pub frozen: ComposerQueueFrozenConfigV4,
    pub references: Vec<crate::ComposerReference>,
    pub attachments: Vec<Uuid>,
    pub attachment_receipts: Vec<crate::ComposerAttachmentReceipt>,
    /// Safe receipt for a successful text-only cut-in. This references the
    /// durable guidance row and carries no execution authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_in_message_id: Option<Uuid>,
    pub status: ComposerQueueStatusV4,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<ComposerQueueFailureCodeV4>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

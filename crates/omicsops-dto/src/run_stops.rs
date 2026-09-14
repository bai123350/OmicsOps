use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Local stop intent state. `Observed` only means that the local run already
/// has terminal state; it does not assert that any remote job was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopRunStatusV4 {
    Requested,
    Observed,
}

/// Idempotent request to stop a locally owned Agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopRunRequestV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
}

/// Durable receipt for a local stop intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopRunReceiptV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub status: StopRunStatusV4,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

//! Public detached-job observation contracts. Execution inputs and remote
//! roots, host keys and complete runtime receipts remain on the host.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ListRemoteJobsV4Request {
    pub project_id: Uuid,
    pub conversation_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RefreshRemoteJobV4Request {
    pub project_id: Uuid,
    pub job_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteJobRecordedStateV4 {
    Reserved,
    Running,
    Unknown,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteJobQueryEligibilityV4 {
    NeedsRootCheck,
    BindingChanged,
    Unsupported,
    IntegrityMismatch,
    RecordedReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteJobObservationStatusV4 {
    Running,
    Unknown,
    Completed,
    Timeout,
    Unavailable,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteJobSummaryV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub call_id: String,
    pub backend_id: String,
    pub language: String,
    pub environment: String,
    pub request_sha256: String,
    pub recorded_state: RemoteJobRecordedStateV4,
    pub query_eligibility: RemoteJobQueryEligibilityV4,
    /// Legacy job rows have no timestamp; do not infer one from the run.
    pub recorded_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteJobsPageV4 {
    pub jobs: Vec<RemoteJobSummaryV4>,
    pub limit: u32,
    pub remote_liveness_checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteJobCaptureReferenceV4 {
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteJobObservationV4 {
    pub job: RemoteJobSummaryV4,
    pub status: RemoteJobObservationStatusV4,
    pub observed_at: String,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    pub captures: Vec<RemoteJobCaptureReferenceV4>,
    pub resubmitted: bool,
}

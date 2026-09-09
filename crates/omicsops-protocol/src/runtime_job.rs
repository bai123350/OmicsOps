use super::*;

/// A runtime invocation, distinct from the reusable interpreter session.
/// Reserved/running records do not prove a remote process is still alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeJobStateV4 {
    Reserved,
    Running,
    Unknown,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeJobV4 {
    pub job_id: Uuid,
    pub context: ExecutionContextKeyV4,
    pub call_id: String,
    pub request_sha256: String,
    pub state: RuntimeJobStateV4,
    pub session_id: Option<Uuid>,
    pub result_request_id: Option<Uuid>,
    pub result_sha256: Option<String>,
}

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
    /// Detached SSH job root, frozen at dispatch; absent for legacy kernels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_host_key: Option<String>,
    pub session_id: Option<Uuid>,
    pub result_request_id: Option<Uuid>,
    pub result_sha256: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_job_omits_detached_fields_when_round_tripped() {
        let value = serde_json::json!({"job_id":Uuid::nil(),"context":{"project_id":Uuid::nil(),"run_id":Uuid::nil(),"backend_id":"local","language":"python","environment":"system"},"call_id":"legacy","request_sha256":"a".repeat(64),"state":"running","session_id":null,"result_request_id":null,"result_sha256":null});
        let job: RuntimeJobV4 = serde_json::from_value(value.clone()).unwrap();
        assert!(job.remote_root.is_none() && job.remote_host_key.is_none());
        assert_eq!(serde_json::to_value(job).unwrap(), value);
    }
}
